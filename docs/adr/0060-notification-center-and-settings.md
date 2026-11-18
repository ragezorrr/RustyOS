# ADR-0060: `Notification Center`, разделение триггеров `Control Center` / `Clock` и редизайн вкладки `Notifications` в `rusty-settings` (`RUSTYOS-109`)

## Status

accepted

## Context

До внедрения `RUSTYOS-109` в оболочке и настройках RustyOS отсутствовал полноценный центр уведомлений:

1. В `TopBar` ([top_bar/panel.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/top_bar/panel.rs)) клик по часам попадал в общую правую зону `Control Center` (`status_hit_x .. clock_x + clock_w`) и открывал `Control Center` вместо отдельной панели уведомлений.
2. Поступающие уведомления существовали только в виде временных баннеров (`notifications`) с коротким TTL и исчезали из интерфейса после скрытия тоста; не было ни долгоживущей истории `notification_history`, ни виджетов правой панели (`Weather`, интерактивный список задач `Reminders`, кнопка `Edit Widgets`).
3. В приложении настроек ([apps/rusty-settings/src/layout.rs](file:///home/anc/RustyOS_Main/apps/rusty-settings/src/layout.rs)) страница `Notifications` содержала только базовый заголовок без секций политик показа уведомлений и без списка настроек по приложениям; в `UserConfig` ([config.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/config.rs)) отсутствовала секция `[notifications]`.

## Decision

1. **Разделение клика `Control Center` и `Clock` в `TopBar` и персистентная история уведомлений:**
   - В [top_bar/panel.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/top_bar/panel.rs) и [top_bar/chrome_shapes.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/top_bar/chrome_shapes.rs) зона `status_hit_x` / `status_hit_w` сужена до блока `Control Center` (`[control_center_x - 5 .. clock_x - 2]`), а для часов добавлена отдельная функция `check_top_bar_clock_click_for_options` (`[clock_x - 2 .. clock_x + clock_w + 8]`) и флаг `notification_center_open` в `TopBarTelemetry` с подсветкой-пилюлей позади часов.
   - В [state/mod.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/state/mod.rs), [state/shell_actions.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/state/shell_actions.rs) и [state/wm_windows.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/state/wm_windows.rs) добавлена история `notification_history`, сохраняемая после истечения TTL всплывающего баннера и очищаемая только при явном `dismiss_notification`, `clear_notifications` или D-Bus `Close`; устаревший синий бейдж `N:k` из `TopBar` убран (`notif_w = 0`), чтобы панель статуса соответствовала `27-Golden-Gate-Notification-Center.png`, а `Notification Center` открывался по клику на `Clock`.

2. **Правая панель `Notification Center` (`ShellOverlayView::NotificationCenter`) без изменений `Liquid Glass`, `Dock` и `Control Center`:**
   - В [shell_render/notification_center.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/notification_center.rs) реализованы `NotificationCenterRenderState`, `NotificationCenterHit`, `NotificationCenterItemView`, `NotificationCardSlot`, `NotificationCenterLayout`, `DEFAULT_NOTIFICATION_TASKS`, `compute_notification_center_layout`, `hit_test_notification_center`, `render_notification_center_text` и `render_notification_center_composite` поверх `GlassPanelStyle::Popover` и стандартных скруглённых карточек/виджетов, ограниченных сверху над `Dock`.
   - Композиция повторяет эталон `27-Golden-Gate-Notification-Center.png`: ведущая верхняя карточка уведомления, строка заголовка **`Notification Center`** с кнопкой очистки `ClearAll`, стопка карточек уведомлений, кэшируемая антиалиасированная карточка **`Weather`** (`r = 22`) с небесно-голубым градиентом и почасовой полосой (`UiGlyphKind::Sun`, `UiGlyphKind::CloudSun` из [surfaces/glyphs.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/glyphs.rs)), интерактивный виджет **`Reminders`** (`UiGlyphKind::Checklist`) с переключением задач и нижняя панель с кнопками **`Edit Widgets`** (переход в `Notifications` в `rusty-settings`) и **`ClosePanel`**.
   - Поддержаны `KeyIntercept::NotificationCenterClose` (`Escape`), взаимное закрытие всех оверлеев и сохранение без изменений [liquid_glass.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/liquid_glass.rs), [sdf.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/sdf.rs), [dock.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/dock.rs) и [quick_settings.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/quick_settings.rs).

3. **Конфигурация `[notifications]` и редизайн `SettingsPage::Notifications` в `apps/rusty-settings`:**
   - В [config.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/config.rs) добавлены `NotificationPreviewsMode` (`Always`, `WhenUnlocked`, `Never`), поля `notifications_allow`, `notifications_show_previews`, `notifications_when_sleeping`, `notifications_when_locked`, `notifications_mirroring_off` в `UserConfig` и TOML-парсинг/сериализация (`apply_notifications_kv`, `append_notifications_toml`), применяемые в [frame_composer.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/frame_composer.rs).
   - В [apps/rusty-settings/src/{state.rs, layout.rs, render.rs}](file:///home/anc/RustyOS_Main/apps/rusty-settings/src/layout.rs) цвет иконки `SettingsPage::Notifications` обновлён до `#FF3B30`, а страница оформлена по эталону `27-Golden-Gate-Settings-Notifications.png`: заголовочная карточка `Notifications`, группа политик **`Notification Center`** (`Show previews`, `when display is sleeping`, `when screen is locked`, `when mirroring or sharing the display`), группа **`Application Notifications`** (`Files`, `Terminal`, `Browser`, `Software Update`, `System Settings` с векторными скворклами) и секция `Menu Bar & Updates`, синхронизируемые с `[notifications]` в `config.toml`.

## Consequences

- Клик по `Clock` и клик по иконке `Control Center` в `TopBar` работают независимо и взаимно закрывают открытые оверлеи, а `Notification Center` сохраняет уведомления, виджет `Weather` и интерактивный список задач без вмешательства в `Liquid Glass`, `Dock` и `Control Center`.
- Политики уведомлений (`notifications_allow`, `notifications_show_previews`, `notifications_when_locked` и др.) настраиваются в `rusty-settings` и немедленно применяются композитором.
