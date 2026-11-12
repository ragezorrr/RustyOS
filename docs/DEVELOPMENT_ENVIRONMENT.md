# RustyOS --- Development Environment Baseline

Документ фиксирует проверенную среду разработки, обнаруженную в ходе аудита `RUSTYOS-001` (2026-11-11).

## 1. Система и оборудование

- **ОС:** EndeavourOS (Arch Linux rolling)
- **Ядро:** Linux `7.2.11-arch1-1` (`x86_64`)
- **GPU:** Intel Corporation Raptor Lake-U `[Intel Graphics] [8086:a7ac] (rev 04)`
- **Активный модуль ядра GPU:** `i915` (доступные модули: `i915`, `xe`)

## 2. Инструментарий Rust

- **Канал:** `stable-x86_64-unknown-linux-gnu`
- **Компилятор:** `rustc 1.99.1 (58cf8f9c3 2026-10-13)`
- **Пакетный менеджер:** `cargo 1.99.1 (5f94df478 2026-08-27)`
- **Форматирование:** `rustfmt 1.10.0-stable`
- **Линтер:** `clippy 0.1.99`
- **MSRV проекта:** `1.99`
- **Git:** `git version 2.56.0`

## 3. Установленные системные и графические dev-пакеты (`pkg-config`)

Следующие библиотеки проверены через `pkg-config` и доступны для последующих фаз (Фаза 1: Smithay + Winit, Фаза 3: Native DRM/KMS):

- **Wayland:** `wayland-server`, `wayland-client`, `wayland-cursor`, `wayland-egl`, `wayland-egl-backend`, `wayland-scanner`
- **DRM / KMS / Graphics:** `libdrm` (`libdrm_intel`, `libdrm_amdgpu`, `libdrm_nouveau`, `libdrm_radeon`), `gbm`, `egl`, `glesv2`, `pixman-1`
- **Input / Seat / Device:** `libinput`, `libudev`, `udev`, `libseat`, `xkbcommon`, `xkbcommon-x11`, `libei-1.0`, `libeis-1.0`
- **X11 Compatibility:** `xwayland`, `xwaylandproto`

## 4. Локальные проверки (Фаза 0 и Фаза 1)

```bash
cargo fmt --all -- --check
cargo check --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo run --offline -p rusty-compositor -- --winit --max-frames 30 --spawn es2gears_wayland
```

## 5. Особенности и ограничения Фазы 1 (`RUSTYOS-002`)

- **Поддерживаемые протоколы в Фазе 1:** `wl_compositor`, `wl_subcompositor`, `wl_shm`, `xdg_wm_base`, `wl_seat` (keyboard + pointer), `wl_data_device_manager`, `wl_output`, `zxdg_output_manager_v1`.
- **Графический путь:** nested Winit window (`PLATFORM_WAYLAND_KHR`, EGL 1.5, OpenGL ES 3.2 Mesa `i915`) через `GlesRenderer` и `OutputDamageTracker`.
- **Ограничения Фазы 1:**
  - `wl_drm` / `zwp_linux_dmabuf_v1` на стороне сервера в nested-режиме ещё не экспортируются (клиенты вроде `es2gears_wayland` выводят предупреждение `MESA-EGL: failed to get driver name for fd -1` и прозрачно переключаются на поддерживаемый буферный путь; DMA-BUF будет добавлен на этапе развития рендерера/native backend).
  - Горячие клавиши выхода в nested-окне: `Escape` или `Ctrl+Alt+Backspace` (а также штатное закрытие окна Winit).

