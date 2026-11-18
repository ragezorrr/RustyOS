# ADR-0059: Разделение статусов `Ethernet` / `Wi-Fi`, всплывающее окно `Wi-Fi Popover` и редизайн страниц `Wi-Fi` и `Network` в `rusty-settings` (`RUSTYOS-108`)

## Status

accepted

## Context

При одновременном или раздельном использовании проводного подключения (`Ethernet`) и беспроводной сети (`Wi-Fi`) прежняя реализация сетевой телеметрии и UI смешивала оба интерфейса:

1. `NetworkStatus` в [connectivity.rs](file:///home/anc/RustyOS_Main/crates/rusty-services/src/connectivity.rs) хранил только общий `status_label` (`"Проводное соединение 1"`, `"Wi-Fi: ..."`) и `connected`, из-за чего в `Control Center` ([quick_settings.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/quick_settings.rs)) под заголовком **`Wi-Fi`** отображалась подпись `"Проводное ..."` с активным значком Wi-Fi, даже когда работал только кабель Ethernet (или когда оба интерфейса работали параллельно, имя `SSID` перекрывалось проводным профилем).
2. В `TopBar` ([top_bar/status.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/top_bar/status.rs)) всегда рисовался только глиф `UiGlyphKind::Wifi` без отдельного индикатора проводного соединения `Ethernet`, а клик по иконке сети открывал общий `Control Center` вместо специализированного всплывающего окна выбора сетей `Wi-Fi`.
3. В приложении настроек [apps/rusty-settings/src/layout.rs](file:///home/anc/RustyOS_Main/apps/rusty-settings/src/layout.rs) страницы `Wi-Fi` и `Network` были объединены в общую статическую карточку из двух текстовых строк без переключателя Wi-Fi, списка `Known Networks` / `Other Networks` и отдельных карточек сервисов `Wi-Fi`, `Ethernet` и `Firewall`.

## Decision

1. **Разделение проводного (`Ethernet`) и беспроводного (`Wi-Fi`) состояния и список точек доступа в `crates/rusty-services`:**
   - В [connectivity.rs](file:///home/anc/RustyOS_Main/crates/rusty-services/src/connectivity.rs) и [lib.rs](file:///home/anc/RustyOS_Main/crates/rusty-services/src/lib.rs) добавлена структура `WifiAccessPoint` (`ssid`, `signal_percent`, `secure`, `connected`, `known`), а `NetworkStatus` расширен полями `wired_connected`, `wired_interface`, `wired_label`, `wifi_connected` и `wifi_networks`.
   - `query_network_via_dbus()` и `NetworkService::query_status()` раздельно определяют активный проводной интерфейс (включая одновременную работу `eno1` + `wlan0`) и наполняют отсортированный список точек доступа через `parse_nmcli_wifi_list` (`nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY dev wifi list --rescan no` и сохранённые профили `802-11-wireless`); добавлены `NetworkService::list_wifi_networks` и `NetworkService::connect_wifi`.

2. **Иконка `Ethernet` в `TopBar`, изоляция подписи `Wi-Fi` в `Control Center` и `WifiPopover` (`r = 13`, `r_pill = 7`):**
   - Добавлен SDF-глиф `UiGlyphKind::Ethernet` ([surfaces/mod.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/mod.rs), [glyphs.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/glyphs.rs)).
   - В [top_bar/panel.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/top_bar/panel.rs) и [top_bar/status.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/top_bar/status.rs) добавлены поля `wired_connected`, `wifi_connected`, `wifi_enabled`, `wifi_open` и `check_top_bar_wifi_click_for_options`: при активном проводном соединении без активного Wi-Fi отображается `UiGlyphKind::Ethernet`, при активном/включённом Wi-Fi — `UiGlyphKind::Wifi` с дугами сигнала, а клик по иконке в `TopBar` открывает `WifiPopover`.
   - В [quick_settings.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/quick_settings.rs) добавлены `wifi_connected`, `wifi_ssid`, `is_wifi_status_label` и `concise_wifi_subtitle_for_state` без каких-либо изменений координат, размеров или радиусов капсул `Control Center` (`RUSTYOS-106`): подпись `Wi-Fi` показывает `SSID`, `"On"` или `"Off"` и никогда не отображает `"Проводное ..."`.
   - Реализован модуль [wifi_popover.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/wifi_popover.rs) (`ShellOverlayView::Wifi`) по стандарту меню `macOS 27 Golden Gate` (`MENU_POPOVER_RADIUS = 13`, ховер-пилюли `HOVER_PILL_RADIUS = 7` с инсетом `5 px`, `GlassPanelStyle::Popover`, тень `(16, 52)`): заголовок **`Wi-Fi`** с тумблером включения, строка статуса `Ethernet (<iface>) · Connected` при активном кабеле, секции **`Known Networks`** (с синим/серым круглым бейджем и замком) и **`Other Networks`**, клик по сети для подключения (`SystemMetricsHandle::connect_wifi` без блокировки event loop), а также пункт **`Wi-Fi Settings...`** для запуска `rusty-settings --page wifi`.
   - Шейдер и геометрия `Dock` ([liquid_glass.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/liquid_glass.rs), [sdf.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/sdf.rs), [dock.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/dock.rs)) не изменялись.

3. **Редизайн вкладок `Wi-Fi` и `Network` в `apps/rusty-settings`:**
   - В [apps/rusty-settings/src/layout.rs](file:///home/anc/RustyOS_Main/apps/rusty-settings/src/layout.rs), [render.rs](file:///home/anc/RustyOS_Main/apps/rusty-settings/src/render.rs) и [state.rs](file:///home/anc/RustyOS_Main/apps/rusty-settings/src/state.rs) вкладка **`Wi-Fi`** оформлена по эталону `27-Golden-Gate-Settings-Wi-Fi.png` (`WifiHeaderSwitch` с синей иконкой и тумблером, текущая подключённая сеть `● Connected · <signal>%`, секции `Known Networks` и `Other Networks` с галочкой `✓`, замком, дугами сигнала и кнопкой `Refresh`), а вкладка **`Network`** — по эталону `27-Golden-Gate-Settings-Network.png` (отдельные карточки сервисов **`Wi-Fi`**, **`Ethernet`**, **`Firewall`** с цветными индикаторами статуса `●` и переходом по шеврону `>` в **`Wi-Fi`**, а также секция **`Other Services`**).

## Consequences

- `TopBar`, `Control Center`, всплывающее окно `WifiPopover` и страницы `Wi-Fi` / `Network` в `rusty-settings` строго разграничивают `Ethernet` и `Wi-Fi`, корректно отображая как чисто проводной режим, так и одновременную работу `Ethernet` + `Wi-Fi`.
- Пользователь может включать/выключать `Wi-Fi` и подключаться к точкам доступа напрямую из `TopBar` поповера или из окна `rusty-settings` без блокировки композитора.
