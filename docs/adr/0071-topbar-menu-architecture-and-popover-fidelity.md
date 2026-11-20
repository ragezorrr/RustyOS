# 0071. TopBar Menu Architecture, Active App Pill and Popover Visual Fidelity

- **Status:** accepted
- **Date:** 2026-11-20
- **Task:** RUSTYOS-127

## Context

При сопоставлении верхней панели (`TopBar`) и выпадающих меню окружения RustyOS с эталонным дизайном среды рабочего стола были выявлены несоответствия:
1. Когда ни одно прикладное окно не было сфокусировано, в левом слоте `TopBar` по умолчанию отображался профиль `"Files"` (включая пункт меню `Go`), вместо режима рабочего стола `"Desktop"` (без `Go`);
2. Меню активного приложения (`ActiveApp`) и системное меню (`SystemLogo`) содержали устаревшие пункты, сторонние иконки/глифы и не отражали жизненный цикл активного приложения (`Hide {App}`, `Hide Others`, разделитель, `Quit`) с корректным состоянием активности (`enabled: true | false`);
3. Плашка открытого пункта меню в `TopBar` использовала радиус `6 px` вместо `8 px` и единую статичную альфу без адаптации к `Light`/`Dark`;
4. Выпадающий popover не использовал концентрический радиус выделения `9 px` в связке с `14 px` рамкой, `enabled`-фильтрацию при наведении/клике и калиброванную акцентную заливку ховер-пилюли (`alpha = 230` Light / `210` Dark);
5. Снимок экрана при старте `--winit` до события resize мог получить нулевой размер `primary.transformed_physical_size()`, а `save_screenshot_sync` блокировался на `wl-copy` без активного клиента буфера обмена.

## Decision

1. **Профиль рабочего стола `TopBarAppProfile::Desktop` и структура меню (`top_bar_menus.rs`, `top_bar/panel.rs`, `top_bar/menus.rs`, `state/shell_actions.rs`):**
   - Добавлен вариант `TopBarAppProfile::Desktop` как `#[default]`; при отсутствии сфокусированного окна `active_top_bar_app_context()` возвращает `("Desktop", TopBarAppProfile::Desktop)` и скрывает меню `Go` (`has_go_menu() == false`).
   - `build_active_app_menu` формирует унифицированный жизненный цикл: `Hide {App}`, `Hide Others`, разделитель, `Quit` (при активном профиле `Desktop` пункты `Hide Desktop` и `Quit` имеют `enabled: false` и отрисовываются цветом `popover_text_disabled`).
   - В `TopBarMenuAction` добавлены действия `HideActiveApp` и `HideOtherApps`.
   - `MENU_SYSTEM` очищен и упорядочен: `About RustyOS`, `System Settings...`, разделитель, `Suspend`, `Restart...`, `Shut Down...`, разделитель, `Lock Screen`, `Log Out...`.
   - В `TopBarMenuEntry::Item` и `ResolvedTopBarMenuItem::Item` добавлено поле `enabled: bool`; клики (`check_top_bar_menu_popover_click`) и hover (`chrome_shapes.rs`, `text_pass.rs`) игнорируют неактивные пункты.

2. **Визуальный стиль `TopBar` и выпадающих меню (`chrome_shapes.rs`, `text_pass.rs`):**
   - Плашка открытого пункта `TopBar` переведена на радиус `8 px` с адаптивным фоном (`[255, 255, 255, 51]` Dark / `[0, 0, 0, 26]` Light).
   - Контейнер меню использует `MENU_POPOVER_RADIUS = 14 px`, ховер-пилюля — концентрический `HOVER_PILL_RADIUS = 9 px` с акцентным синим фоном (`alpha = 230` Light / `210` Dark) и чисто-белым текстом/глифом при наведении.

3. **Диагностика скриншотов в nested-среде (`frame_composer.rs`, `screenshot.rs`, `winit_backend.rs`):**
   - `process_pending_screenshot` использует `fallback_size` при нулевом `primary_size`.
   - Добавлена поддержка переменной `RUSTYOS_SCREENSHOT_DEST` для прямой синхронной записи PNG без блокировки в `wl-copy` и тестового сценария `RUSTYOS_TEST_CAPTURE_MENU={desktop,system}` для живой проверки в `--winit`.

## Consequences

- `TopBar` и его меню при отсутствии открытых окон соответствуют эталону рабочего стола `Desktop` (`Hide Desktop` / `Hide Others` / `Quit`), а `SystemLogo` — чистому системному меню.
- Исключено выделение и срабатывание отключённых пунктов меню.
- Подтверждено прохождением всех 325 unit-тестов (включая `desktop_active_app_and_system_menu_structure_matches_reference`), `cargo clippy` без предупреждений и живой визуальной проверкой кропов `--winit`.
