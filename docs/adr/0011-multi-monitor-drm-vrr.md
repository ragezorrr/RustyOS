# ADR-0011: Multi-Monitor DRM/KMS, Hotplug, Mixed Refresh & Runtime VRR (`RUSTYOS-042`)

- **Статус:** accepted
- **Дата:** 2026-11-12
- **Автор:** AI Agent + User

## Контекст

`RUSTYOS-012` заложил логическую модель `OutputMap`/`OutputId` и `WorkArea`, однако в `drm_backend.rs` и рендер-пайплайне оставались ограничения single-output архитектуры:
1. `DrmRuntime` обслуживал только один `primary` коннектор и один `DrmCompositor`, поэтому вторичные дисплеи не сканировались, а `UdevBackend` не был подключён к `calloop` для hotplug.
2. Выбор DRM-режима брал первый preferred mode без приоритета максимальной частоты обновления (`60/120/144/240 Hz`), а runtime VRR не управлялся.
3. `build_frame_render_list` строил один общий список элементов на весь workspace без фильтрации окон/полноэкранных поверхностей по конкретному `OutputId` и без смещения в локальные координаты выхода.
4. Удаление выхода в `RustyCompositorState::remove_output` не отзывало `wl_output` `GlobalId`.

## Рассмотренные варианты

- **Вариант A (Выбран):** Перевести `DrmRuntime` на per-CRTC `HashMap<crtc::Handle, DrmOutputState>` с независимым циклом `DrmEvent::VBlank(crtc)` и page-flip для mixed refresh, подключить `UdevBackend` hotplug в `calloop` (`sync_hotplug_connectors` / `attach_secondary_connector`), добавить `select_best_drm_mode` (preferred-разрешение + max refresh при `prefer_max_refresh`), безопасный runtime VRR (`VrrMode` в `[display]` конфига с проверкой `DrmCompositor::vrr_supported`) и параметризовать `build_frame_render_list_for_output` + `WindowManager::visible_windows_for_output` + отзыв `wl_output` `GlobalId`.
- **Вариант B:** Multi-monitor только в логическом `OutputMap` без per-CRTC `DrmOutputState`, udev hotplug и VRR (не решает mixed refresh и подключение физических мониторов в KMS).
- **Вариант C:** Единый синхронный VBlank-таймер на все выходы с принудительным VRR (ломает frame pacing при mixed 60/120/144/240 Hz и несовместим с мониторами без VRR).

## Решение

Утверждён **Вариант A** (`APPROVE:` пользователя от 2026-11-12).

## Последствия

- Каждый физический DRM CRTC имеет собственный `DrmOutputState`, независимый `VBlank`/page-flip loop и собственную политику VRR (`Off` / `Fullscreen` / `Always`), что устраняет взаимную блокировку дисплеев с разной герцовкой.
- `build_frame_render_list_for_output` переводит координаты окон и указателя в локальную систему конкретного выхода, ограничивает primary chrome (`TopBar`, `Dock`, оверлеи, OSD) основным монитором и дублирует `LockScreen` на все активные выходы.
- По `HARDWARE.md` требуется последующая bare-metal проверка `--drm` на стендах AMD/Intel/NVIDIA с 2+ дисплеями, hotplug и VRR-панелью (в headless-окружении подтверждены только unit/integration тесты и логика выбора режимов/изоляции выходов).
