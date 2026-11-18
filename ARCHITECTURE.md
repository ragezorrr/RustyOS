# RustyOS --- Architecture

Статус: **актуально для реализации до `RUSTYOS-088`** (развитие новых
границ и протоколов по-прежнему требует ADR и `APPROVE:` по `AGENTS.md`).

## 1. Архитектурная модель

RustyOS состоит из собственного Wayland-композитора (`rusty-compositor`)
на базе Smithay `0.7` (`calloop`, EGL/GLES, DRM/KMS/GBM/libinput/udev),
модулей управления окнами/анимациями/сервисами и внешнего процесса
оболочки (`rusty-shell`), взаимодействующего через версионируемый Unix
socket IPC (`rusty-ipc`) и `zwlr_layer_shell_v1`.

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
  `1 -> half_a -> quarter_a -> eighth_a -> quarter_b -> half_out`, кубический суперэллиптический
  шейдер `LIQUID_GLASS_FRAG` с 14 px преломляющей линзой, 3-слойным направленным кантом `uRim` и
  профилями `GlassPanelStyle::{Dock, Popover, ControlModule}` + `invalidate_all()` при смене темы/обоев);
- 2D shell/SSD рендеринг и CPU-fallback стекла через `UiShapeCache` (`surfaces/`: `mod`, `sdf`,
  `glyphs`, `xcursor`, `window_shadow`, `window_minimize`) и `TextCache` (`fontdue`).

### Animation Engine (`crates/rusty-animation`)

- единая монотонная временная шкала, независимая от `60/120/144/240` Hz;
- easing-кривые (`EaseOutQuart`, spring/smooth переходы);
- прерывание/реверс на лету и режим `reduced_motion`.

### Shell и IPC (`apps/rusty-shell`, `crates/rusty-ipc`)

- `rusty-ipc`: версионируемый протокол (`ShellRequest`, `CompositorEvent`,
  `DesktopStateSnapshot`) поверх Unix domain socket с защитой от битых/oversized
  сообщений;
- `apps/rusty-shell`: опциональный внешний процесс shell-клиента
  (`--wayland`, opt-in через `RUSTYOS_EXTERNAL_SHELL=1` в `rustyos-session`),
  плюс канонические встроенные композиторные оверлеи
  (`shell_render/`: `mod`, `text_pass`, `overlay_shapes`, `chrome_shapes`, `tokens`,
  `top_bar`, `dock`, `context_menu`, `quick_settings`, `spotlight_overview`,
  `lock_screen`, `modals`, `ssd`) для low-latency
  взаимодействия (TopBar, Dock, Context Menus, Spotlight Launcher, Overview, Quick Settings,
  Lock Screen, Notifications, Alt+Tab HUD, Polkit/Portal/PickColor/Settings modals).

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

### Системные приложения (`apps/rusty-files`, `apps/rusty-settings`, `apps/rusty-updater`)

- Самостоятельные Wayland CSD-клиенты на базе `smithay-client-toolkit` и
  `fontdue` в едином эталонном стиле окон (`14 px` скругления и тень
  `WINDOW_RADIUS = 14`, единые `14 px` traffic lights `#EE5A6A`/`#F4B63F`/`#3CC68A`
  в `(14/36/58, 13)`, обводка до угловой маски и сквозная синхронизация темы
  `Light` / `Dark` / `Auto`), запускаемые композитором с приоритетом `exe_dir`
  над `PATH` (`launcher.rs`):
- `rusty-files`: плоский edge-to-edge sidebar (`#ECEDF1`/`#262628`, активный пункт `r = 10` `#D9DADD`/`#3E3E41`),
  бесшовные капсулы тулбара/вкладок без рамок, Icons/List/Columns/Gallery,
  clickable Path Bar, Quick Look, Get Info, Copy/Move, Trash, теги и Liquid Glass поповеры;
- `rusty-settings`: отдельное окно настроек (`882×600`, `org.rustyos.Settings`) с
  плоским непрозрачным сайдбаром (`#ECEDF1`/`#262628`), карточками `#FFFFFF`/`#2C2C2D` (`r = 12`)
  на фоне `#F5F6FA`/`#1C1C1D` без рамок, скроллингом, выбором темы (`Light`/`Dark`/`Auto`)
  и живой отправкой `v1|RELOAD_CONFIG` в композитор;
- `rusty-updater`: окно *Software Update* (`740×560`, шапка `42 px`) на фоне `#F5F6FA`/`#1C1C1D`
  с карточками (`r = 12`) и пилюлями (`r = 10`) без рамок, списком обновлений
  Arch Linux и RustyOS (`checkupdates`), потоковым прогрессом/логом и
  запуском `pacman` через валидируемый Polkit-хелпер `rustyos-update-helper`.

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
│   │       ├── shell_render/  # mod (фасад), text_pass, overlay_shapes, chrome_shapes, tokens, top_bar (panel, menus, status, notifications), dock, dock_stack, context_menu, quick_settings, updates_popover, spotlight_overview, lock_screen, modals (polkit, portal, magnifier, settings), ssd
│   │       ├── input/         # keyboard, pointer, shell_pointer, gestures
│   │       └── surfaces/      # UiShapeCache, sdf, glyphs, xcursor, liquid_glass, window_shadow, window_minimize
│   ├── rusty-wm/              # Детерминированная модель WM и workspace 1..9
│   ├── rusty-animation/       # Refresh-rate independent animation engine
│   ├── rusty-ipc/             # Версионируемый IPC-протокол compositor <-> shell
│   └── rusty-services/        # Адаптеры NetworkManager, BlueZ, PipeWire, UPower, sysfs, logind, Polkit, Portal, file_manager, image_decode, updates
│       └── src/               # lib (ServiceRegistry), power, connectivity, media, display_session, auth, logind, polkit, portal, file_manager (types, listing, trash, operations, metadata), image_decode, updates
├── apps/
│   ├── rusty-files/           # Самостоятельный Wayland-файловый менеджер (CSD, render: canvas, chrome, sidebar, views, dialogs, popup, mod; layout; state)
│   ├── rusty-settings/        # Самостоятельное Wayland CSD-окно System Settings (`882×600`, flat sidebar, scroll, TOML + `RELOAD_CONFIG`)
│   ├── rusty-shell/           # Внешний процесс shell поверх Wayland layer-shell + IPC
│   └── rusty-updater/         # Автономный GUI-клиент обновления Arch Linux и RustyOS (CSD, Этап B)
├── bin/
│   └── rustyos-session        # Лаунчер пользовательской сессии
├── packaging/
│   ├── applications/          # org.rustyos.updater.desktop
│   ├── arch/PKGBUILD          # Arch Linux PKGBUILD (rustyos-git)
│   ├── polkit/                # org.rustyos.updater.policy (rustyos-update-helper)
│   └── wayland-sessions/      # rustyos.desktop
├── scripts/
│   └── install.sh
└── docs/
    ├── adr/                   # ADR-0001 .. ADR-0058
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
  - Стандарт контекстных и выпадающих меню (`top_bar/menus.rs`, `shell_render/context_menu.rs`, `crates/rusty-compositor/src/frame_composer.rs`, `apps/rusty-files/src/{layout.rs, render/popup.rs, wayland.rs}`): концентрический `MENU_POPOVER_RADIUS = 13`, `MENU_POPOVER_PAD_Y = 5`, `MENU_ITEM_HEIGHT = 24`, `MENU_SEPARATOR_HEIGHT = 9`, инсетные пилюли ховера `HOVER_PILL_RADIUS = 7`, `HOVER_PILL_INSET_H = 5`, `HOVER_PILL_INSET_V = 1`, двухрежимное левое выравнивание (`icon: Option<UiGlyphKind>`, `14 px` без иконки / `14 px` глиф + `35 px` текст; в `rusty-files` раздельные `any_non_swatch_checked` / `swatch_rgb`: `x + 14` для текстовых строк, `cx = x + 19.0` + `x + 32` для tag-кружков, `x + 14` чекмарк + `x + 28` текст для `SortMenu`), многостадийная SDF-тень `(16, 52)`, направленный внутренний `1 px` hairline (верх `100%`, низ `35%`), колонка шорткатов `12 px`. Клиентские `xdg_popup` (`sync_popup_surface`) передают точный `wl_region` (`spec.layout.rect`) и прозрачный SHM-буфер при активном композиторном стекле (`render_popup_surface_pixels_with_glass`) (`ADR-0043`, `D-051`, `ADR-0053`, `ADR-0058`, `D-071`).
  - Унифицированный GPU-шейдер Liquid Glass и CPU-fallback (`crates/rusty-compositor/src/surfaces/liquid_glass.rs`, `sdf.rs`): `LIQUID_GLASS_FRAG` с кубическим суперэллипсом (`cornerExtent = 1.5*r`), 14 px линзой (`ior = 1.7`) и 3-слойным направленным `uRim` для `GlassPanelStyle::{Dock, Popover, ControlModule}` плюс матовый карточный вариант `GlassPanelStyle::NotificationCard` (`refract = 3.0`, `rim = 0.22`, `alpha ≈ 0.75..0.79`) и `LiquidGlassPipeline::invalidate_all()` при смене темы/обоев (ADR-0052, ADR-0053, ADR-0061).
  - First-party Wayland-протокол Liquid Glass `rustyos_liquid_glass_v1`: серверный менеджер `crates/rusty-compositor/src/state/wayland_handlers/liquid_glass.rs`, композиторная отрисовка в `frame_composer.rs` (`push_window_liquid_glass_elements` для основных окон и `push_client_popup_elements` / `push_popup_glass_panels` для `xdg_popup` поверх `PrepareBackdrop` и окон/Dock), клиентская интеграция в `apps/rusty-files` (`liquid_glass.rs`, `wayland.rs`) (ADR-0048, D-056, ADR-0058).
  - Сеть, проводное подключение, `WifiPopover` и страницы `Wi-Fi` / `Network` (`RUSTYOS-108`, `ADR-0059`, `D-072`):
    - `crates/rusty-services/src/connectivity.rs`: разделение Ethernet и Wi-Fi в `NetworkStatus` (`wired_connected`, `wired_interface`, `wired_label`, `wifi_connected`, `wifi_networks: Vec<WifiAccessPoint>`), сбор списка сетей `NetworkService::list_wifi_networks` / `parse_nmcli_wifi_list` и подключение `NetworkService::connect_wifi`;
    - `crates/rusty-compositor`: глиф `UiGlyphKind::Ethernet` (`surfaces/glyphs.rs`), отображение `Ethernet` в `TopBar` при `wired_connected && !wifi_connected`, клик `TopBarHit::Wifi` / правый клик `QuickSettingsHit::ToggleWifi`, оверлей `ShellOverlayView::Wifi` (`shell_render/wifi_popover.rs`, `WifiPopoverRenderState`, `WifiPopoverHit`, `KeyIntercept::WifiPopoverClose`, `MENU_POPOVER_RADIUS = 13`, `HOVER_PILL_RADIUS = 7`), изолированная подпись Wi-Fi в `QuickSettingsRenderState` (`wifi_connected`, `wifi_ssid`) без изменения геометрии `Control Center` / `Dock`;
    - `apps/rusty-settings/src/{state.rs, layout.rs, render.rs}`: раздельные страницы `SettingsPage::Wifi` (`RowControlId::{WifiEnabledToggle, WifiConnectIndex, NetworkRefresh}`) и `SettingsPage::Network` (карточки сервисов `Wi-Fi`, `Ethernet`, `Firewall` + `Other Services`).
  - `Notification Center`, триггер `Clock` и настройки уведомлений (`RUSTYOS-109`, `ADR-0060`, `D-073`):
    - `crates/rusty-compositor/src/config.rs` + `frame_composer.rs`: `NotificationPreviewsMode` (`Always`, `WhenUnlocked`, `Never`), секция `[notifications]` (`notifications_allow`, `notifications_show_previews`, `notifications_when_sleeping`, `notifications_when_locked`, `notifications_mirroring_off`), `apply_notifications_kv` и `append_notifications_toml`;
    - `crates/rusty-compositor/src/shell_render/{notification_center.rs, top_bar/panel.rs, overlay_shapes.rs, text_pass.rs}` + `state/*` + `input/*`: разграничение клика `Control Center` и `check_top_bar_clock_click_for_options`, флаг `TopBarTelemetry::notification_center_open`, глифы `UiGlyphKind::{CloudSun, Checklist}` (`surfaces/glyphs.rs`), долговременная история `notification_history`, панель `ShellOverlayView::NotificationCenter` (`NotificationCenterRenderState`, `NotificationCenterHit`, `compute_notification_center_layout`, `hit_test_notification_center`, `render_notification_center_text`, `render_notification_center_composite`, виджеты `Weather` и интерактивный `Reminders`, `Edit Widgets`, `KeyIntercept::NotificationCenterClose`) без правок `liquid_glass.rs`, `sdf.rs`, `dock.rs` и `quick_settings.rs`;
    - `apps/rusty-settings/src/{state.rs, layout.rs, render.rs}`: страница `SettingsPage::Notifications` (`#FF3B30`, `NotificationHeader`, `Notification Center`, `Application Notifications`, `Menu Bar & Updates`, `RowControlId::{NotifAllowToggle, NotifShowPreviewsCycle, NotifWhenSleepingToggle, NotifWhenLockedToggle, NotifMirroringCycle, NotifAppToggle}`).
- **Эталонный стиль системных CSD-окон (`ADR-0055`, `ADR-0057`):**
  - Единые константы окон и теней (`crates/rusty-compositor/src/shell_render/tokens.rs`, `apps/rusty-{settings,files,updater}/src/layout.rs`): `WINDOW_RADIUS = 14`, `WINDOW_CORNER_RADIUS = 14`, `1 px` внешний контур (`[0, 0, 0, 24]` Light / `[255, 255, 255, 22]` Dark) строго **до** угловой маски (`apply_window_corner_mask` / `clip_rounded_corners`).
  - Единые `14 px` traffic lights (`Rect(14/36/58, 13, 14, 14)`, центры `(21/43/65, 20)`, `r = 7.0`, `#EE5A6A` / `#F4B63F` / `#3CC68A`, без квадратной рамки ореола, с hover-глифами).
  - Сайдбар `#ECEDF1`/`#262628`, активный пункт `r = 10` (`#D9DADD`/`#3E3E41`), холст `#F5F6FA`/`#1C1C1D`, карточки (`CARD_CORNER_RADIUS = 12`, `#FFFFFF`/`#2C2C2D`) и капсулы тулбара/фильтров без серых рамок.
- **Нетривиальные сигнатуры и lint-правила:**
  - `KeyboardHandle::change_repeat_info(&self, rate: i32, delay: i32)` (порядок `(rate, delay)`, обратный к `seat.add_keyboard(xkb, delay, rate)`).
  - `set_output_rotation(id, rot, use_winit_fbo_flip)`: `use_winit_fbo_flip = name.eq_ignore_ascii_case("winit-0")` (`true` для Winit FBO, `false` для DRM/KMS).
  - `TextCache::render_label_weighted(renderer, text, FontWeight, size_px, rgba, loc)` и `render_label_centered_weighted(renderer, text, FontWeight, size_px, rgba, logical_rect)`.
  - Использовать `.map_or_default(...)` вместо `.map(...).unwrap_or_default()` (Clippy `1.100+` в CI) и не допускать `expect_used` / `items_after_statements` / `too_many_lines` (`>100`) даже в `#[cfg(test)]`.
