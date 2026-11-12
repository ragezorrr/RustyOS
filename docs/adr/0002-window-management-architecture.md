# ADR-0002: Фаза 2 — Архитектура и границы Window Manager (`RUSTYOS-004` / `RUSTYOS-005`)

- Status: accepted
- Date: 2026-11-12
- Decision owner: RustyOS user

## Context

После завершения Фазы 0 (`RUSTYOS-001`), Фазы 1 (`RUSTYOS-002`) и первичной инструментации таймингов (`RUSTYOS-003`) проект переходит к **Фазе 2 — Window Management**.
Согласно `ROADMAP.md` и `ARCHITECTURE.md`:
- Цель Фазы 2: детерминированное управление окнами (outputs, workspaces, windows, focus, layout/placement, stacking, keyboard shortcuts, поддержка нескольких окон и outputs).
- Критерии выхода: модель WM покрыта unit-тестами, focus/layout работают предсказуемо, проверен multi-window сценарий.
- Правило `window-management` skill: «Keep WM state deterministic and testable. Separate policy from low-level rendering. Define focus and stacking rules explicitly. Test multi-window, multi-workspace and multi-output scenarios. Public IPC or WM architecture changes require approval.»

Необходимо принять архитектурное решение о границах модуля/крейта Window Manager и модели размещения окон.

## Options

### Option A (Рекомендуемый): Выделенный чистый крейт `crates/rusty-wm` (без зависимости от `smithay`) + интеграция в `crates/rusty-compositor`

1. **Граница крейтов:**
   - Создаётся `crates/rusty-wm` (zero external dependencies, `std`-only, `#![deny(unsafe_code)]`), содержащий чистую, детерминированную логическую модель управления окнами:
     - Идентификаторы и геометрия (`OutputId`, `WorkspaceId`, `WindowId`, `Rect`).
     - Управление `Outputs` (рабочая область экрана, привязка активного workspace к каждому output).
     - Управление `Workspaces` (1..=9 по умолчанию, переключение активного workspace, перенос окна между workspaces и outputs).
     - Стек окон (`z-order` / stacking: bottom-to-top, raise-on-focus / raise-on-click, fullscreen/maximized/floating/tiled состояния).
     - Детерминированная политика фокуса (фокус при открытии нового окна, возврат фокуса к предыдущему верхнему окну при закрытии/перемещении текущего, цикл `FocusNext` / `FocusPrev`).
     - Режимы раскладки (`LayoutMode`):
       - **Floating (macOS-style default):** каскадное/центрированное размещение новых окон с сохранением пользовательской геометрии, плюс быстрые действия привязки (Left Half, Right Half, Maximize, Center);
       - **Tiled (Master-Stack):** автоматическое деление рабочей области output между окнами активного workspace.
     - Абстрактные команды `WmAction` (например, `FocusNext`, `FocusPrev`, `CloseFocused`, `SwitchWorkspace(u8)`, `MoveFocusedToWorkspace(u8)`, `ToggleLayoutMode`, `SnapFocused(SnapTarget)`, `ToggleMaximize`), которые транслируются из клавиатурных хоткеев в `rusty-compositor`.
   - `crates/rusty-compositor` подключает `rusty-wm`, связывает `WindowId` с `smithay::desktop::Window` (`ToplevelSurface`), синхронизирует геометрию/видимость в `smithay::desktop::Space<Window>` и обрабатывает клавиатурные сочетания (`Alt+Tab` / `Alt+J/K`, `Alt+1..4`, `Alt+Shift+1..4`, `Alt+Space` / `Alt+L`, `Alt+M`, `Alt+Q` и т.д.).

- **Плюсы:**
  - Полное отделение логики и политик WM от низкоуровневого рендеринга и протоколов Wayland (`Separate policy from low-level rendering`).
  - Мгновенные (< 10 мс) детерминированные unit-тесты для multi-window, multi-workspace и multi-output сценариев без необходимости поднимать EGL/Wayland сервер в каждом тесте.
  - Точное соответствие целевой карте `ARCHITECTURE.md` (`crates/rusty-wm/`) при наличии реальной границы и потребителя (`rusty-compositor`).
- **Минусы:**
  - Необходимость поддерживать отображение `WindowId <-> smithay::desktop::Window` в `rusty-compositor` (решается компактной таблицей соответствия в `state.rs`).
- **Риски:** Минимальные.

### Option B: Реализация всей логики WM внутри `crates/rusty-compositor/src/wm.rs` напрямую поверх типов `smithay::desktop::Window`

- **Плюсы:**
  - Не нужно создавать второй крейт в workspace и маппить `WindowId`.
- **Минусы:**
  - Жёсткая привязка логики WM к `smithay::desktop::Window` и `ToplevelSurface`: невозможно легко протестировать multi-window / multi-workspace / multi-output сценарии в чистых unit-тестах без создания фиктивных Wayland-клиентов и поверхностей.
  - Размывает границу между композитором (протоколы/рендер) и политикой управления окнами.

### Option C: Выделение `rusty-wm` в отдельный OS-процесс с IPC уже в Фазе 2

- **Плюсы:** Изоляция процессов.
- **Минусы:** В `ARCHITECTURE.md` явно указано: «На раннем этапе может быть crate внутри compositor process. Выделение в отдельный процесс требует ADR и approval». Вынос WM в отдельный процесс добавит IPC-задержки на каждый кадр/событие ввода и неоправданно усложнит систему.

## Recommendation

Рекомендуется **Option A**:
1. Добавить в `TASKS.md` задачи Фазы 2:
   - `RUSTYOS-004` — Core Window Management Model (`crates/rusty-wm`: outputs, workspaces, stacking, focus, floating + snap + master-stack tiling layouts, unit-тесты multi-window/multi-workspace/multi-output).
   - `RUSTYOS-005` — Compositor WM Integration & Keyboard Shortcuts (интеграция `rusty-wm` в `rusty-compositor`, синхронизация `Space<Window>` и `xdg_toplevel` configure-размеров, клавиатурные сочетания и проверка с несколькими Wayland-клиентами).
2. Реализовать `crates/rusty-wm` как чистый `std`-only крейт внутри процесса композитора и интегрировать его в `crates/rusty-compositor`.

## Decision

Утверждён **Option A** (APPROVE: Option A, 2026-11-12): выделенный чистый `std`-only крейт `crates/rusty-wm` с детерминированной моделью управления окнами, рабочими столами, раскладками (Floating + Snap + Master-Stack Tiling) и интеграцией в `crates/rusty-compositor`.

## Consequences

- В workspace добавляется крейт `crates/rusty-wm` (без новых внешних зависимостей).
- Политика фокуса, стекинга, рабочих столов и раскладок покрывается изолированными unit-тестами.
- `rusty-compositor` получает полноценное клавиатурное и мышиное управление несколькими окнами и рабочими столами.
