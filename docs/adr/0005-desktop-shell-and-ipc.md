# ADR-0005: Desktop Shell Process Boundary & Versioned Local IPC (`crates/rusty-ipc` + `apps/rusty-shell`)

- **Статус:** accepted
- **Дата:** 2026-11-12
- **Автор:** AI Agent + User

## Контекст

Согласно `ROADMAP.md` (Фаза 6 --- Desktop Shell), `ARCHITECTURE.md` (раздел 2, Shell) и `SECURITY.md`:
- Оболочка рабочего стола (`Shell`) отвечает за top bar / system status, dock / task switcher, app launcher, notifications, quick settings, workspace overview, lock screen и wallpaper management.
- `Shell` **обязана иметь независимый жизненный цикл (lifecycle)**: падение, зависание или перезапуск `rusty-shell` не должны завершать или блокировать `rusty-compositor`.
- `Shell` и `Compositor` должны общаться через **версионируемый локальный IPC**, с валидацией входных данных, ограничением размера сообщений (защита от OOM/DoS) и проверкой прав доступа к сокету (`0600` в `$XDG_RUNTIME_DIR`).

Поскольку введение новой межпроцессной границы (`apps/rusty-shell`), нового IPC-протокола (`crates/rusty-ipc`) и выбор механизма рендеринга/представления оболочки входят в список **Approval Gates**, требуется явное архитектурное решение.

## Рассмотренные варианты

### Вариант A: Выделенный крейт `crates/rusty-ipc` (Unix Domain Socket + line-delimited versioned protocol) + отдельный процесс `apps/rusty-shell` (Рекомендуемый)

1. **`crates/rusty-ipc` (`std`-only, `unsafe_code = "deny"`, 0 внешних зависимостей):**
   - **Транспорт:** неблокирующий `UnixListener` / `UnixStream` в `$XDG_RUNTIME_DIR/rustyos-ipc-<wayland_socket>.sock` с правами доступа только для текущего пользователя (`0600` через `std::os::unix::fs::PermissionsExt`).
   - **Версионирование и защита:** каждое сообщение содержит `protocol_version: u16` (`IPC_PROTOCOL_VERSION = 1`), жёсткий лимит длины строки (`MAX_IPC_MESSAGE_BYTES = 8192`) и детерминированный текстовый/JSON-совместимый парсер без сторонних макросов.
   - **Сообщения `ShellRequest` (Shell -> Compositor):**
     - `Handshake { protocol_version, client_name }`
     - `GetStateSnapshot`
     - `SwitchWorkspace { workspace: u8 }`
     - `FocusWindow { window_id: u64 }`
     - `ToggleLayoutMode`
     - `ToggleOverview`
     - `SetReducedMotion { enabled: bool }`
     - `SetLockScreen { locked: bool }`
     - `Notify { title, body, urgency }`
   - **Сообщения `CompositorEvent` (Compositor -> Shell):**
     - `HandshakeAck { protocol_version, server_version }`
     - `StateSnapshot(DesktopStateSnapshot)` — полный снимок состояния: активный `workspace` (`1..=9`), режим раскладки (`Floating`/`MasterStack`), список открытых окон (`id`, `workspace`, `focused`, `maximized`), статус `reduced_motion`, `overview_active`, `screen_locked`, количество активных уведомлений и метрики FPS/frame interval (`p50/p95`).
     - `Error { code, message }`
2. **Интеграция IPC-сервера в `crates/rusty-compositor` (`RUSTYOS-009`):**
   - Композитор создаёт неблокирующий IPC-сокет рядом с Wayland-сокетом и опрашивает клиентов без блокировки event loop.
   - Разрыв соединения или перезапуск `rusty-shell` обрабатывается штатно (`Disconnected` -> очистка клиентского слота без остановки композитора); новое подключение `rusty-shell` сразу получает актуальный `StateSnapshot`.
3. **Независимое приложение `apps/rusty-shell` (`RUSTYOS-010`):**
   - Самостоятельный процесс и библиотека модели оболочки, включающая все компоненты Фазы 6 с дизайн-токенами из `DESIGN.md` (`#0F0A19`, `#161023`, `#A56EFF`, `#F4F0FA`):
     - `TopBarState` (активный рабочий стол `1..=9`, раскладка, статус сети/питания/звука, часы, FPS/latency индикатор);
     - `DockState` (закреплённые приложения + живой список окон из `StateSnapshot` с переключением фокуса);
     - `AppLauncher` (поиск и фильтрация приложений + безопасный неблокирующий спавн);
     - `NotificationCenter` (очередь уведомлений с уровнями `Low`/`Normal`/`Critical` и авто-истечением по монотонному таймеру);
     - `QuickSettingsState` (громкость, Wi-Fi, Bluetooth, `Reduced Motion`, `Dark Mode`);
     - `WorkspaceOverview` и `LockScreenState`, плюс `WallpaperConfig`.
   - Поддерживает режимы:
     - `--ipc-socket <PATH>` / `--once`: подключение к живому `rusty-compositor`, синхронизация `StateSnapshot`, отправка команд (`--switch-workspace`, `--toggle-overview`, `--notify`) и проверка выживаемости композитора при перезапуске `rusty-shell`;
     - `--status-hud`: вывод отрендеренной структуры TopBar / Dock / QuickSettings / Notifications.

- **Плюсы:**
  - Истинная изоляция процессов: падение или перезапуск `apps/rusty-shell` (`kill` + повторный запуск) никак не влияет на `rusty-compositor`.
  - Версионированный IPC (`v1`) с лимитом буфера (`8 KiB`) и правами `0600` строго выполняет требования `SECURITY.md`.
  - Нулевые внешние зависимости (`std`-only), мгновенная сборка в `--offline` песочнице.
- **Минусы:**
  - До выбора тяжёлого GPU UI-тулкита (в будущих фазах) визуальное представление `rusty-shell` работает через детерминированную модель сцены/HUD и IPC-клиент.

### Вариант B: Встроить `Shell` внутрь процесса `rusty-compositor` без IPC

- **Плюсы:** Проще передавать состояние напрямую в памяти.
- **Минусы:** Прямо нарушает `ARCHITECTURE.md` («Shell должен иметь независимый lifecycle и не завершать compositor при собственном падении») и критерии выхода Фазы 6 в `ROADMAP.md` («shell можно перезапустить без перезапуска compositor; IPC документирован и версионируется»).

### Вариант C: Использовать D-Bus (`zbus`) или `serde_json` + тяжелый GUI-тулкит (`iced` / `gtk4`) прямо сейчас

- **Плюсы:** Готовая сериализация.
- **Минусы:** Требует десятков новых внешних крейтов, отсутствующих в локальном offline-кэше Cargo, и сильно раздувает дерево зависимостей.

## Решение

Выбран **Вариант A**: создать крейт `crates/rusty-ipc` и приложение `apps/rusty-shell`, интегрировать неблокирующий IPC-сервер в `crates/rusty-compositor` и проверить независимый перезапуск `rusty-shell` при работающем композиторе (`RUSTYOS-009` и `RUSTYOS-010`).

## Последствия

- Выполняются все критерии выхода Фазы 6 (`ROADMAP.md`).
- В workspace добавляются `crates/rusty-ipc` и `apps/rusty-shell` (в точном соответствии со структурой `ARCHITECTURE.md`).
