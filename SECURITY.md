# RustyOS --- Security Principles

## Общие правила

- Least privilege. Не запускать shell/compositor от root без
  необходимости.
- Не обходить Polkit и штатные механизмы authorization.
- Не хранить пароли, tokens и private keys в репозитории, fixtures и
  логах.
- Валидировать недоверенный ввод и IPC.
- Проверять peer credentials и permissions для local IPC, где применимо.
- Документировать версии IPC и error behavior.
- Изолировать optional integrations.
- Минимизировать `unsafe` и dependency surface.
- Проверять происхождение, лицензии и maintenance status зависимостей.

## System changes

Любые изменения systemd units, udev rules, display manager, bootloader,
kernel params, device permissions, privileged helpers или signing/update
infrastructure требуют: 1. предварительного approval; 2. точного списка
изменений; 3. оценки риска; 4. плана отката.

## Security issue reporting

Описать impact, затронутые компоненты и mitigation. Не скрывать проблему
ради успешного CI и не включать секреты в отчёт.
