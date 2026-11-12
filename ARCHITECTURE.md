# RustyOS --- Architecture

Статус: целевая архитектура. Не является разрешением реализовать все
компоненты сразу.

## 1. Архитектурная модель

RustyOS состоит из собственного Wayland compositor и независимых
пользовательских компонентов. Compositor отвечает за display, input
routing, поверхности, управление окнами, рендеринг и presentation. Shell
отвечает за пользовательские интерфейсы и системные affordances.

## 2. Компоненты

### Compositor

- Wayland display/server lifecycle;
- clients, surfaces, outputs;
- input routing и focus;
- интеграция window management;
- rendering/presentation;
- frame scheduling;
- native DRM/KMS backend.

Не должен содержать всю бизнес-логику shell или блокироваться на
медленных системных сервисах.

### Window Manager

Логическая модель: - outputs; - workspaces; - windows; - focus; -
layout/placement; - stacking; - shortcuts/actions.

На раннем этапе может быть crate внутри compositor process. Выделение в
отдельный процесс требует ADR и approval.

### Renderer

Абстрагирует: - GPU/device capabilities; - rendering context; -
surfaces/textures; - damage; - presentation; - fallback paths.

Начальный кандидат --- инфраструктура Smithay + EGL/OpenGL ES, после
проверки API и драйверов. Vulkan/wgpu не добавлять без измеримого
требования и approval.

### Animation Engine

- единая временная шкала;
- monotonic clock;
- easing;
- interrupt/reverse/cancel;
- reduced motion;
- независимость от fixed FPS;
- отсутствие блокирующей работы в event loop.

### Shell

- panel;
- dock;
- launcher;
- notifications;
- quick settings;
- overview;
- lock screen.

Shell должен иметь независимый lifecycle и не завершать compositor при
собственном падении. IPC должен быть версионируемым. Transport ---
открытый вопрос для ADR.

### System Services

Предпочитать существующую инфраструктуру: - PipeWire --- audio/media; -
NetworkManager --- networking; - BlueZ --- Bluetooth; - Polkit ---
authorization; - XDG Desktop Portal --- desktop integration; - XWayland
--- X11 compatibility; - systemd/logind --- session/device lifecycle; -
UPower --- power/battery.

Не переписывать эти сервисы без доказанной необходимости.

## 3. Предлагаемая структура Cargo workspace

``` text
rustyos/
├── Cargo.toml
├── rust-toolchain.toml
├── crates/
│   ├── rusty-compositor/
│   ├── rusty-wm/
│   ├── rusty-renderer/
│   ├── rusty-animation/
│   ├── rusty-ipc/
│   ├── rusty-config/
│   ├── rusty-services/
│   └── rusty-diagnostics/
├── apps/
│   ├── rusty-shell/
│   ├── rusty-panel/
│   ├── rusty-dock/
│   └── rusty-settings/
├── tests/
├── docs/
└── packaging/
```

Это целевая карта, не обязательный список для немедленного создания.
Добавляй crate только при реальной границе и потребителе.

## 4. Начальный стек-кандидат

  Область                Кандидат                           Условие
  ---------------------- ---------------------------------- --------------------------------
  Язык                   Rust stable                        Зафиксировать MSRV
  Display protocol       Wayland                            Основной протокол
  Compositor framework   Smithay                            Проверить актуальные API
  Nested prototype       Smithay Winit                      Первый runnable milestone
  Native output          DRM/KMS                            После nested prototype
  Graphics               EGL + GLES                         Проверить драйверный путь
  Input                  libinput                           Проверить permissions
  Event loop             calloop или совместимый механизм   Не блокировать
  Audio                  PipeWire                           Интегрировать, не переписывать
  Config                 TOML-кандидат                      Схема и миграции явные
  Session                systemd/logind                     Не заменять без причины

## 5. Потоки и ошибки

- Event loop выполняет короткие операции.
- IO и тяжёлые вычисления не блокируют presentation path.
- Потоки создаются под измеренную потребность.
- Shared state и synchronization документируются.
- Ошибка optional feature не должна рушить весь desktop.
- Отсутствующая capability приводит к fallback и диагностике.

## 6. Архитектурные решения

Крупные решения фиксировать в `docs/adr/NNNN-title.md`. До approval
агент может подготовить proposal, но не внедрять спорное изменение.
