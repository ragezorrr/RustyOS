# ADR-0015: Explicit Sync (`wp_linux_drm_syncobj_v1`), Observable Hardware Cursor Plane и HiDPI-курсоры (`RUSTYOS-046`)

- **Статус:** accepted
- **Дата:** 2026-11-12
- **Автор:** AI Agent + User

## Контекст

На KMS-бэкенде (`drm_backend.rs`) композитор уже использовал `DrmCompositor`, direct scanout, `wp_linux_dmabuf_v1` и `wp_presentation`, однако оставались три пробела низколатентного графического пайплайна и совместимости:
1. Отсутствовал протокол explicit synchronization (`wp_linux_drm_syncobj_v1`): при наличии `DMA-BUF` / Vulkan / EGL клиентов синхронизация опиралась на implicit sync, что ограничивает современные драйверы (включая NVIDIA 555+ и актуальный Mesa/RADV/ANV/Iris).
2. Аппаратная плоскость курсора (`DrmSurface::planes().cursor` + `Kind::Cursor`) работала непрозрачно: не проверялись доступные cursor planes на выходе, отсутствовал `RenderFrameResult::needs_sync()` / `element.sync.wait()` перед `queue_frame`, не велась телеметрия `hw_cursor_frames`, а `InputEvent::PointerMotion` / `PointerMotionAbsolute` перерисовывали все выходы (`mark_all_dirty()`) вместо затронутых курсором.
3. При HiDPI (`output_scale > 1.0`) размер XCursor и смещение hotspot для `Named`-курсоров выбирались по базовому 1x масштабу и без учёта аппаратного лимита `drm_device.cursor_size()`.

## Рассмотренные варианты

- **Вариант A (Выбран):**
  - В `state.rs` хранить `LoopHandle<'static, Self>`, `DrmSyncobjState` и `syncobj_surface_unblocked`, реализовать `DrmSyncobjHandler` + `delegate_drm_syncobj!`, `init_drm_syncobj` с runtime-проверкой `supports_syncobj_eventfd(drm_fd)` и безопасным fallback на implicit sync (плюс `RUSTYOS_DISABLE_EXPLICIT_SYNC`). В `CompositorHandler::new_surface` регистрировать `pre_commit_hook` (`handle_syncobj_pre_commit`) с `DrmSyncPoint::generate_blocker()` и `DrmSyncPointSource` в `LoopHandle` с последующим `compositor::add_blocker` и пробуждением рендера через `take_syncobj_unblocked()`.
  - В `drm_backend.rs` и `frame_timing.rs` учитывать `drm_surface.planes().cursor.len()`, флаг `RUSTYOS_DISABLE_HW_CURSOR`, `result.needs_sync()` -> `element.sync.wait()`, `result.cursor_element.is_some()` -> `hw_cursor_frames` (лог и JSON), поля `syncobj_eventfd` и `cursor_plane_count` в `DrmHardwareReport`, а также scoped output dirtying (`output_ids_affected_by_pointer_motion` + `mark_outputs_dirty`) при движении курсора.
  - В `surfaces.rs` и `winit_backend.rs` добавить `CursorRenderMode`, `named_cursor_element_for_output` / `cursor_arrow_element_for_output` / `build_frame_render_list_for_output_with_cursor` с корректным масштабированием hotspot/размера под `output_scale` и понижением `Kind::Cursor` в `Kind::Unspecified` при превышении `cursor_size`.
- **Вариант B:** Только `wp_linux_drm_syncobj_v1` + `DrmHardwareReport` без доработки HiDPI-курсоров, `hw_cursor_frames` и scoped pointer dirtying.
- **Вариант C:** Вариант A + полная реализация `drmModeAtomicCommit` cursor-only fast-path в обход `DrmCompositor` (высокая сложность и риск регрессий на смешанных GPU).

## Решение

Утверждён **Вариант A** (`APPROVE:` пользователя от 2026-11-12).

## Последствия

- При поддержке `supports_syncobj_eventfd` ядром и драйвером композитор экспортирует `wp_linux_drm_syncobj_v1` и асинхронно ожидает acquire timeline syncobj через `calloop` без блокировки event loop; на неподдерживаемых устройствах или при `RUSTYOS_DISABLE_EXPLICIT_SYNC=1` автоматически сохраняется implicit sync.
- Hardware cursor plane становится полностью наблюдаемым в `--probe-drm` JSON, стартовых логах, `FrameTimingSummary` (`hw_cursor_frames`) и управляемым через `RUSTYOS_DISABLE_HW_CURSOR=1`, а перемещение курсора на multi-monitor конфигурации перерисовывает только затронутые выходы.
- Живое подтверждение `wp_linux_drm_syncobj_v1` и cursor plane на физическом оборудовании AMD/Intel/NVIDIA требует запуска `--drm` на bare-metal хосте.
