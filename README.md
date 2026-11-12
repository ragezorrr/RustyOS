# RustyOS --- Gemini / Antigravity Engineering Kit

Полный стартовый комплект документации для разработки RustyOS с **Gemini
в Google Antigravity**.

RustyOS --- самостоятельная Linux Desktop Environment на Rust:
собственный Wayland compositor, window management, renderer, animation
engine и desktop shell. Визуальное направление вдохновлено macOS, но
интерфейс и реализация должны быть оригинальными.

> Архив содержит план, правила агента, skills, workflows и шаблоны. Это
> не готовая ОС и не реализованный compositor.

## Сборка и проверка (Фаза 0 --- Project Foundation)

Текущий Cargo workspace (`MSRV 1.99`, `edition = "2024"`) содержит:
- `crates/rusty-wm` — детерминированная модель управления окнами, рабочими столами (`1..=9`), фокусом, стекингом и раскладками (`Floating`, `Snap`, `MasterStack` tiling).
- `crates/rusty-compositor` — Wayland композитор (Smithay + Winit nested prototype, frame timing instrumentation, интеграция с `rusty-wm`).

```bash
# Проверка форматирования
cargo fmt --all -- --check

# Проверка компиляции workspace
cargo check --workspace

# Статический анализ Clippy
cargo clippy --workspace --all-targets -- -D warnings

# Запуск тестов
cargo test --workspace

# Запуск композитора в режиме bootstrap-проверки
cargo run -p rusty-compositor -- --check

# Запуск nested Wayland композитора (Smithay + Winit, Фаза 1)
cargo run -p rusty-compositor -- --winit

# Автоматизированный smoke-тест с запуском Wayland-клиента на 30 кадров
cargo run -p rusty-compositor -- --winit --max-frames 30 --spawn es2gears_wayland

# Замер frame pacing и экспорт JSON-отчёта (RUSTYOS-003)
cargo run -p rusty-compositor -- --winit --max-frames 120 --spawn es2gears_wayland --metrics-out /tmp/rustyos-frame-metrics.json

# Мультиоконный тест Фазы 2 (RUSTYOS-004 / RUSTYOS-005)
cargo run -p rusty-compositor -- --winit --max-frames 60 --spawn es2gears_wayland --spawn es2gears_wayland

# Аппаратная проверка Native DRM/KMS, GBM, EGL и DMA-BUF (Фаза 3, RUSTYOS-006)
cargo run -p rusty-compositor -- --drm-probe --metrics-out /tmp/rustyos-drm-report.json
```

### Клавиатурные сочетания в композиторе (Фаза 2)
- `Alt + Tab` / `Alt + J` / `Alt + K` — переключение фокуса между окнами активного рабочего стола
- `Alt + 1..9` — переключение активного рабочего стола (`Workspace 1..9`)
- `Alt + Shift + 1..9` — перенос активного окна на рабочий стол `1..9`
- `Alt + L` / `Alt + Space` — переключение режима раскладки (`Floating` <-> `Master-Stack Tiling`)
- `Alt + M` — развернуть / восстановить активное окно (`Maximize`)
- `Alt + Left` / `Alt + Right` / `Alt + C` — быстрая привязка окна (`Left Half`, `Right Half`, `Center`)
- `Alt + O` — перенос активного окна на следующий монитор (`Output`)
- `Alt + W` / `Alt + Q` — закрыть активное окно (`xdg_toplevel::close`)
- `Escape` / `Ctrl + Alt + Backspace` — выход из nested-композитора

Описание проверенного dev-окружения и доступных системных библиотек зафиксировано в `docs/DEVELOPMENT_ENVIRONMENT.md`.

## Быстрый старт

1.  Распакуй архив в корень нового Git-репозитория `RustyOS`.
2.  Открой корневую папку репозитория в Antigravity.
3.  Проверь, что среда обнаруживает `.agents/rules/`, `.agents/skills/`
    и `.agents/workflows/`.
4.  Прочитай `AGENTS.md` и `docs/GETTING_STARTED.md`.
5.  Начинай с `RUSTYOS-001`. Сначала только read-only аудит, затем план
    и согласование.

## Главные документы

  Файл                   Назначение
  ---------------------- --------------------------------------
  `AGENTS.md`            Компактные постоянные правила агента
  `ROADMAP.md`           Фазы разработки и критерии выхода
  `ARCHITECTURE.md`      Архитектура и границы компонентов
  `PERFORMANCE.md`       Плавность, frame pacing и latency
  `HARDWARE.md`          GPU, драйверы и совместимость
  `DESIGN.md`            UX и дизайн-система
  `SECURITY.md`          Безопасность
  `DECISIONS.md`         Принятые решения и открытые вопросы
  `TASKS.md`             Ближайшие задачи
  `docs/`                Запуск, hardware matrix, ADR
  `.agents/rules/`       Правила Antigravity
  `.agents/skills/`      14 специализированных навыков
  `.agents/workflows/`   Повторяемые workflows `/...`

## Принципы проекта

- Пользователь --- главный архитектор RustyOS.
- Агент автономно выполняет задачи в пределах утверждённой архитектуры.
- Существенные архитектурные, технологические, API и системные изменения
  --- только после явного согласования.
- Экономия токенов обязательна, но не за счёт качества, тестирования,
  безопасности и измерений.
- Поддержка GPU/VRR/zero-copy заявляется только после реальных проверок.
- KDE, GNOME и Hyprland не используются как основа RustyOS; изучать их
  реализацию и UX можно.

## Antigravity

Комплект использует корневой `AGENTS.md` и workspace customization в
`.agents/`: - `.agents/rules/*.md` -
`.agents/skills/<skill-name>/SKILL.md` - `.agents/workflows/*.md`

Не дублируй целиком `AGENTS.md` в `GEMINI.md`: повторная загрузка одних
и тех же правил расходует контекст. После распаковки проверь обнаружение
файлов в используемой версии Antigravity.
