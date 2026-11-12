# RustyOS --- Final Development Roadmap

Статус: **план утверждён как направление; технические развилки остаются
открытыми до ADR и согласования**.

## Продуктовая цель

RustyOS --- самостоятельная Linux Desktop Environment на Rust с
собственным Wayland compositor, window management, renderer, animation
engine и desktop shell.

Главные приоритеты: отзывчивость, плавность, низкая задержка, GPU
acceleration, цельный UX и подтверждённая поддержка оборудования.

## Фаза 0 --- Project Foundation

**Цель:** воспроизводимая основа разработки.

Работы: - Git repository и Cargo workspace; - Rust toolchain и MSRV; -
fmt, clippy, tests, CI; - logging и error handling; - ADR process; -
документация локальной сборки.

Критерии выхода: - `cargo fmt --check` проходит; -
`cargo check --workspace` проходит; -
`cargo clippy --workspace --all-targets -- -D warnings` проходит либо
ограничения документированы; - `cargo test --workspace` проходит; -
сборка воспроизводима; - системные настройки не менялись без approval.

## Фаза 1 --- Nested Compositor Prototype

**Цель:** минимальный Wayland compositor внутри обычной графической
сессии.

Работы: - Smithay Winit backend; - Wayland display/server lifecycle; -
клиентские поверхности; - базовый input; - корректное завершение и
диагностика.

Критерии выхода: - compositor запускается в nested window; - тестовый
Wayland client отображается; - ввод работает; - disconnect/shutdown
обрабатываются корректно; - event loop не зависает в штатном сценарии.

## Фаза 2 --- Window Management

**Цель:** детерминированное управление окнами.

Работы: - outputs, workspaces, windows, focus; - layout и placement; -
stacking; - keyboard shortcuts; - несколько окон и outputs.

Критерии выхода: - модель WM покрыта unit tests; - focus/layout работают
предсказуемо; - проверен multi-window сценарий.

## Фаза 3 --- Native Graphics Backend

**Цель:** вывод непосредственно на реальное устройство.

Работы: - capability discovery; - DRM/KMS lifecycle; - GBM/EGL; -
DMA-BUF и synchronization там, где поддерживаются; - page
flip/presentation; - hotplug и modeset; - fallback paths.

Критерии выхода: - native backend работает на конкретном
документированном GPU; - записаны kernel, driver и userspace versions; -
ограничения известны; - nested backend остаётся доступным для
разработки.

## Фаза 4 --- Performance Foundation

**Цель:** измерять причины рывков и задержек.

Работы: - frame timing; - presentation timestamps; - missed/late
frames; - input-to-display latency; - event-loop stalls; - damage
tracking; - benchmark harness.

Критерии выхода: - воспроизводимые benchmark-сценарии; - baseline для
тестового оборудования; - изменения производительности сравниваются
до/после.

## Фаза 5 --- Animation Engine

**Цель:** плавные переходы, независимые от частоты обновления.

Работы: - monotonic animation clock; - easing curves; -
interruptible/reversible transitions; - reduced-motion mode; -
тестирование разных refresh rates.

Критерии выхода: - анимации не привязаны к фиксированным 60 FPS; -
переходы можно корректно прервать; - reduced motion не ломает UX.

## Фаза 6 --- Desktop Shell

**Цель:** собственная полноценная оболочка.

Компоненты: - top bar/system status; - dock/task switcher; - app
launcher; - notifications; - quick settings; - workspace overview; -
lock screen; - wallpaper management.

Критерии выхода: - shell имеет независимый lifecycle; - shell можно
перезапустить без перезапуска compositor; - IPC документирован и
версионируется.

## Фаза 7 --- System Integration

**Цель:** интеграция базовых функций Linux.

Сервисы: - PipeWire; - NetworkManager; - BlueZ; - Polkit; - XDG Desktop
Portal; - XWayland; - systemd/logind; - UPower.

Критерии выхода: - используются штатные API; - минимальные привилегии; -
корректная обработка отсутствующих сервисов и отказов доступа.

## Фаза 8 --- Core UX

**Цель:** завершённые повседневные desktop workflows.

Работы: - display/input/appearance settings; - запуск приложений; -
автозапуск; - мультимониторный UX; - доступность; - локализация; -
файловый workflow (собственная реализация или интеграция --- отдельное
решение).

Критерии выхода: - основные сценарии документированы и проверены; -
настройки сохраняются и применяются предсказуемо.

## Фаза 9 --- Hardware Compatibility

**Цель:** расширение поддержки AMD, Intel и NVIDIA.

Проверки: - разные kernel/driver combinations; - multi-monitor и mixed
refresh; - hotplug; - suspend/resume; - hardware cursor; - VRR, direct
scanout и DMA-BUF, если доступны.

Критерии выхода: - матрица реальных тестов заполнена; -
unsupported/degraded cases документированы; - fallback paths работают.

## Фаза 10 --- Distribution and Installer

**Цель:** превратить DE в устанавливаемую систему.

Работы: - выбор базового дистрибутива; - package layout; - boot/session
startup; - installer; - recovery; - signing/update strategy; - release
engineering.

Критерии выхода: - reproducible build; - установка и восстановление
проверены в VM и на целевом оборудовании; - release artifacts
публикуются только после approval.

## Ближайший порядок

1.  Read-only аудит dev environment.
2.  RUSTYOS-001: bootstrap и CI.
3.  RUSTYOS-002: минимальный Smithay Winit compositor.
4.  Отображение тестового Wayland client.
5.  RUSTYOS-003: frame timing instrumentation.
6.  Только затем расширение функциональности и native backend.

## Правила закрытия фазы

Фаза закрывается, когда критерии выхода проверены, тесты воспроизводимы,
ограничения записаны, нет блокирующих дефектов и все требовавшие
approval решения согласованы.
