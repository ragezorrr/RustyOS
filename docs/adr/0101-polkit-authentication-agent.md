# ADR-0101: Нативный Safe Rust Polkit Authentication Agent (`RUSTYOS-172`)

- **Статус:** Accepted
- **Дата:** 2026-11-24
- **Решение:** `D-115`
- **Задача:** `RUSTYOS-172` (Системная безопасность: Polkit Agent)

## Контекст

Привилегированные действия в десктопном окружении Linux (запуск приложений с повышенными правами через `pkexec`, дисковые операции в `rusty-installer`, монтирование файловых систем в `rusty-files`, изменение системного времени и параметров сети) контролируются службой `polkitd` (`org.freedesktop.PolicyKit1`).

1. **Отсутствие нативного агента:** До `RUSTYOS-172` в RustyOS не было собственного графического агента аутентификации (`org.freedesktop.PolicyKit1.AuthenticationAgent`). При запросе root-прав `polkitd` возвращал ошибку `NotAuthorized` («No authentication agent found»), либо требовался запуск сторонних GTK/Qt агентов (`polkit-gnome`).
2. **Безопасность и архитектурные границы:** Встраивание диалога аутентификации внутрь процесса композитора (`rusty-compositor`) недопустимо, так как композитор не должен напрямую обрабатывать секретные пароли пользователей и блокироваться на вызовах PAM/сокетов.
3. **Единый визуальный стиль:** Диалог запроса прав должен соответствовать дизайн-системе RustyOS (Liquid Glass / Sheet / Popover, светлая и тёмная темы, поддержка HiDPI, клавиатурная навигация `Enter`/`Escape`).

## Решение

Реализовано самостоятельное приложение `apps/rusty-polkit` в пространстве пользовательской сессии:

1. **Регистрация в Polkit Authority по D-Bus (`zbus = "5"`):**
   - Получение ID сессии пользователя через `org.freedesktop.login1.Manager.GetSessionByPID` на системной шине.
   - Регистрация агента по пути `/org/rustyos/PolicyKit1/AuthenticationAgent` через метод `RegisterAuthenticationAgent` на объекте `/org/freedesktop/PolicyKit1/Authority`.
   - Экспорт интерфейса `org.freedesktop.PolicyKit1.AuthenticationAgent`:
     - `BeginAuthentication(action_id, message, icon_name, details, cookie, identities)`: старт сессии аутентификации.
     - `CancelAuthentication(cookie)`: отмена сессии при отзыве запроса или таймауте.
   - Подписка на сигнал `NameOwnerChanged` для сервиса `org.freedesktop.PolicyKit1` с автоматической перерегистрацией при перезапуске `polkitd`.

2. **PAM-аутентификация через стандартный протокол хелпера:**
   - Подключение к UNIX domain сокету `/run/polkit/agent-helper.socket` (с фоллбэком на спавн бинарника `/usr/lib/polkit-1/polkit-agent-helper-1 <user>`).
   - Передача `cookie` и получение строк `PAM_PROMPT_ECHO_OFF`, `PAM_ERROR_MSG`, `SUCCESS`, `FAILURE`.
   - Защита от подбора пароля (до 3 попыток ввода перед закрытием с ошибкой).
   - При успешной проверке метод `BeginAuthentication` завершается успешно; при отмене возвращается D-Bus ошибка `org.freedesktop.PolicyKit1.Error.Cancelled`.

3. **Графический интерфейс на `rusty-ui`:**
   - Компактный модальный диалог (`400x230 px`, скругление `18 px`, тень `ShadowElevation::Popup`).
   - Векторная иконка замка/щита (`UiGlyphKind::Lock`), имя вызываемого действия/программы, логин пользователя.
   - Поле ввода пароля с маскировкой `••••••••`, автоматическим фокусом и мгновенной реакцией на `Enter` (подтверждение) и `Escape` (отмена).
   - Адаптивная палитра `ThemePalette` (Light / Dark) и индикатор ошибки красным цветом при неверном пароле.

4. **Автозапуск в сессии:**
   - Поставка `/etc/xdg/autostart/rustyos-polkit.desktop` и systemd user unit `rustyos-polkit.service`.

## Последствия и инварианты

- **0 новых зависимостей Cargo, 0 `unsafe`:** используются только существующие в workspace крейты `rusty-ui`, `rusty-services`, `zbus`, `wayland-client`, `libc`.
- **Изоляция:** сбой или зависание PAM-хелпера не влияет на стабильность Wayland-композитора.
- **Совместимость:** полная совместимость с любыми Linux-утилитами (`pkexec`, `systemctl`, `gparted`, `pacman`).
