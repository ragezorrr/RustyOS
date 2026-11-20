# 0074. Unified Client UI Toolkit (`rusty-ui`) and Dual GLES 2.0 / CPU Presenter

- **Status:** accepted
- **Date:** 2026-11-20
- **Task:** RUSTYOS-130

## Context

До `RUSTYOS-130` системные Wayland-клиенты (`apps/rusty-settings`, `apps/rusty-files`, `apps/rusty-installer`) содержали разрозненные CPU-реализации контролов, дублировали загрузку и кеш шрифтов `Inter` (`fontdue`), использовали неполную клавиатурную декодировку evdev без `wl_data_device` буфера обмена/выделения/навигации по словам, а также не имели единого аппаратного пути презентации поверхности на `Wayland-EGL + GLES 2.0` с автоматическим CPU `wl_shm` fallback.

## Decision

1. **Полноценный клиентский UI-тулкит в `crates/rusty-ui`:**
   - `text.rs`: общий движок `UiFontEngine` для `Inter-Regular.ttf` / `Inter-SemiBold.ttf`, кеш глифов, `measure_width`, `x_for_byte`, `byte_for_x`, `word_bounds_at`, `prev_word_boundary`, `next_word_boundary`, `ellipsize`, `draw_text_on_canvas`, `draw_text_centered_on_canvas`;
   - `focus.rs`: `WidgetId` (`from_tag` FNV-1a) и `FocusScope` (`Tab`/`Shift+Tab`, `110 ms` `EaseOutCubic` затухание фокуса);
   - `clipboard.rs`: `CLIPBOARD_MIME_TYPES`, `ClipboardSink`, `MemoryClipboard` и неблокирующий `WaylandClipboardBridge` над `wl_data_device` / `wl_data_source` (`OwnedFd` pipe I/O + wake callback);
   - `motion.rs`: прерываемые анимации контролов на `rusty-animation` с немедленным snap при `reduced_motion = true` (`ToggleMotion`, `HoverPressMotion`, `ScrollMotion`, `CaretBlink`);
   - `text_input.rs`: `TextBuffer` и `TextField` (`new`, `password`, `search`) с кареткой, выделением (клик/двойной клик/тройной клик/drag), горизонтальным скроллом, кнопкой очистки и `Ctrl+A/C/X/V`;
   - `scroll.rs`, `controls.rs`, `list.rs`, `dialog.rs`, `menu.rs`, `widgets.rs`: `ScrollView` с перетаскиваемым ползунком скроллбара, `DropdownSelect`, `SegmentedTabs` с плавной пилюлей активной вкладки, `Checkbox`, `Slider`, `SelectableList`, `DialogSheet`/`DialogLayout`, `ContextMenuSession`/`paint_menu_with_painter`, `ToggleSwitch::paint_animated` и `PushButton::paint_labeled`.

2. **Единый декларативный `UiDrawList` и dual GLES 2.0 / CPU `UiCanvas` презентер:**
   - `draw.rs`: `UiDrawList`, `UiDrawCmd`, `UiPainter`, стек clip-прямоугольников, `squircle_sdf_dist` и точный CPU-исполнитель `render_draw_list_to_canvas` с профилями `ShadowElevation`;
   - `gpu_gles.rs` + `build.rs`: безопасная обёртка `GlesSurfaceRenderer` над `gl_generator` (`EGL 1.5` + `GLES 2.0`) и `wayland-egl`, объединяющая `RECT_PROG` (скругления, суперэллипс, обводки, аналитические тени), `TEXT_PROG` (`1024×1024` `ShelfPacker` альфа-атлас) и `SPRITE_PROG` (`BGRA`/`RGBA` LRU-кеш `<= 256` спрайтов). Конфигурация `EGL_ALPHA_SIZE = 8` и premultiplied blending (`ONE, ONE_MINUS_SRC_ALPHA`) сохраняют прозрачность сайдбаров для композиторного протокола `rustyos_liquid_glass_v1` (`WlRegion`).
   - **Обоснование локального `unsafe`:** workspace запрещает `unsafe_code = "deny"`; по аналогии с `ADR-0064` (`rusty-terminal`) весь FFI для `EGL`, `GLES 2.0` и `wl_egl_window` строго изолирован в единственном модуле `crates/rusty-ui/src/gpu_gles.rs` с `#![allow(unsafe_code)]`, документированными инвариантами `// SAFETY:` и автоматическим fallback на CPU `wl_shm` при `--no-gpu`, `RUSTYOS_NO_GPU=1` или ошибке EGL.

3. **Миграция системных приложений:**
   - Удалены дублирующие зависимости `fontdue` в `apps/rusty-installer`, `apps/rusty-settings` и `apps/rusty-files` (используется общий `UiFontEngine` из `rusty-ui`);
   - В трёх приложениях подключены `xkbcommon` через `smithay-client-toolkit`, `DataDeviceManagerState` + `WaylandClipboardBridge`, `TextField` (форма инсталлятора + Wi-Fi пароль, поиск в настройках, поиск/inline-переименование/Go to Folder и кнопки диалогов в `rusty-files`), `ToggleSwitch::paint_animated` и перетаскиваемый скроллбар в `rusty-settings`, а также `GlesSurfaceRenderer` с `--no-gpu` / `wl_shm` fallback;
   - `xdg_popup` меню `rusty-files` сохранили прозрачные `wl_shm`-буферы с композиторным `GlassStyle::Popover`.

## Consequences

- Системные приложения переведены на общие интерактивные контролы `rusty-ui` без дублирования шрифтовых кешей, текстовых буферов и Wayland-буфера обмена.
- Основные окна клиентов рендерятся через `Wayland-EGL + GLES 2.0` с полной визуальной идентичностью CPU `wl_shm` фолбэку и без потери прозрачного Liquid Glass сайдбаров.
- Расширение тулкита для будущих утилит (`rusty-screenshot`, настройки Bluetooth/Power/Users) не требует повторной реализации базовых виджетов.
