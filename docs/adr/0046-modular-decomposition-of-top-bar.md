# ADR-0046: Модульная декомпозиция подсистемы TopBar композитора crates/rusty-compositor/src/shell_render/top_bar по стандартам CutefishOS и macOS 27

- **Статус:** accepted
- **Дата:** 2026-11-17
- **Задача:** `RUSTYOS-090`
- **Связанные решения:** `D-054`, `D-053`, `D-052`, `D-051`, `D-050`, `D-030`, `ADR-0045`, `ADR-0044`, `ADR-0043`, `ADR-0042`

## Контекст

1. **Разрастание монолита `top_bar.rs`:**
   В ходе развития композитора модуль `crates/rusty-compositor/src/shell_render/top_bar.rs` разросся до 2,232 строк кода, объединяя разнородные подсистемы:
   - Базовая панель Menu Bar, расчёт динамической раскладки левой и правой частей, отрисовка полосы воркспейсов и индикаторов;
   - Меню macOS/CutefishOS (системное Apple-like лого, меню активного приложения, File, Edit, View, Go, Window, Help), плавающие Liquid Glass поповеры, шорткаты и хит-тестинг строк меню;
   - Кластер системного статуса: Control Center / Quick Settings триггер, слоты трея Freedesktop StatusNotifierItem (SNI), индикатор обновлений Arch/RustyOS, переключатель раскладки клавиатуры и белые монохромные глифы;
   - Центр уведомлений (Notification HUD): всплывающие тосты, бейдж непрочитанных уведомлений, плавающий OSD пилюльный индикатор громкости/яркости над нижним доком.

2. **Опыт CutefishOS и стандарты macOS 27:**
   Архитектурный анализ репозиториев `cutefishos` (`cutefish-compositor`, `cutefish-toolkit`, `cutefish-core`) показал чёткое модульное разделение верхней панели на независимые компоненты (`panel`, `menu`, `status`, `notifications`), изолирующие бизнес-логику и рендеринг по отдельным файлам до 800 строк.

3. **Требование:**
   - Выполнить декомпозицию монолита `crates/rusty-compositor/src/shell_render/top_bar.rs` в модульный каталог `top_bar/` (`panel.rs`, `menus.rs`, `status.rs`, `notifications.rs`, `mod.rs`);
   - Обеспечить 100% обратную совместимость всех публичных структур, перечислений и функций без единого изменения внешних контрактов в `shell_render/mod.rs`, `state/shell_actions.rs`, `input/pointer.rs` и тестах;
   - Сохранить прохождение 100% тестов рабочего пространства (244/244) и строгую проверку `cargo clippy` (`-D warnings`).

## Решение

Монолитный файл `crates/rusty-compositor/src/shell_render/top_bar.rs` упразднён и заменён на модульный пакет `crates/rusty-compositor/src/shell_render/top_bar/`:

1. **`panel.rs` (661 строк):**
   - Структуры настроек и геометрии панели: `TopBarLayoutOptions`, `TopBarLeftLayout`, `TopBarRightLayout`, телеметрия `TopBarTelemetry`;
   - Вычисление динамической геометрии: `compute_top_bar_left_layout*`, `compute_top_bar_right_layout*`;
   - Отрисовка базовой панели и полосы рабочих пространств: `render_top_bar`, `render_top_bar_workspaces`;
   - Хит-тестинг воркспейсов и бейджа раскладки окон (`check_top_bar_workspace_click*`, `check_top_bar_layout_click*`).

2. **`menus.rs` (759 строк):**
   - Типы меню и действий: `TopBarMenuHit`, `TopBarMenuAction`, `TopBarMenuEntry`, `TopBarMenuPopoverClick`, `MENU_POPOVER_RADIUS`;
   - Статические списки меню (`MENU_SYSTEM`, `MENU_ACTIVE_APP`, `MENU_FILE`, `MENU_EDIT`, `MENU_VIEW`, `MENU_GO`, `MENU_WINDOW`, `MENU_HELP`);
   - Разрешение элементов и геометрии поповеров: `top_bar_menu_entries`, `top_bar_menu_items`, `top_bar_menu_popover_rect*`;
   - Хит-тестинг кликов и наведения по строкам поповеров: `check_top_bar_menu_popover_click*`, `check_top_bar_menu_popover_hover_row*`, `check_top_bar_menu_click*`.

3. **`status.rs` (477 строк):**
   - Типы и константы трея: `TopBarTrayItem`, `MAX_TOP_BAR_TRAY_ITEMS`;
   - Геометрия слотов трея и статусных пилюль: `status_pill_rect`, `kbd_badge_rect`, `tray_slot_rects*`;
   - Хит-тестинг правого кластера: `check_top_bar_tray_click*`, `check_top_bar_kbd_click*`, `check_top_bar_updates_click_for_options`, `check_top_bar_search_click*`, `check_top_bar_status_click*`;
   - Отрисовка монохромных символов статуса: `push_top_bar_right_glyphs`.

4. **`notifications.rs` (353 строк):**
   - Структуры и константы уведомлений и OSD: `NotificationToastView`, `MAX_VISIBLE_NOTIFICATIONS`, `OsdRenderState`;
   - Геометрия и хит-тестинг тостов и бейджа уведомлений: `notification_toast_rects`, `notification_badge_rect`, `check_notification_click`, `check_top_bar_notification_click*`;
   - Логика иконки приложения: `notification_app_icon`;
   - OSD пилюля громкости/яркости: `osd_glyph_kind`, `osd_pill_rect`, `generate_osd_elements`, `push_osd_level_bar`.

5. **`mod.rs` (17 строк):**
   - Объявление внутренних модулей `pub(crate) mod panel; pub(crate) mod menus; pub(crate) mod status; pub(crate) mod notifications;`;
   - Полный реэкспорт всех символов наружу (`pub use menus::*; pub use notifications::*; pub use panel::*; pub use status::*;`).

## Последствия

- Монолит на 2,232 строки разделён на 4 сфокусированных файла размером от 353 до 759 строк.
- Внешний фасад `shell_render/mod.rs` и потребители не потребовали изменений.
- Все 244 unit-теста рабочего пространства проходят успешно (`cargo test --workspace`).
- Код полностью соответствует строгим правилам `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt`.
