# Getting Started --- Gemini in Antigravity

## 1. Подготовка

1.  Распакуй комплект в корень репозитория.
2.  Открой корень репозитория как workspace в Antigravity.
3.  Проверь обнаружение `.agents/rules/`, `.agents/skills/`,
    `.agents/workflows/`.
4.  Не запускай первую сессию из вложенной папки.

## 2. Первый запрос агенту

Скопируй в Antigravity:

> Прочитай AGENTS.md, ROADMAP.md, ARCHITECTURE.md и RUSTYOS-001. Пока
> ничего не меняй. Проведи read-only аудит workspace и dev environment:
> ОС, kernel, Rust/Cargo, Git status, доступные graphics libraries и
> ограничения. Используй короткие целевые команды, не выводи большие
> файлы целиком. Затем предложи минимальный план Project Bootstrap,
> перечисли файлы, команды проверок и риски. Не устанавливай пакеты и не
> меняй системные настройки без моего явного разрешения. Дождись
> подтверждения перед внесением изменений.

## 3. Согласование

Используй точные ответы: - `APPROVE: <вариант>` --- разрешить конкретное
предложение. - `REJECT` --- отклонить. - `REVISE: <уточнение>` ---
запросить новый вариант.

Approval относится только к описанному объёму. Для нового существенного
изменения нужно новое согласование.

## 4. После bootstrap

Попроси выполнить RUSTYOS-002. Перед сменой graphics stack, IPC, process
boundaries или крупными зависимостями требуй ADR proposal и approval.

## 5. Установка и обкатка на чистом Arch Linux (`RUSTYOS-076`..`RUSTYOS-078`)

1. **Предварительная проверка окружения (без прав root и без изменений `/usr`):**

   ```bash
   ./bin/rustyos-session --doctor
   ```

2. **Сборка и установка Arch-пакета (`rustyos-git`) или локальная установка:**

   ```bash
   # Вариант A (штатный пакет Arch через pacman, легко откатывается `sudo pacman -Rns rustyos-git`):
   cd packaging/arch && makepkg -si

   # Вариант B (ручная установка в /usr/local + /usr/lib/rustyos для pkexec):
   ./scripts/install.sh --system
   ```

   Для тестовой сборки `.pkg.tar.zst` без установки в систему хоста задай изолированные `BUILDDIR`/`PKGDEST` и `--holdver`.

3. **Проверка реального DRM/KMS (TTY) и `pkexec` после установки пакета:**
   - Выйди в чистую TTY (`Ctrl + Alt + F3`), проверь готовность `rustyos-session --doctor` и запусти `rustyos-session --drm` (или выбери сессию **RustyOS** в `SDDM`/`greetd`).
   - В `--drm` композитор сам публикует `WAYLAND_DISPLAY` и `DISPLAY` (`XWayland`) в `dbus-update-activation-environment --systemd` после старта; в nested `--winit` этот экспорт отключён.
   - Для живой проверки `rusty-updater` открой меню `TopBar -> Check for Updates... -> Open Updater...` (либо запусти `rusty-updater --dry-run` для безопасной симуляции без записи в `/var/lib/pacman`).
   - После живой проверки `--drm` на целевом GPU занеси фактический результат (модель GPU, ядро, драйвер, разрешение/Гц, курсор, suspend/resume) в `docs/HARDWARE_MATRIX.md`.
