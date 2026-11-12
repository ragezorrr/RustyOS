# ADR-0010: Direct Scanout & Client `zwp_linux_dmabuf_v1` Support (`RUSTYOS-041`)

- **Статус:** accepted
- **Дата:** 2026-11-12
- **Автор:** AI Agent + User

## Контекст

`drm_backend.rs` использует `DrmCompositor::render_frame(..., FrameFlags::DEFAULT)`, где в Smithay 0.7 включён прямой scanout (`ALLOW_PRIMARY_PLANE_SCANOUT | ALLOW_OVERLAY_PLANE_SCANOUT | ALLOW_CURSOR_PLANE_SCANOUT`), однако на практике zero-copy direct scanout не срабатывал по трём причинам:
1. В `RustyCompositorState` отсутствовал Wayland-протокол `zwp_linux_dmabuf_v1` (`DmabufState` / `DmabufHandler`), поэтому клиенты не могли передавать композитору аппаратные `DMA-BUF` буферы.
2. Отсутствовало состояние полноэкранного окна (`fullscreen` без рамок SSD и с выходом за пределы `usable_work_area`) и обработка `xdg_toplevel::set_fullscreen` / `unset_fullscreen` / XWayland fullscreen.
3. При построении списка рендера в `build_frame_render_list` `TopBar`, `Dock` и декорации перекрывали клиентские поверхности, не позволяя `DrmCompositor` выводить единственную полноэкранную поверхность напрямую в KMS primary plane.

## Рассмотренные варианты

- **Вариант A (Выбран):** Зарегистрировать `zwp_linux_dmabuf_v1` (`DmabufState`/`DmabufHandler`) с форматами от `EGLDisplay`, добавить `fullscreen`-состояние и `WmAction::ToggleFullscreen` в `rusty-wm` + `XdgShellHandler`/`XwmHandler`, отключать shell chrome и SSD под неперекрытыми полноэкранными окнами в `build_frame_render_list`, использовать консервативный `FrameFlags::DEFAULT` в `DrmCompositor` с автоматическим fallback в GLES и учётом `direct_scanout_frames` в `FrameTimingCollector`.
- **Вариант B:** Только `zwp_linux_dmabuf_v1` без полноэкранного обхода композитинга (не достигает цели zero-copy KMS scanout).
- **Вариант C:** Форсированный `ALLOW_PRIMARY_PLANE_SCANOUT_ANY` и ручное управление KMS cursor plane в обход `DrmCompositor` (высокий риск `EINVAL`/артефактов на разных GPU).

## Решение

Утверждён **Вариант A** (`APPROVE:` пользователя от 2026-11-12).

## Последствия

- Wayland/XWayland клиенты могут использовать `zwp_linux_dmabuf_v1` для zero-copy передачи GPU-буферов как в DRM/KMS, так и в nested Winit.
- Полноэкранные приложения разворачиваются на всю геометрию выхода без SSD и панелей, а при появлении оверлеев (`LockScreen`, `Spotlight`, `QuickSettings`, `Overview`, `OSD`, `Notifications`) композитор мгновенно возвращается к GLES-композитингу.
- Телеметрия `FrameTimingSummary` фиксирует `direct_scanout_frames` для верификации на реальном оборудовании (`HARDWARE.md` / `PERFORMANCE.md`).
