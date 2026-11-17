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
  `fontdue` со сквозной синхронизацией темы `Light` / `Dark`;
- `rusty-files`: full-bleed/полупрозрачный sidebar, Icons/List/Columns/Gallery,
  вкладки, clickable Path Bar, Quick Look, Get Info, Copy/Move, Trash, теги и Liquid Glass поповеры;
- `rusty-settings`: отдельное окно настроек (`882×600`, `org.rustyos.Settings`) с
  плоским непрозрачным сайдбаром (`#ECEDF1`/`#262628`), карточками без рамок, скроллингом,
  выбором темы (`Light`/`Dark`/`Auto`) и живой отправкой `v1|RELOAD_CONFIG` в композитор;
- `rusty-updater`: окно *Software Update* со списком всех обновлений
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
    ├── adr/                   # ADR-0001 .. ADR-0055
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
  - Клиентский стандарт Liquid Glass Popover (`apps/rusty-files/src/render/popup.rs`): `MENU_POPOVER_RADIUS = 16`, инсетные плавающие пилюли ховера `HOVER_PILL_RADIUS = 8`, `HOVER_PILL_INSET_H = 6`, многостадийная SDF-тень, направленный внутренний `1 px` hairline (верх `100%`, низ `35%`), колонка шорткатов (ADR-0043, D-051, ADR-0053).
  - Унифицированный GPU-шейдер Liquid Glass и CPU-fallback (`crates/rusty-compositor/src/surfaces/liquid_glass.rs`, `sdf.rs`): `LIQUID_GLASS_FRAG` с кубическим суперэллипсом (`cornerExtent = 1.5*r`), 14 px линзой (`ior = 1.7`) и 3-слойным направленным `uRim` для `GlassPanelStyle::{Dock, Popover, ControlModule}` + `LiquidGlassPipeline::invalidate_all()` при смене темы/обоев (ADR-0052, ADR-0053).
  - First-party Wayland-протокол Liquid Glass `rustyos_liquid_glass_v1`: серверный менеджер `crates/rusty-compositor/src/state/wayland_handlers/liquid_glass.rs`, композиторная отрисовка в `frame_composer.rs` (`push_window_liquid_glass_elements`), клиентская интеграция в `apps/rusty-files` (`liquid_glass.rs`, `wayland.rs`) (ADR-0048, D-056).
- **Нетривиальные сигнатуры и lint-правила:**
  - `KeyboardHandle::change_repeat_info(&self, rate: i32, delay: i32)` (порядок `(rate, delay)`, обратный к `seat.add_keyboard(xkb, delay, rate)`).
  - `set_output_rotation(id, rot, use_winit_fbo_flip)`: `use_winit_fbo_flip = name.eq_ignore_ascii_case("winit-0")` (`true` для Winit FBO, `false` для DRM/KMS).
  - `TextCache::render_label_weighted(renderer, text, FontWeight, size_px, rgba, loc)` и `render_label_centered_weighted(renderer, text, FontWeight, size_px, rgba, logical_rect)`.
  - Использовать `.map_or_default(...)` вместо `.map(...).unwrap_or_default()` (Clippy `1.100+` в CI) и не допускать `expect_used` / `items_after_statements` / `too_many_lines` (`>100`) даже в `#[cfg(test)]`.
