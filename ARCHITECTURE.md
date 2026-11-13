# RustyOS --- Architecture

Статус: **актуально для реализации до `RUSTYOS-045`** (развитие новых
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
  - `state/wayland_handlers/` — сгруппированные Smithay/Wayland delegate-хендлеры (`core`, `seat_input`, `selection`, `session_lock`, `dmabuf_sync`, `desktop_protocols`);
- Wayland display/server lifecycle и клиентский input routing (`input/`: `mod`, `keyboard`, `pointer`, `shell_pointer`, `gestures`);
- multi-output раскладка (`outputs.rs`) и интеграция с `rusty-wm`;
- nested Winit backend (`winit_backend.rs`) и native DRM/KMS per-CRTC
  render loop с mixed refresh, hotplug и VRR (`drm_backend.rs`);
- direct scanout (`zwp_linux_dmabuf_v1`), `wp_fractional_scale_v1` +
  `wp_viewporter`, `zwp_relative_pointer_manager_v1` +
  `zwp_pointer_constraints_v1`, `wp_presentation` (`Vsync`/`HwClock`/`ZeroCopy`
  и `Refresh::Variable`/`Fixed`), `wp_cursor_shape_v1`, `xdg_activation_v1`,
  `ext_idle_notify_v1` + `zwp_idle_inhibit_manager_v1`, `ext_session_lock_v1`;
- `XWayland` интеграция и синхронизация clipboard (`xwayland.rs`);
- рендер-инструментация (`frame_timing.rs`), конфигурация
  (`~/.config/rustyos/config.toml` в `config.rs`), скриншоты, лончер,
  OSD/уведомления и `StatusNotifierItem` трей.

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
- 2D shell/SSD рендеринг через `UiShapeCache` (`surfaces/`: `mod`, `sdf`,
  `glyphs`, `xcursor`) и `TextCache` (`fontdue`).

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
  (`shell_render/`: `tokens`, `top_bar`, `dock`, `quick_settings`,
  `spotlight_overview`, `lock_screen`, `modals`, `ssd`) для low-latency
  взаимодействия (TopBar, Dock, Spotlight Launcher, Overview, Quick Settings,
  Lock Screen, Notifications, Alt+Tab HUD, Polkit/Portal/PickColor modals).

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
  `portal`.

## 3. Текущая структура Cargo workspace и сессии

```text
RustyOS/
├── Cargo.toml                 # workspace (edition = 2024, MSRV 1.99, unsafe_code = "deny")
├── rust-toolchain.toml
├── crates/
│   ├── rusty-compositor/      # Wayland compositor, Winit & DRM/KMS backends, frame_composer, input/, state/, shell_render/, surfaces/
│   │   └── src/
│   │       ├── frame_composer.rs  # Backend-независимая сборка render list кадра, Z-order overlay/chrome/layer-shell, HiDPI scale, readback
│   │       ├── state/         # RustyCompositorState, wm_windows, shell_actions, system_bridges, wayland_handlers/
│   │       ├── shell_render/  # tokens, top_bar, dock, quick_settings, spotlight_overview, lock_screen, modals, ssd
│   │       ├── input/         # keyboard, pointer, shell_pointer, gestures
│   │       └── surfaces/      # UiShapeCache, sdf, glyphs, xcursor
│   ├── rusty-wm/              # Детерминированная модель WM и workspace 1..9
│   ├── rusty-animation/       # Refresh-rate independent animation engine
│   ├── rusty-ipc/             # Версионируемый IPC-протокол compositor <-> shell
│   └── rusty-services/        # Адаптеры NetworkManager, BlueZ, PipeWire, UPower, sysfs, logind, Polkit, Portal
│       └── src/               # lib (ServiceRegistry), power, connectivity, media, display_session, auth, logind, polkit, portal
├── apps/
│   └── rusty-shell/           # Внешний процесс shell поверх Wayland layer-shell + IPC
├── bin/
│   └── rustyos-session        # Лаунчер пользовательской сессии
├── packaging/
│   └── wayland-sessions/      # rustyos.desktop
├── scripts/
│   └── install.sh
└── docs/
    ├── adr/                   # ADR-0001 .. ADR-0027
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
  Config                 TOML (`~/.config/rustyos/config.toml`) Display scales, VRR, XKB, UI
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
