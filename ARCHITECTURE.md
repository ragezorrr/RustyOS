# RustyOS --- Architecture

Статус: **актуально для реализации до `RUSTYOS-128`** (развитие новых
границ и протоколов по-прежнему требует ADR и `APPROVE:` по `AGENTS.md`).

## 1. Архитектурная модель

RustyOS состоит из собственного Wayland-композитора (`rusty-compositor`)
на базе Smithay `0.7` (`calloop`, EGL/GLES, DRM/KMS/GBM/libinput/udev),
встроенного single-process desktop shell (`shell_render/*`, `ADR-0027`,
`ADR-0072`), модулей управления окнами/анимациями/сервисами и
самостоятельных системных Wayland CSD-приложений (`rusty-files`,
`rusty-settings`, `rusty-terminal`, `rusty-installer`), взаимодействующих с
композитором через Wayland и версионируемый Unix socket IPC (`rusty-ipc`).

## 2. Компоненты

### Compositor (`crates/rusty-compositor`)

- состояние композитора (`state/`):
  - `state/mod.rs` — core `RustyCompositorState`, `ClientState`, типы overlay/drag/clipboard и wiring;
  - `state/wm_windows.rs` — `WindowMap`, workspaces, focus, fullscreen/maximize, foreign toplevel;
  - `state/shell_actions.rs` — TopBar, Dock, Spotlight, Overview, Switcher, Quick Settings и notifications;
  - `state/system_bridges.rs` — Lock Screen, PAM/logind, Polkit, Portal/ScreenCast, PipeWire, config и IPC;
  - `state/wayland_handlers/` — сгруппированные Smithay/Wayland delegate-хендлеры (`core`, `seat_input`, `selection`, `session_lock`, `dmabuf_sync`, `desktop_protocols`, `liquid_glass`);
- Wayland display/server lifecycle и клиентский input routing (`input/`: `mod`, `keyboard`, `pointer`, `shell_pointer`, `gestures`);
- multi-output раскладка (`outputs.rs`) и интеграция с `rusty-wm`;
- nested Winit backend (`winit_backend.rs`) и native DRM/KMS per-CRTC
  render loop с mixed refresh, hotplug и VRR (`drm_backend.rs`);
- direct scanout (`zwp_linux_dmabuf_v1`), `wp_fractional_scale_v1` +
  `wp_viewporter`, `zwp_relative_pointer_manager_v1` +
  `zwp_pointer_constraints_v1`, `wp_presentation` (`Vsync`/`HwClock`/`ZeroCopy`
  и `Refresh::Variable`/`Fixed`), `wp_cursor_shape_v1`, `xdg_activation_v1`,
  `ext_idle_notify_v1` + `zwp_idle_inhibit_manager_v1`, `ext_session_lock_v1`,
  собственный протокол `rustyos_liquid_glass_v1` (`protocols/rustyos-liquid-glass-v1.xml`);
- `XWayland` интеграция и синхронизация clipboard (`xwayland.rs`);
- рендер-инструментация (`frame_timing.rs`), конфигурация с атомарным
  сохранением и live reload (`~/.config/rustyos/config.toml` в `config.rs`),
  скриншоты, лончер, OSD/уведомления и `StatusNotifierItem` трей.

Event loop не должен блокироваться тяжёлыми операциями и внешним D-Bus IO.

### Window Manager (`crates/rusty-wm`)

Чистая детерминированная модель (без системного IO):
- outputs и per-output workspaces (`1..=9`);
- `Floating`, `Snap` (`LeftHalf`, `RightHalf`, `Center`) и `MasterStack`
  tiling;
- focus/stacking, maximize/fullscreen (unbordered для direct scanout) и
  действия `WmAction`.

Выделение в отдельный процесс требует отдельного ADR и approval.

### Renderer (`winit_backend.rs`, `drm_backend.rs`, `shell_render/`, `surfaces/`)

- EGL + OpenGL ES (`GlesRenderer`) + GBM/DMA-BUF без Vulkan/wgpu;
- tracking damage (`OutputDamageTracker`), Direct Scanout при полноэкранном
  DMA-BUF клиенте;
- единый Liquid Glass пайплайн (`surfaces/liquid_glass.rs`: damage-gated Dual-Kawase FBO-пирамида
  `1 -> half_a -> quarter_a -> eighth_a -> quarter_b -> half_out`, непрерывный кубический суперэллиптический
  шейдер `LIQUID_GLASS_FRAG` с 14 px преломляющей линзой, 3-слойным направленным кантом `uRim` и
  профилями `GlassPanelStyle::{Dock, Popover, ControlModule}` + `invalidate_all()` при смене темы/обоев);
- композиторная обрезка неполноэкранных клиентских окон (`surfaces/window_clip.rs`: `ClippedSurfaceElement`,
  `SURFACE_CLIP_FRAG`, единый кубический суперэллипс `WINDOW_RADIUS = 14`, прямая сохранность Direct Scanout
  для полноэкранных окон);
- единый 3-дольный GPU-шейдер теней (`surfaces/window_shadow.rs`: wide + narrow + `1.5 px` contact) и
  жизненные scale+fade анимации открытия/закрытия окон (`surfaces/window_minimize.rs`: `180 ms` open /
  `160 ms` close по сдвигу `0.93 <-> 1.0`, плюс Genie/Scale minimize/restore);
- 2D shell/SSD рендеринг и CPU-fallback стекла/теней через `UiShapeCache` (`surfaces/`: `mod`, `sdf`,
  `glyphs`, `xcursor`, `liquid_glass`, `window_clip`, `window_shadow`, `window_minimize`) и `TextCache` (`fontdue`).

### Animation Engine (`crates/rusty-animation`)

- единая монотонная временная шкала, независимая от `60/120/144/240` Hz;
- easing-кривые (`EaseOutQuart`, `EaseOutCubic`, spring/smooth переходы);
- прерывание/реверс на лету и режим `reduced_motion`.

### Shell и IPC (`crates/rusty-compositor/src/shell_render`, `crates/rusty-ipc`)

- `rusty-ipc`: версионируемый протокол (`ShellRequest`, `CompositorEvent`,
  `DesktopStateSnapshot`) поверх Unix domain socket с защитой от битых/oversized
  сообщений для внешних клиентов (`rusty-settings` и др.);
- встроенные композиторные оверлеи (`shell_render/`: `mod`, `text_pass`, `overlay_shapes`,
  `chrome_shapes`, `tokens`, `top_bar`, `top_bar_menus`, `dock`, `dock_stack`,
  `context_menu`, `quick_settings`, `notification_center`, `updates_popover`,
  `wifi_popover`, `spotlight_overview`, `desktop_canvas`, `lock_screen`,
  `modals/{magnifier,polkit,portal}`, `ssd`) для low-latency взаимодействия без
  дублирующего процесса `rusty-shell` или дубля `modals/settings.rs` (`ADR-0027`, `ADR-0072`).

### System Services (`crates/rusty-services`)

Переиспользует штатную инфраструктуру Linux без переписывания демонов:
- `systemd/logind` (`libseat`) — VT switch и управление DRM-устройствами;
- `NetworkManager` / `BlueZ` (`zbus` D-Bus) — сеть и Bluetooth;
- `PipeWire` / `wpctl` / `pactl` — громкость и mute;
- `UPower` / `sysfs` — питание, батарея и подсветка экрана;
- `XWayland` — X11-приложения и общий буфер обмена;
- целевой этап Фазы 7: XDG Desktop Portal / PipeWire ScreenCast, Polkit UI,
  `zwp_text_input_v3`;
- модульная структура `src/`: `lib.rs` (`ServiceRegistry`), `power`,
  `connectivity`, `media`, `display_session`, `auth`, `logind`, `polkit`,
  `portal`, `file_manager`, `image_decode`.

### Системные приложения (`apps/rusty-files`, `apps/rusty-settings`, `apps/rusty-terminal`, `apps/rusty-installer`) и общий UI-крейт (`crates/rusty-ui`)

- Самостоятельные Wayland CSD-клиенты на базе `smithay-client-toolkit`, `fontdue` и
  общего крейта `crates/rusty-ui` в едином эталонном стиле окон (`WINDOW_RADIUS = 14`,
  единые `14 px` traffic lights `#EE5A6A`/`#F4B63F`/`#3CC68A` в `(14/36/58, 13)`
  с центрами `21.0, 43.0, 65.0` и приглушением `inactive_color()` в фоне, обводка до угловой
  маски и сквозная синхронизация темы `Light` / `Dark` / `Auto`), запускаемые композитором с
  приоритетом `exe_dir` над `PATH` (`launcher.rs`):
- `rusty-ui`: единый общий крейт для оконной геометрии (`Rect`, `Point`, `Size`, `WINDOW_RADIUS = 14`,
  `TITLEBAR_HEIGHT = 40`, `SSD_TITLEBAR_HEIGHT = 28`), светофоров (`TrafficLightButtons`, `TrafficLightKind`,
  векторные микроглифы `x`, `-`, `+`), 7-цветовых палитр Light/Dark (`ThemePalette`, `ACCENT_PALETTE`,
  `accent_for`), меню (`menu`: `14/26/12/9 px`), шапок (`window_header`), 3-дольных теней Гаусса
  (`shadow`: `ShadowElevation`, `ShadowStyle`, `draw_elevation_shadow_with`), структуры окна (`WindowFrame`,
  `WindowHitTarget`, double-click maximize) и CPU-рендерера с Porter-Duff premul-смешиванием (`UiCanvas`);
- `rusty-files`: сайдбар с реальным Liquid Glass через `rustyos_liquid_glass_v1` (`GlassStyle::Sidebar`,
  `WlRegion` шириной сайдбара + радиус, плоский `#ECEDF1`/`#262628` fallback при `reduce_transparency`,
  активный пункт `r = 10` `#D9DADD`/`#3E3E41`), бесшовные капсулы тулбара/вкладок без рамок,
  Icons/List/Columns/Gallery, clickable Path Bar, Quick Look, Get Info, Copy/Move, Trash, теги и Liquid Glass поповеры;
- `rusty-settings`: отдельное окно настроек (`920×640` / `882×600`, `org.rustyos.Settings`) с
  Liquid Glass сайдбаром (`rustyos_liquid_glass_v1`, `WlRegion` по ширине сайдбара + плоский fallback при
  `reduce_transparency`), карточками `#FFFFFF`/`#2C2C2D` (`r = 12`) на фоне `#F5F6FA`/`#1C1C1D`,
  выбором темы (`Light`/`Dark`/`Auto`), встроенной страницей *Software Update* (`--page updates`) и
  живой отправкой `v1|RELOAD_CONFIG` в композитор;
- `rusty-terminal`: Wayland-EGL GLES2 + wl_shm терминал с векторными светофорами, вкладками,
  scrollback и плавным скроллингом;
- `rusty-installer`: мастер установки системы и первый OOBE Setup Assistant
  (`org.rustyos.Installer`, `--install`/`--oobe`/`--live`/`--demo`/`--dry-run`) с
  карточками дисков, безопасным Polkit-бэкендом `scripts/rustyos-install.sh`,
  потоковым прогрессом (`AnimatedValue`) и живой настройкой `Light`/`Dark`/`Auto` +
  `[ui].accent_index` (`RELOAD_CONFIG`).

## 3. Текущая структура Cargo workspace и сессии

```text
RustyOS/
├── Cargo.toml                 # workspace (edition = 2024, MSRV 1.99, unsafe_code = "deny")
├── rust-toolchain.toml
├── crates/
│   ├── rusty-compositor/      # Wayland compositor, Winit & DRM/KMS backends, frame_composer, input/, state/, shell_render/, surfaces/
│   │   └── src/
│   │       ├── frame_composer.rs  # Backend-независимая сборка render list кадра, Z-order overlay/chrome/layer-shell, HiDPI scale, readback
│   │       ├── state/         # RustyCompositorState, wm_windows, shell_actions, context_menu_actions, system_bridges, wayland_handlers/
│   │       ├── shell_render/  # mod (фасад), text_pass, overlay_shapes, chrome_shapes, tokens, top_bar, top_bar_menus, dock, dock_stack, context_menu, quick_settings, updates_popover, wifi_popover, notification_center, spotlight_overview, desktop_canvas, lock_screen, modals (polkit, portal, magnifier), ssd
│   │       ├── input/         # keyboard, pointer, shell_pointer, gestures
│   │       └── surfaces/      # UiShapeCache, sdf, glyphs, xcursor, liquid_glass, window_clip, window_shadow, window_minimize
│   ├── rusty-wm/              # Детерминированная модель WM и workspace 1..9
│   ├── rusty-animation/       # Refresh-rate independent animation engine
│   ├── rusty-ipc/             # Версионируемый IPC-протокол клиентов/приложений с композитором
│   ├── rusty-services/        # Адаптеры NetworkManager, BlueZ, PipeWire, UPower, sysfs, logind, Polkit, Portal, file_manager, image_decode, updates
│   │   └── src/               # lib (ServiceRegistry), power, connectivity, media, display_session, auth, logind, polkit, portal, file_manager (types, listing, trash, operations, metadata), image_decode, updates
│   └── rusty-ui/              # Общий UI-крейт: геометрия окон (WINDOW_RADIUS=14), светофоры, menu, shadow, window_header, палитры Light/Dark, WindowFrame, UiCanvas
├── apps/
│   ├── rusty-files/           # Самостоятельный Wayland-файловый менеджер (CSD + Liquid Glass sidebar, render: canvas, chrome, sidebar, views, dialogs, popup, mod; layout; state)
│   ├── rusty-installer/       # Wayland CSD-установщик и OOBE Setup Assistant (disks, layout, liquid_glass, render, state, wayland)
│   ├── rusty-settings/        # Самостоятельное Wayland CSD-окно System Settings (Liquid Glass sidebar, Software Update, scroll, TOML + `RELOAD_CONFIG`)
│   └── rusty-terminal/        # Wayland-EGL GLES2 + wl_shm терминал с Liquid Glass сайдбаром, вкладками и векторными светофорами
├── bin/
│   └── rustyos-session        # Лаунчер пользовательской сессии (`--doctor`, `.oobe-pending` autolaunch)
├── packaging/
│   ├── applications/          # org.rustyos.{files,installer,settings,terminal,updater}.desktop
│   ├── arch/PKGBUILD          # Arch Linux PKGBUILD (rustyos-git)
│   ├── archiso/               # Загрузочный профиль Live ISO (`greetd`, `liveuser`, `build-iso.sh`)
│   ├── polkit/                # org.rustyos.{installer,updater}.policy
│   └── wayland-sessions/      # rustyos.desktop
├── scripts/
│   ├── install.sh
│   └── rustyos-install.sh     # Инсталлятор разметки/pacstrap/boot/GREETD с --dry-run и STEP:-выводом
└── docs/
    ├── adr/                   # ADR-0001 .. ADR-0065
    ├── DEVELOPMENT_ENVIRONMENT.md
    ├── GETTING_STARTED.md
    └── HARDWARE_MATRIX.md
```

## 4. Фактический стек

  Область                Реализация                         Примечание
  ---------------------- ---------------------------------- --------------------------------
  Язык                   Rust stable (`1.99`, ed. 2024)     `unsafe_code = "deny"` в workspace
  Display protocol       Wayland + `XWayland`               Smithay `0.7.0`
  Nested backend         Smithay `WinitGraphicsBackend`     Для разработки и CI/smoke
  Native output          `DRM/KMS` + `GBM` + `libseat`      Per-CRTC loop, VRR, hotplug
  Graphics               `EGL` + `OpenGL ES` (`GlesRenderer`) DMA-BUF + Direct Scanout
  Input                  `libinput` / `winit` + `XKB`       Relative pointer + constraints
  Event loop             `calloop`                          Без блокировок presentation path
  Text / Geometry        `fontdue` + SDF/pixel caches       Независимо от host GUI toolkits
  Services               `zbus` (D-Bus), `wpctl`, `sysfs`   BlueZ, NM, notifications, SNI
  Config                 TOML (`~/.config/rustyos/config.toml`) Display scales, VRR, XKB, UI, `[dock]`, live save/reload
  Session                `systemd/logind` + `rustyos-session` В разработке Фаза 10 (packaging)

## 5. Потоки и ошибки

- Event loop выполняет только короткие неблокирующие операции.
- Фоновый сбор телеметрии, D-Bus уведомления, трей и PNG-скриншоты вынесены
  из render/presentation path.
- Отсутствующие аппаратные capabilities (VRR, DMA-BUF modifiers, direct
  scanout) переводят вывод на безопасный композитный fallback с диагностикой.

## 6. Архитектурные решения

Существенные архитектурные изменения фиксируются в `docs/adr/NNNN-title.md`
и `DECISIONS.md` до реализации только после явного `APPROVE:` пользователя.

## 7. Точки расширения и внутренние контракты (Fast Code Map)

Краткая карта типовых врезок для экономии контекста и поиска:

- **Новый shell-оверлей / модал:**
  1. Рендер и hit-test: `shell_render/modals/` -> реэкспорт, `ShellOverlayView` и маски скрытия в `shell_render/mod.rs` + диспетчеризация текста/геометрии в `text_pass.rs` / `overlay_shapes.rs`.
  2. Композиция (Z-order `foreground_overlay`): `frame_composer.rs`.
  3. Состояние и fullscreen-bypass: `state/mod.rs` (`has_blocking_shell_overlay`, `should_hide_shell_chrome_for_fullscreen_on`, `active_unoccluded_fullscreen_window_on`) + взаимное закрытие в `state/shell_actions.rs` и `state/system_bridges.rs`.
  4. Pointer-перехват: `input/pointer.rs` (`space.output_geometry(&self.output)...to_physical(1)`) + `input/shell_pointer.rs`.
- **Новый хоткей / пункт меню / IPC-команда:**
  1. IPC: `crates/rusty-ipc/src/lib.rs` (`ShellRequest`, `encode_line`, `decode_line`) -> обработка в `state/wm_windows.rs`.
  2. Меню: `shell_render/top_bar/` (`TopBarMenuAction`, `MENU_*`, `top_bar_menu_items`) -> `state/shell_actions.rs` (`dispatch_top_bar_menu_action`).
  3. Клавиатура: `input/mod.rs` (`KeyIntercept`) -> `input/keyboard.rs` (`dispatch_key_intercept`, `classify_keyboard_input`, `match_shortcut`).
- **Настройки `UserConfig` и live save/reload:**
  - `config.rs` (`UserConfig`, `parse_toml`, `to_toml_string`, `save_to_path`); `state/shell_actions.rs` (`sync_and_save_user_config`, `apply_user_config`, `reload_user_config`); `config_path` = `None` в тестах и `default_user_config_path()` в `winit_backend.rs`/`drm_backend.rs`.
- **Контекстные меню и выпадающие списки (Desktop / Dock / Window Management / Clients):**
  - Системные меню: `shell_render/context_menu.rs` (`ContextMenuState`, `ContextMenuEntry`, `ContextMenuAction`, `compute_context_menu_layout`, `generate_context_menu_elements`) + `ShellOverlayView::ContextMenu` в `shell_render/mod.rs`.
  - Модульная подсистема: `crates/rusty-compositor/src/context_menu/` (`window_menu.rs`, `desktop_menu.rs`, `dock_menu.rs`, `actions.rs`, `mod.rs`) с фасадом в `state/context_menu_actions.rs`.
  - Оконное управление: перехват правого клика на SSD-заголовках/кнопках (`input/pointer.rs`), глобальные хоткеи `Alt+Right` / `Super+Right` и Wayland CSD callback `show_window_menu` (`state/wayland_handlers/core.rs`).
  - Стандарт контекстных и выпадающих меню (`top_bar/menus.rs`, `shell_render/context_menu.rs`, `crates/rusty-compositor/src/frame_composer.rs`, `crates/rusty-ui/src/menu.rs`, `apps/rusty-files/src/{layout.rs, render/popup.rs, wayland.rs}`): концентрический `MENU_RADIUS = 14`, `MENU_POPOVER_PAD_Y = 5`, `MENU_ITEM_HEIGHT = 26`, `MENU_SEPARATOR_HEIGHT = 12`, инсетные пилюли ховера `HOVER_PILL_RADIUS = 9`, `HOVER_PILL_INSET_H = 5`, `HOVER_PILL_INSET_V = 1`, `TopBar` плашка открытого меню `8 px`, профиль `TopBarAppProfile::Desktop` по умолчанию, двухрежимное левое выравнивание, 3-дольная SDF/GPU тень `ShadowElevation::MenuOrTooltip`, направленный внутренний `1 px` hairline (верх `100%`, низ `35%`), колонка шорткатов `12 px`. Клиентские `xdg_popup` (`sync_popup_surface`) передают точный `wl_region` (`spec.layout.rect`) и прозрачный SHM-буфер при активном композиторном стекле (`render_popup_surface_pixels_with_glass`) (`ADR-0043`, `ADR-0058`, `ADR-0069`–`0071`).
  - Унифицированный GPU-шейдер Liquid Glass и CPU-fallback (`crates/rusty-compositor/src/surfaces/liquid_glass.rs`, `sdf.rs`): `LIQUID_GLASS_FRAG` с непрерывным кубическим суперэллипсом (`r * 1.42` для скруглённых углов и L2-круг для капсул), 14 px линзой (`ior = 1.7`) и 3-слойным направленным `uRim` для `GlassPanelStyle::{Dock, Popover, ControlModule}` плюс `GlassPanelStyle::NotificationCard` (`ADR-0052`, `ADR-0053`, `ADR-0061`, `ADR-0072`).
  - First-party Wayland-протокол Liquid Glass `rustyos_liquid_glass_v1`: серверный менеджер `crates/rusty-compositor/src/state/wayland_handlers/liquid_glass.rs`, композиторная отрисовка в `frame_composer.rs` (`push_window_liquid_glass_elements` для основных окон/сайдбаров и `push_client_popup_elements` / `push_popup_glass_panels` для `xdg_popup`), клиентская интеграция сайдбаров в `apps/rusty-files`, `apps/rusty-settings` и `apps/rusty-terminal` (`liquid_glass.rs`, `WlRegion` по ширине сайдбара + радиус) (`ADR-0048`, `ADR-0058`, `ADR-0072`).
  - Композиторная обрезка клиентских углов и lifecycle-анимации (`RUSTYOS-128`, `ADR-0072`): `surfaces/window_clip.rs` (`ClippedSurfaceElement`, `SURFACE_CLIP_FRAG`) + `surfaces/window_minimize.rs` (`180 ms` open / `160 ms` close `0.93 <-> 1.0`).
  - Сеть, проводное подключение, `WifiPopover` и страницы `Wi-Fi` / `Network` (`RUSTYOS-108`, `ADR-0059`, `D-072`):
    - `crates/rusty-services/src/connectivity.rs`: разделение Ethernet и Wi-Fi в `NetworkStatus` (`wired_connected`, `wired_interface`, `wired_label`, `wifi_connected`, `wifi_networks: Vec<WifiAccessPoint>`), сбор списка сетей `NetworkService::list_wifi_networks` / `parse_nmcli_wifi_list` и подключение `NetworkService::connect_wifi`;
    - `crates/rusty-compositor`: глиф `UiGlyphKind::Ethernet` (`surfaces/glyphs.rs`), отображение `Ethernet` в `TopBar` при `wired_connected && !wifi_connected`, клик `TopBarHit::Wifi` / правый клик `QuickSettingsHit::ToggleWifi`, оверлей `ShellOverlayView::Wifi` (`shell_render/wifi_popover.rs`, `WifiPopoverRenderState`, `WifiPopoverHit`, `KeyIntercept::WifiPopoverClose`, `MENU_POPOVER_RADIUS = 13`, `HOVER_PILL_RADIUS = 7`), изолированная подпись Wi-Fi в `QuickSettingsRenderState` (`wifi_connected`, `wifi_ssid`) без изменения геометрии `Control Center` / `Dock`;
    - `apps/rusty-settings/src/{state.rs, layout.rs, render.rs}`: раздельные страницы `SettingsPage::Wifi` (`RowControlId::{WifiEnabledToggle, WifiConnectIndex, NetworkRefresh}`) и `SettingsPage::Network` (карточки сервисов `Wi-Fi`, `Ethernet`, `Firewall` + `Other Services`).
  - `Notification Center`, триггер `Clock` и настройки уведомлений (`RUSTYOS-109`, `ADR-0060`, `D-073`):
    - `crates/rusty-compositor/src/config.rs` + `frame_composer.rs`: `NotificationPreviewsMode` (`Always`, `WhenUnlocked`, `Never`), секция `[notifications]` (`notifications_allow`, `notifications_show_previews`, `notifications_when_sleeping`, `notifications_when_locked`, `notifications_mirroring_off`), `apply_notifications_kv` и `append_notifications_toml`;
    - `crates/rusty-compositor/src/shell_render/{notification_center.rs, top_bar/panel.rs, overlay_shapes.rs, text_pass.rs}` + `state/*` + `input/*`: разграничение клика `Control Center` и `check_top_bar_clock_click_for_options`, флаг `TopBarTelemetry::notification_center_open`, глифы `UiGlyphKind::{CloudSun, Checklist}` (`surfaces/glyphs.rs`), долговременная история `notification_history`, панель `ShellOverlayView::NotificationCenter` (`NotificationCenterRenderState`, `NotificationCenterHit`, `compute_notification_center_layout`, `hit_test_notification_center`, `render_notification_center_text`, `render_notification_center_composite`, виджеты `Weather` и интерактивный `Reminders`, `Edit Widgets`, `KeyIntercept::NotificationCenterClose`) без правок `liquid_glass.rs`, `sdf.rs`, `dock.rs` и `quick_settings.rs`;
    - `apps/rusty-settings/src/{state.rs, layout.rs, render.rs}`: страница `SettingsPage::Notifications` (`#FF3B30`, `NotificationHeader`, `Notification Center`, `Application Notifications`, `Menu Bar & Updates`, `RowControlId::{NotifAllowToggle, NotifShowPreviewsCycle, NotifWhenSleepingToggle, NotifWhenLockedToggle, NotifMirroringCycle, NotifAppToggle}`).
  - Антиалиасинг иконoк и UI-глифов (`RUSTYOS-111`, `ADR-0062`, `D-075`):
    - `crates/rusty-compositor/src/aa.rs` (`R2_TAPS_16`, `LinearAccum`): 16-точечный субпиксельный сэмплинг в линейном свете с `GLYPH_BUFFER_SCALE = 2` (`2x` `HiDPI` буферы в `icons.rs`, `surfaces/glyphs.rs`, а также `R2_TAPS_16` в `surfaces/sdf.rs`);
    - `crates/rusty-services/src/image_decode.rs` (`downscale_lanczos_linear`): сепарабельный `Lanczos-3` downsampling в линейном свете для внешних `.png` (`load_png_as_bgra_scaled`) и `SNI` (`argb32_be_to_premul_bgra`);
    - `apps/rusty-settings/src/render.rs`, `crates/rusty-ui/src/canvas.rs`: 4-tap `RGSS` (`aa_cov8`) и субпиксельный AA для скруглённых прямоугольников, скворклов, кругов, сегментов и эмблем.
  - Resize-минимум, минимизация и плавность `rusty-settings` (`RUSTYOS-112`, `ADR-0063`, `D-076`):
    - `crates/rusty-wm/src/lib.rs` + `crates/rusty-compositor/src/{state/wm_windows.rs, input/pointer.rs}`: `WindowManager::resize_window_with_min`, `effective_window_min_size`, `compute_resized_rect_with_min`;
    - `crates/rusty-compositor/src/{frame_composer.rs, surfaces/liquid_glass.rs, surfaces/window_minimize.rs}`: `push_window_elements_with_options`, `should_skip_uncommitted_bound_window`, `make_glass_element_with_backdrop_loc`, premul `WINDOW_MINIMIZE_FRAG` с `u_bounds_px` и 4-tap `sample_filtered`;
    - `apps/rusty-settings/src/{state.rs, wayland.rs, render.rs}`: фоновый worker `execute_coalesced_service_batch` + `calloop::ping`, субпиксельный/плавный скролл `scroll_smooth_by` / `step_scroll_animation`, `DamageScope`, сканлайны `rounded_interior_span` и `SPRITE_CACHE`.
  - GPU-терминал `rusty-terminal` (`RUSTYOS-113`, `ADR-0064`, `D-077`):
    - `apps/rusty-terminal/src/{gpu_egl.rs, renderer_shm.rs, font.rs, theme.rs, liquid_glass.rs}`: `GpuRenderer` (`Wayland-EGL` + `GLES 2.0` 2-pass `RECT_PROG`/`TEXT_PROG` + `1024×1024` `ShelfPacker` атлас), CPU fallback `ShmFallbackRenderer` (`--no-gpu`), `FontSet` с процедурными `Box Drawing` / `Block Elements` / `Powerline` / UI-иконками, `TerminalTheme` (`Clear Light` / `Clear Dark` / `Opaque`, `ConfigWatcher`) и `rustyos_liquid_glass_v1` (`GlassStyle::Sidebar`, `r = 14`);
    - `apps/rusty-terminal/src/{vt.rs, pty.rs, state.rs, layout.rs, wayland.rs}`: `TerminalEngine` (UTF-8, `16`/`256`/TrueColor, `DECSTBM`, alt-screen, SGR mouse, `10 000`-строчный scrollback), `PtySession` (`rustix::pty` + `poll` + `calloop::ping`), вкладки, плавный скролл (`AnimatedValue`), поиск, clipboard (`DataDeviceManagerState`);
    - `crates/rusty-compositor/src/{launcher.rs, shell_render/dock.rs, state/shell_actions.rs, context_menu/actions.rs}`: приоритет `rusty-terminal` для `terminal` Dock/Launcher/Desktop (`launch_terminal_in_dir`) с fallback на внешние терминалы.
  - Инсталлятор `rusty-installer`, OOBE Setup Assistant, Live ISO и Dual-Boot (`RUSTYOS-115`, `RUSTYOS-116`, `ADR-0065`, `ADR-0066`, `D-079`, `D-080`):
    - `apps/rusty-installer/src/{main.rs, disks.rs, state.rs, layout.rs, render.rs, liquid_glass.rs, wayland.rs}`: двухрежимный CSD-клиент (`--install` / `--oobe`, `--live`, `--demo`, `--dry-run`) с режимами `Erase Entire Disk` и `Dual-Boot` (выбор уже созданного root-раздела или неразмеченного пространства по `lsblk -Jpb` без `sgdisk --zap-all`), защитой смонтированных разделов, переиспользованием EFI `>= 260 MiB` без форматирования либо выделением отдельного `512 MiB` EFI при маленьком ESP, живой перекраской `RustyOS Light`/`Dark`/`Auto` + `[ui].accent_index` (`RELOAD_CONFIG`) и потоковым прогрессом `STEP:<pct>:<stage>:<msg>`;
    - `scripts/rustyos-install.sh` (`--install-mode`, `--root-part`, `--free-space`, `--efi-part`, `--confirm-dualboot`, `os-prober`, сохранение чужого `loader.conf`) + `packaging/polkit/org.rustyos.installer.policy` + `bin/rustyos-session` (`--doctor`, `.oobe-pending`) + `packaging/arch/PKGBUILD` + `packaging/archiso/` (`profiledef.sh`, `greetd`, `packages.x86_64`, `build-iso.sh`).
- **Эталонный стиль системных CSD-окон (`crates/rusty-ui`, `ADR-0055`, `ADR-0057`, `ADR-0067`, `ADR-0070`, `ADR-0072`):**
  - Единые константы окон и теней (`crates/rusty-ui/src/{geometry,shadow,menu}.rs`, `crates/rusty-compositor/src/shell_render/tokens.rs`): `WINDOW_RADIUS = 14` (`WINDOW_CORNER_RADIUS = 14`), `1 px` внешний контур (`[0, 0, 0, 24]` Light / `[255, 255, 255, 22]` Dark) строго **до** угловой маски (`apply_window_corner_mask` / `clip_rounded_corners`).
  - Единые `14 px` traffic lights (`crates/rusty-ui/src/traffic_lights.rs`: `TrafficLightButtons`, `TRAFFIC_LIGHT_DIAMETER = 14`, `Rect(14/36/58, 13, 14, 14)`, центры `(21/43/65, 20)`, `r = 7.0`, `#EE5A6A` / `#F4B63F` / `#3CC68A`, без квадратной рамки ореола, с векторными hover-глифами `x`, `-`, `+`).
  - Сайдбар `#ECEDF1`/`#262628`, активный пункт `r = 10` (`#D9DADD`/`#3E3E41`), холст `#F5F6FA`/`#1C1C1D`, карточки (`CARD_CORNER_RADIUS = 12`, `#FFFFFF`/`#2C2C2D`) и капсулы тулбара/фильтров без серых рамок.
- **Нетривиальные сигнатуры и lint-правила:**
  - `KeyboardHandle::change_repeat_info(&self, rate: i32, delay: i32)` (порядок `(rate, delay)`, обратный к `seat.add_keyboard(xkb, delay, rate)`).
  - `set_output_rotation(id, rot, use_winit_fbo_flip)`: `use_winit_fbo_flip = name.eq_ignore_ascii_case("winit-0")` (`true` для Winit FBO, `false` для DRM/KMS).
  - `TextCache::render_label_weighted(renderer, text, FontWeight, size_px, rgba, loc)` и `render_label_centered_weighted(renderer, text, FontWeight, size_px, rgba, logical_rect)`.
  - Использовать `.map_or_default(...)` вместо `.map(...).unwrap_or_default()` (Clippy `1.100+` в CI) и не допускать `expect_used` / `items_after_statements` / `too_many_lines` (`>100`) даже в `#[cfg(test)]`.
