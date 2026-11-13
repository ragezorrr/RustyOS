# ADR-0022: Floating Liquid Glass Control Center Islands & Live MPRIS2 / Display Integration (`RUSTYOS-053`)

- **Статус:** accepted
- **Дата:** 2026-11-13
- **Автор:** AI Agent + User

## Контекст

Предыдущий Control Center в `crates/rusty-compositor/src/shell_render.rs` рендерился внутри единой тёмной внешней подложки `GlassPanelStyle::Popover`, объединявшей Wi-Fi/Bluetooth в одном прямоугольном блоке и содержавшей текстовые подписи под круглыми кнопками, толстые ползунки с процентными подписями справа, отдельную нижнюю карточку Power и три круглых кнопки действий (`Search`, `Sync`, `Lock`). При сопоставлении с референсом macOS 27 требуется 1:1 макет без внешней оболочки:

1. Каждый элемент должен парить на рабочем столе как самостоятельный островок Liquid Glass (`GlassPanelStyle::ControlModule`) с собственной тенью и мягким верхне-нижним спекулярным бликом.
2. Левая колонка должна состоять из отдельных капсул (`Wi-Fi`, `Bluetooth`, `Power`) с инвертированным сплошным белым кругом и цветным глифом в активном состоянии.
3. Правый верхний модуль `Now Playing` должен работать с живым плеером (`MPRIS2` через Session D-Bus + fallback на `playerctl`), а оставшиеся островки включают парные круглые кнопки без подписей (`Tile/Float`, `Overview`, `Motion`, `Clear Glass`), капсулу `Focus` (режим «Не беспокоить», подавляющий toast-уведомления), тонкие слайдеры `Display` (поддержка `BrightnessService` с fallback на `glass_opacity` `35..=100%`) и `Sound` (с круглой кнопкой `Mute` справа), и нижнюю пилюлю `Lock Screen`.

## Рассмотренные варианты

- **Вариант 1 (Выбран):**
  - В `crates/rusty-services/src/lib.rs` расширить `NetworkStatus` (`wifi_enabled: bool`), добавить `NetworkService::set_wifi_enabled` (NetworkManager D-Bus `WirelessEnabled` + fallback `nmcli radio wifi`) и `MediaStatus` + `MediaService::query_status` (MPRIS2 Session D-Bus `org.freedesktop.DBus.Properties.GetAll` + fallback `playerctl`).
  - В `crates/rusty-compositor/src/telemetry.rs` добавить опрос `media`, команду `TelemetryCmd::SetWifiEnabled(bool)`, `SystemMetricsHandle::set_brightness_percent` и `toggle_wifi`.
  - В `crates/rusty-compositor/src/surfaces.rs` добавить SDF-глифы `MediaPrev`, `MediaPlay`, `MediaPause`, `MediaNext`, `MusicNote`, `Moon`, `OverviewSpaces` (4×4 SSAA) и настроить светящуюся полупрозрачную заливку + спекулярную окантовку `GlassPanelStyle::ControlModule`.
  - В `crates/rusty-compositor/src/state.rs`, `input.rs`, `shell_render.rs` и `winit_backend.rs` добавить `focus_mode`, удалить внешнюю обёртку `Popover`, развернуть `quick_settings_rect` (`316×456`) с отдельными островками Liquid Glass, полужирными заголовками `Inter SemiBold`, живым `Now Playing`, `Display` (яркость/стекло), `Sound` + `Mute`, `Focus` и `Lock Screen`.
- **Вариант 2:** Имитировать парящие островки поверх статичной внешней прозрачной рамки без `MPRIS2`, `BrightnessService` и `Focus`-подавления уведомлений. Не даёт живой функциональности и оставляет артефакты общей коробки.
- **Вариант 3:** Вынести Control Center в отдельный процесс `rusty-shell` на внешнем GUI-тулките. Потребует смены архитектуры и добавления крупных зависимостей при наличии прямого рендера и input routing в `rusty-compositor`.

## Решение

Утверждён **Вариант 1** (`APPROVE: RUSTYOS-053 Вариант 1` от 2026-11-13).

## Последствия

- Control Center визуально и композиционно соответствует эталону macOS 27 (независимые парящие островки Liquid Glass без внешней рамки, инвертированные белые круги активных капсул, `Now Playing` с управлением воспроизведением, круглые кнопки без подписей, `Focus`, тонкие слайдеры `Display`/`Sound` и пилюля `Lock Screen`).
- Новые публичные поля/методы (`NetworkStatus::wifi_enabled`, `MediaStatus`, `MediaService::query_status`, `UiGlyphKind` медиа/Focus/Overview глифы, `QuickSettingsHit`, `QuickSettingsSliderDrag::Display`, `State::focus_mode`) добавлены без новых внешних Rust-зависимостей и без `unsafe`.
