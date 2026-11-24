# ADR-0098: Клиентский streaming GLES-презентер, HiDPI, исправления `rusty-terminal` и DnD в `rusty-files` (`RUSTYOS-169`)

- **Статус:** Accepted
- **Дата:** 2026-11-24
- **Решение:** `D-112`
- **Задача:** `RUSTYOS-169` (Фаза 2 полного аудита системы)

## Контекст

Полный аудит клиентского слоя (`rusty-ui`, `rusty-terminal`, `rusty-files`, `rusty-settings`) выявил группу дефектов графического презентера, HiDPI-масштабирования, жизненного цикла PTY, Unicode/VT-обработки и интерактивности приложений:

1. **Утечка VRAM и артефакты GLES2 в `crates/rusty-ui`:**
   - `apps/rusty-files` и `apps/rusty-settings` на каждом кадре клонировали `cpu_backbuf` в новый `Arc<[u8]>` и передавали его в `UiDrawCmd::Image`, что при кэшировании спрайтов по `Arc::as_ptr` и новой `frame_rev` создавало до 700 МБ–2.1 ГБ VRAM-утечек и лишние `glGenTextures`/`glTexImage2D`.
   - `push_line_quad` рисовал диагональные линии как залитый AABB вместо повёрнутого четырёхугольника.
   - При переполнении глиф-атласа посреди кадра `reset_glyph_atlas()` очищал атлас без предварительного flush накопленных текстовых вершин, искажая ранее добавленный текст.
   - `WaylandClipboardBridge::serve_send_request` писал в `write_fd` синхронно на UI-потоке, а `register_source` не вызывал `.destroy()` для вытесненных `WlDataSource`.
2. **HiDPI (`set_buffer_scale`):** `CompositorHandler::scale_factor_changed` в `rusty-files`, `rusty-settings` и `rusty-terminal` был заглушкой, из-за чего клиенты рендерились в 1× на HiDPI-мониторах.
3. **`apps/rusty-terminal`:**
   - Клик по вкладке (`HitTarget::Tab`) эмитил `StartInteractiveMove`, мешая переключению вкладок.
   - `PtySession::drop` блокировал UI-поток через `worker.join()` и посылал `SIGKILL` только лидеру вместо `SIGHUP` группе процессов (потеря истории shell и зависание UI).
   - `VtGrid::put_char` не учитывал ширину символов (combining marks, CJK, Emoji), `OSC 7` не декодировал `%XX`, `search_scrollback` делал ASCII-lowercase (ломая кириллицу), `--no-gpu` SHM-рендерер не рисовал панель поиска, подсветку совпадений и кнопки `'×'` на вкладках, `set_title` вызывался каждый кадр, а сброс глиф-атласа в `gpu_egl.rs` не обнулял текстуру и не перестраивал вершины кадра.
4. **`apps/rusty-files`:**
   - Внутренний DnD не обновлял `drag.target_dir` при движении курсора, а `DataDeviceHandler` не принимал входящий `text/uri-list`.
   - ПКМ по уже выделенному элементу сбрасывал мультивыделение; контекстный `MoveToTrash` и `action_duplicate_selected` действовали только на один элемент.
   - Фоновые воркеры загрузки/поиска/превью/инфо/трансфера/архивов не будили `calloop` через `Ping`, а активные задачи архивации не учитывались в `has_pending_jobs()`.

## Решение

1. **`crates/rusty-ui`:**
   - В `GlesSurfaceRenderer` добавлена одна переиспользуемая streaming-текстура (`streaming_tex`, `GL_BGRA_EXT` при поддержке `GL_EXT_texture_format_BGRA8888` с fallback на swizzle) и метод `present_bgra_frame(&mut self, pixels, buf_w, buf_h, buffer_scale, damage, clear_rgba)` с частичной загрузкой `glTexSubImage2D` по `damage`.
   - `push_line_quad` переписан на истинный повёрнутый quad по нормали `(-dy/len, dx/len) * half_w`.
   - В `render_draw_list` `Viewport`/`Clear` выполняются до `compile_batches`, а перед `reset_glyph_atlas()` вызывается `flush_pending_batches`.
   - `WaylandClipboardBridge::register_source` вызывает `.destroy()` для вытесненных `WlDataSource`; `serve_send_request` отдаёт payload в фоновом потоке `rusty-ui-clip-send`.
2. **Streaming GLES и целочисленный HiDPI (`rusty-files`, `rusty-settings`, `rusty-terminal`):**
   - `rusty-files` и `rusty-settings` переведены на `present_bgra_frame` (с передачей `DamageScope` в `rusty-settings`), убраны покадровые `Arc::from(cpu_backbuf)` и `UiDrawList`-обёртки.
   - Во всех трёх приложениях подключён `CompositorHandler::scale_factor_changed` + `wl_surface.set_buffer_scale(scale)` с масштабированием EGL-viewport / SHM-буфера.
3. **`apps/rusty-terminal`:**
   - Удалён `StartInteractiveMove` при клике по `HitTarget::Tab`.
   - `PtySession::drop` шлёт `kill_process_group(pgid, Signal::HUP)` и переносит `worker.join()` + ожидание (250 мс) + `Signal::KILL` в фоновый поток `rusty-term-reaper`.
   - В `vt.rs` добавлены встроенная таблица `char_display_width` (0/1/2 колонки без новых крейтов) с продолжением `'\0'` для широких глифов, `decode_percent_uri_path` для `OSC 7` и регистронезависимый Unicode-поиск `search_scrollback` (`char::to_lowercase()`).
   - В `renderer_shm.rs` добавлена отрисовка панели поиска, подсветки совпадений и кнопок закрытия вкладок `'×'`.
   - В `wayland.rs` добавлен кэш `last_window_title`, неблокирующий `send_request` буфера обмена (`rusty-term-clip-send`) и `.destroy()` старых `CopyPasteSource`.
   - В `gpu_egl.rs` при переполнении глиф-атласа текстура обнуляется и выполняется пересборка вершин кадра (`atlas_reset_mid_frame`).
4. **`apps/rusty-files`:**
   - Добавлены `resolve_drop_target_dir` (папки, сайдбар, path bar, пустая колонка) для внутреннего DnD и `DataDeviceHandler::{enter, motion, drop_performed}` + `parse_uri_list_paths` для входящего `text/uri-list`.
   - В `execute_right_click` сохраняется мультивыделение при ПКМ по уже выделенному элементу; `MoveToTrash` и `action_duplicate_selected` работают со всеми `selected_entries()`.
   - Все фоновые воркеры (`load`, `home_search`, `preview`, `info`, `transfer`, `archive`) будят `calloop` через `worker_ping`; добавлен счётчик `active_archive_jobs` в `has_pending_jobs()`.

## Последствия

- **Плюсы:** нулевая утечка VRAM в `rusty-files`/`rusty-settings`, мгновенное пробуждение UI по завершении фоновых задач, отсутствие фризов при закрытии вкладок терминала и копировании больших буферов, корректная работа CJK/Emoji/кириллицы/HiDPI и полноценный DnD в `rusty-files`.
- **Ограничения:** 0 новых Cargo-зависимостей; весь `unsafe` остаётся строго в уже изолированных модулях `crates/rusty-ui/src/gpu_gles.rs` и `apps/rusty-terminal/src/gpu_egl.rs`.
