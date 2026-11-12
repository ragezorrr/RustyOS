# ADR-0013: Low-Latency Input и Presentation Feedback (`zwp_relative_pointer_manager_v1` + `zwp_pointer_constraints_v1` + `wp_presentation`) (`RUSTYOS-044`)

- **Статус:** accepted
- **Дата:** 2026-11-12
- **Автор:** AI Agent + User

## Контекст

Композитор уже имел `drm_backend.rs` с per-CRTC `VBlank`, VRR и Direct Scanout, однако прикладной цикл low-latency ввода и обратной связи кадра оставался неполным:
1. В `RustyCompositorState` отсутствовали Wayland-протоколы `zwp_relative_pointer_manager_v1`, `zwp_pointer_constraints_v1` и `wp_presentation`.
2. `handle_pointer_motion` в `input.rs` не отправлял `PointerHandle::relative_motion` с не-акселерированной дельтой (`delta_unaccel`) и не учитывал `LockedPointer` / `ConfinedPointer` (включая `cursor_position_hint` и деактивацию на shell-оверлеях / `LockScreen`).
3. Ни `drm_backend.rs`, ни `winit_backend.rs` не собирали `OutputPresentationFeedback` после `render_frame` и не отмечали `.presented(...)` с флагами `Vsync`/`HwClock`/`ZeroCopy` и `Refresh::Variable`/`Fixed` на `handle_vblank`.

## Рассмотренные варианты

- **Вариант A (Выбран):** Зарегистрировать встроенные в Smithay `0.7` `RelativePointerManagerState`, `PointerConstraintsState` (+ `PointerConstraintsHandler`) и `PresentationState` (monotonic clock `1`), добавить `Clock<Monotonic>` в `RustyCompositorState`, реализовать `PointerHandle::relative_motion` и поддержку `LockedPointer`/`ConfinedPointer` (с `cursor_position_hint`, hit-check по region и безопасной деактивацией при открытии Launcher/Overview/LockScreen) в `input.rs` и `state.rs`, а также собирать и завершать `OutputPresentationFeedback` в `drm_backend.rs` (`handle_vblank` с `VRR`/`ZeroCopy`, `.discarded()` при timeout/ошибке) и `winit_backend.rs`.
- **Вариант B:** Только `zwp_relative_pointer_manager_v1` и `zwp_pointer_constraints_v1` без `wp_presentation` (игры и видеоплееры не получают frame pacing телеметрию `VRR`/`ZeroCopy`).
- **Вариант C:** Только `wp_presentation` с `input` later или custom IPC для pointer lock (ломает совместимость со стандартными Wayland/XWayland играми и `gamescope`).

## Решение

Утверждён **Вариант A** (`APPROVE:` пользователя от 2026-11-12).

## Последствия

- Игры, удалённые рабочие столы и полноэкранные 3D-приложения получают сырую относительную дельту мыши (`relative_motion`), `LockedPointer`/`ConfinedPointer` без потери управления системными оверлеями (`Launcher`/`Overview`/`LockScreen`) и точный `wp_presentation` feedback.
- `drm_backend.rs` завершает `OutputPresentationFeedback` строго при фактическом `VBlank`, выставляя `ZeroCopy` при `PrimaryPlaneElement::Element(_)` и `Refresh::Variable` при активном VRR.
- Требует последующей валидации на реальном оборудовании (bare-metal `--drm` + Wayland-игры и XWayland-клиенты).
