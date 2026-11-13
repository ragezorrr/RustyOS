# ADR-0025: Неблокирующая PAM-аутентификация Lock Screen, интеграция `systemd-logind` и встроенный Polkit Agent (`RUSTYOS-056`)

- **Статус:** accepted
- **Дата:** 2026-11-13
- **Автор:** AI Agent + User

## Контекст

До `RUSTYOS-056` встроенный Lock Screen в `rusty-compositor` работал без проверки пароля, композитор не реагировал на сигналы `systemd-logind` (`PrepareForSleep`, `Session.Lock`, `Session.Unlock`) и не регистрировал собственный `org.freedesktop.PolicyKit1.AuthenticationAgent`, из-за чего в самостоятельной Wayland-сессии RustyOS графические запросы повышения привилегий (`pkexec` и аналоги) не могли показать системный диалог ввода пароля.

При этом архитектура RustyOS запрещает:
- хранить или читать `/etc/shadow` напрямую или запускать композитор с root-правами;
- блокировать главный цикл `calloop` сетевыми/D-Bus/PAM вызовами;
- использовать `unsafe`-блоки или внешние тяжёлые C-привязки (`libpam-sys`) там, где достаточно системных примитивов.

## Рассмотренные варианты

- **Вариант 1 (Выбран):**
  - В [`rusty-services/src/auth.rs`](../../crates/rusty-services/src/auth.rs) реализовать неблокирующий `AsyncAuthHandle` и zeroizing-буфер `SecretText` (преаллокация 512 Б, `Drop` и `Debug` с `[REDACTED]`), проверяющий пароль текущего пользователя через системный setuid-помощник `/usr/bin/unix_chkpwd` (`/sbin/unix_chkpwd`) на выделенном потоке.
  - В [`rusty-services/src/logind.rs`](../../crates/rusty-services/src/logind.rs) добавить `LogindMonitorHandle` на System D-Bus для `org.freedesktop.login1.Manager.PrepareForSleep` (с `delay` sleep-ингибитором, освобождаемым через `ack_prepare_for_sleep` после активации Lock Screen), `Session.Lock` / `Session.Unlock` и асинхронного `SetLockedHint`.
  - В [`rusty-services/src/polkit.rs`](../../crates/rusty-services/src/polkit.rs) реализовать `PolkitAgentHandle`, регистрирующий `org.freedesktop.PolicyKit1.AuthenticationAgent` на System D-Bus с помощью уже присутствующего `zbus` 5 и взаимодействующий с системным `/run/polkit/agent-helper.socket` (или fallback `/usr/lib/polkit-1/polkit-agent-helper-1`).
  - В [`rusty-compositor`](../../crates/rusty-compositor/src/shell_render.rs) добавить Liquid Glass поле ввода пароля (`••••`) и статус ошибки на Lock Screen, центрированный Liquid Glass модал `ShellOverlayView::Polkit`, эксклюзивный захват клавиатуры/указателя/жестов, секцию `[security]` в [`UserConfig`](../../crates/rusty-compositor/src/config.rs) (`require_password_on_lock`, `lock_on_sleep`, `enable_polkit_agent`) и общий запуск/дренирование в Winit и DRM бэкендах.
- **Вариант 2:** Вынести PAM/Polkit/logind в отдельный внешний бинарник `rustyos-authd` поверх `ext_session_lock_v1` и KDE/GNOME Polkit-агентов. Отклонён как преждевременное усложнение IPC и нарушение визуальной цельности Liquid Glass UX.
- **Вариант 3:** Линковать `libpam.so` напрямую через FFI (`unsafe`). Отклонён из-за политики `#![deny(unsafe_code)]`, несовместимости непривилегированного процесса с чтением `/etc/shadow` и лишних системных сборочных зависимостей.

## Решение

Утверждён **Вариант 1** (`APPROVE: RUSTYOS-056 Вариант 1` от 2026-11-13).

## Последствия

- Lock Screen по умолчанию требует PAM-пароль (`[security].require_password_on_lock = true`), не раскрывая секреты в рендер-состоянии и логах; для тестовых/киоск-режимов проверку можно отключить в `config.toml`.
- Сессия автоматически блокируется перед уходом системы в сон (`PrepareForSleep`) и по `loginctl lock-session`, не задерживая suspend сверх необходимого.
- Самостоятельная сессия RustyOS получает встроенный Polkit-агент без сторонних DE-зависимостей, новых внешних Rust-крейтов и `unsafe`.
