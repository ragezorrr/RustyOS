# ADR-0026: Нативный XDG Desktop Portal backend (`Screenshot` / `PickColor` / `ScreenCast`) и изолированный PipeWire worker (`RUSTYOS-057`)

- **Статус:** accepted
- **Дата:** 2026-11-13
- **Автор:** AI Agent + User

## Контекст

До `RUSTYOS-057` захват экрана в RustyOS работал только через встроенные горячие клавиши композитора (`screenshot.rs`), без реализации `org.freedesktop.impl.portal.desktop.*` и без потока `PipeWire` Video/Source. Из-за этого Wayland-приложения и браузеры (Firefox, Chromium/Electron, OBS Studio, графические редакторы) не могли запрашивать скриншоты, пипетку цвета (`PickColor`) и демонстрацию экрана/окна (`ScreenCast`) через стандартный `xdg-desktop-portal`.

Архитектура RustyOS предъявляет жёсткие ограничения:
- `unsafe_code = "deny"` по всему workspace (`Cargo.toml`), запрещающий прямую C FFI линковку к `libpipewire-0.3` внутри крейтов Rust;
- запрет блокировки `calloop` event loop композитора вызовами D-Bus или PipeWire/GL encoding;
- запрет на обход согласия пользователя и утечку содержимого рабочего стола при заблокированной сессии (`SECURITY.md`);
- отсутствие сторонних KDE/GNOME/wlroots portal backend как основы RustyOS и отсутствие новых crates.io зависимостей.

## Рассмотренные варианты

- **Вариант 1 (Выбран):**
  - В [`rusty-services/src/portal.rs`](../../crates/rusty-services/src/portal.rs) реализовать D-Bus сервис `org.freedesktop.impl.portal.desktop.rustyos` на уже используемом `zbus` 5 (`org.freedesktop.impl.portal.Screenshot` v3 с `Screenshot` / `PickColor`, `org.freedesktop.impl.portal.ScreenCast` v6 с `CreateSession` / `SelectSources` / `Start`, а также динамическими объектами `Request` / `Session`), синхронизирующий снимок доступных выходов/окон и состояния блокировки без блокировки `calloop`.
  - В [`crates/rusty-services/build.rs`](../../crates/rusty-services/build.rs) и [`crates/rusty-services/c/rusty-pipewire-stream.c`](../../crates/rusty-services/c/rusty-pipewire-stream.c) собрать изолированный C11 воркер `rusty-pipewire-stream` на `libpipewire-0.3` (`pkg-config`), который экспортирует видеопоток PipeWire (`MediaType=Video`, `MediaCategory=Source`), принимает `RPW1`-кадры по `stdin`, поддерживает форматы `BGRA`/`BGRx`/`RGBA`/`RGBx`, использует `pw_core_sync` барьер перед выдачей `READY <node_id> <serial>` и сообщает статус `streaming`/`paused` для нулевой нагрузки при отсутствии потребителя.
  - В [`rusty-compositor`](../../crates/rusty-compositor/src/state.rs) добавить Liquid Glass диалог выбора источника/согласия `ShellOverlayView::Portal` и пипетку `ShellOverlayView::PickColor`, принудительно отклонять/закрывать все portal-операции и стримы при `is_session_locked()`, и подавать кадры из обоих бэкендов (`winit_backend.rs` и `drm_backend.rs`) через единый `process_portal_captures_and_streams` с переиспользуемым `OffscreenCaptureBuffer` в [`screenshot.rs`](../../crates/rusty-compositor/src/screenshot.rs).
  - Добавить конфигурации [`packaging/portals/rustyos.portal`](../../packaging/portals/rustyos.portal) и [`packaging/portals/rustyos-portals.conf`](../../packaging/portals/rustyos-portals.conf) с интеграцией в [`bin/rustyos-session`](../../bin/rustyos-session) и [`scripts/install.sh`](../../scripts/install.sh).
- **Вариант 2:** Использовать сторонний крейт `pipewire-rs` и снять/ослабить `#![deny(unsafe_code)]` или тащить тяжёлый C-bindgen граф в workspace. Отклонён как нарушающий `#![deny(unsafe_code)]`, добавляющий внешние зависимости и bindgen-связность.
- **Вариант 3:** Делегировать `ScreenCast` и `Screenshot` во внешний `xdg-desktop-portal-wlr`/`kde` или `grim`. Отклонён, так как композитор RustyOS на базе Smithay не реализует wlr-screencopy и требует единого нативного UX.

## Решение

Утверждён **Вариант 1** (`APPROVE: RUSTYOS-057 Вариант 1` от 2026-11-13).

## Последствия

- Rust-кодовая база сохраняет `#![deny(unsafe_code)]` и ноль новых crates.io зависимостей, изолируя работу с `libpipewire-0.3` в отдельном легковесном процессе `rusty-pipewire-stream` со строгим single-slot bounded drop-oldest буфером.
- Приложения получают стандартные порталы `Screenshot`, `PickColor` и `ScreenCast` для мониторов и отдельных окон с обязательным интерактивным Liquid Glass подтверждением (или немедленным отказом при Lock Screen).
- Захват кадров использует offscreen `GlesTexture` readback, что подходит для текущих ES 3.2/desktop режимов; последующая оптимизация zero-copy DMA-BUF буферов PipeWire выделяется в отдельный будущий этап после стабилизации DMA-BUF формата потребителя.
