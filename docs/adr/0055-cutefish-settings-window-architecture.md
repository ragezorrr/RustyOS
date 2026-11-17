# ADR-0055: CutefishOS Split-Pane System Settings Window Architecture

## Status

accepted

## Context

Предыдущее окно `System Settings` в [crates/rusty-compositor/src/shell_render/modals/settings.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/modals/settings.rs) представляло собой компактный `580×432` поповер поверх единой полупрозрачной подложки без контраста материалов, без категоризации сайдбара и без тумблеров `Switch`/групповых карточек. Анализ `cutefishos/settings` (`SettingsWindow`, `Sidebar`, `Page`, `Card`, `ListRow`, `SwitchRow`, `RowIcon`, `ImageSelectionItem`) показал ключевые архитектурные элементы эталона:
1. Разделение материалов: левый сайдбар (`220px`) остаётся полупрозрачным `Liquid Glass`, тогда как правая область страницы (`Page`) рендерится непрозрачным холстом (`#F4F5F9` Light / `#1C1C1E` Dark) с вертикальным `1px` разделителем.
2. Шапка: macOS traffic lights (`Close`/`Minimize`/`Maximize`) и пилюля `Search` в сайдбаре; навигация `← → <Title>` в правой шапке.
3. Цветные `24×24` squircle-бейджи `RowIcon` (`r=6`) под категориями (`Desktop`, `Devices`, `System`) с нейтральной плашкой активного пункта вместо яркой синей заливки и с соблюдением запрета legacy purple/lavender.
4. Групповые карточки `Card` (`r=12`, `#FFFFFF` Light / `#2C2C30` Dark) с внутренними `1px` разделителями, визуальной карточкой превью `Light`/`Dark` + акцентных свотчей и тумблерами `40×22` `Switch`.

## Decision

1. **RUSTYOS-102 (встроенный фолбэк):** Перестроить [crates/rusty-compositor/src/shell_render/modals/settings.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/modals/settings.rs) на split-pane компоновку с плоским сайдбаром (`#ECEDF1` Light / `#262628` Dark), групповыми карточками без рамок и тумблерами `Switch`.
2. **RUSTYOS-103 (самостоятельное окно `apps/rusty-settings`):** Вынести `System Settings` в отдельный Wayland CSD-клиент [apps/rusty-settings](file:///home/anc/RustyOS_Main/apps/rusty-settings/Cargo.toml) (`xdg_toplevel`, `app_id = "org.rustyos.Settings"`, размер по умолчанию `882×600`, минимум `640×500`, перемещение, сворачивание, разворачивание, ресайз за края):
   - плоский серый сайдбар (`230px`, `#ECEDF1` Light / `#262628` Dark) без `Liquid Glass`, круглая пилюля `Search` без рамки (`r=14`), `11` пунктов по категориям `Network` / `Desktop` / `Devices` с градиентными `24×24` иконками;
   - правая шапка `< > <Title>`, фон страницы `#F5F6FA` / `#1C1C1D`, карточки без обводки `#FFFFFF` / `#2C2C2D`, `3` превью `Light / Dark / Auto`, `7` акцентных свотчей, подстраницы `Fonts` / `Icon Theme` / `Cursor Style`;
   - вертикальный скроллинг сайдбара и страницы (`PointerEventKind::Axis`, клавиши `Up`/`Down`/`PageUp`/`PageDown`/`Home`/`End`), клиппинг контента под шапкой (`Canvas::clip`), пропорциональный индикатор скроллбара и защита `hit_test` от перехвата кликов под шапкой;
   - сквозное живое переключение темы `Light`/`Dark`/`Auto` (`theme_mode = "auto"`) с исправлением многострочной доставки `IpcServer::poll` в [crates/rusty-ipc/src/lib.rs](file:///home/anc/RustyOS_Main/crates/rusty-ipc/src/lib.rs), полной инвалидацией `LiquidGlassPipeline::invalidate_all` при смене темы, адаптивными рамками/заголовками SSD в [crates/rusty-compositor/src/shell_render/ssd.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/ssd.rs) и mtime-синхронизацией темы в `rusty-settings`, `rusty-files` и `rusty-updater`.
3. Обновить маршрутизацию запуска/фокусировки `"settings"` в [crates/rusty-compositor/src/launcher.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/launcher.rs), [crates/rusty-compositor/src/shell_render/dock.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/dock.rs), [crates/rusty-compositor/src/state/wm_windows.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/state/wm_windows.rs) и [crates/rusty-compositor/src/state/shell_actions.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/state/shell_actions.rs) (сохраняя headless-фолбэк для unit-тестов без сокета).

## Consequences

- `System Settings` работает как полноценное независимое Wayland-окно без лишних упоминаний сторонних проектов в пользовательском интерфейсе и корректно прокручивается при переполнении контента.
- Смена темы `Light`/`Dark`/`Auto` синхронно обновляет фон, обои, `Dock`, `TopBar`, уведомления, `OSD`, рамки `SSD`-окон, `rusty-settings`, `rusty-files` и `rusty-updater`.
- Подтверждено живыми `--winit` прогонами в режимах `Light` и `Dark`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` и `cargo test --workspace`.
