# ADR-0064: GPU-ускоренный Wayland CSD-терминал `rusty-terminal` (`RUSTYOS-113`)

## Status

accepted

## Context

`RustyOS` до `RUSTYOS-113` использовал для терминала внешние эмуляторы (`foot` / `kitty` / `alacritty`), что создавало три архитектурных и UX-пробела:

1. **Отсутствие собственного первого CSD-терминала в стиле macOS Tahoe:** сторонние терминалы не поддерживают единый протокол размытого backdrop `rustyos_liquid_glass_v1` (`Clear Light` / `Clear Dark`), системные `14 px` углы, `14 px` traffic lights, живую синхронизацию `config.toml` (`theme`, `accent_index`, `reduce_transparency`, `reduced_motion`, `glass_opacity`) и унифицированное поведение вкладок/поиска.
2. **Необходимость GPU-пайплайна с zero-copy представлением без раздувания зависимостей:** все существующие приложения `RustyOS` (`rusty-files`, `rusty-settings`, `rusty-updater`) рисуются CPU в `wl_shm`. Терминал с интенсивной прокруткой логов и TrueColor-графика нуждается в GPU-ускорении (`Wayland-EGL` + `OpenGL ES 2.0` + `DMA-BUF` на композиторе), но без подключения тяжёлых новых внешних крейтов вне уже зафиксированного `Cargo.lock`.
3. **Плавный UX и fallback для headless/legacy сред:** на системах или тестах без рабочего `libEGL.so.1` клиент должен автоматически переходить на программный `wl_shm` рендерер с идентичной геометрией и типографикой.

## Decision

Создать самостоятельный крейт [apps/rusty-terminal](file:///home/anc/RustyOS_Main/apps/rusty-terminal/Cargo.toml) (`app_id = "org.rustyos.Terminal"`) без добавления новых прямых внешних зависимостей (используются крейты workspace/графа: `wayland-egl`, `gl_generator`, `smithay-client-toolkit` с фичей `xkbcommon`, подтягивающей транзитивные версии `xkbcommon 0.7.0` / `memmap2 0.8.0`, `rustix`, `fontdue`, `rusty-animation`, `wayland-scanner`):

1. **GPU-рендерер на `wayland-egl` + `gl_generator` (`GLES 2.0`) и `wl_shm` fallback:**
   - В [build.rs](file:///home/anc/RustyOS_Main/apps/rusty-terminal/build.rs) на этапе сборки генерируются биндинги `EGL 1.5` и `GLES 2.0` (`gl_generator`), а в [gpu_egl.rs](file:///home/anc/RustyOS_Main/apps/rusty-terminal/src/gpu_egl.rs) изолирован безопасный слой `GpuRenderer` (динамическая загрузка `libEGL.so.1`, `WlEglSurface`, строгий порядок освобождения `eglDestroySurface` до `wl_surface`).
   - 2-pass batched-конвейер по архитектуре Alacritty GLES2: `RECT_PROG` (аналитические SDF-прямоугольники с border/радиусом/кругами для окна, заголовка, вкладок, ячеек, выделения, поиска, курсора и скроллбара) + `TEXT_PROG` с динамическим `1024×1024` одноканальным атласом глифов (`ShelfPacker` с `1 px` отступами и `glScissor`-клиппингом области сетки).
   - При отсутствии `libEGL.so.1` или флаге `--no-gpu` в [renderer_shm.rs](file:///home/anc/RustyOS_Main/apps/rusty-terminal/src/renderer_shm.rs) включается эквивалентный premultiplied `ARGB8888` CPU fallback `ShmFallbackRenderer`.

2. **Шрифты, процедурная псевдографика и тема `macOS Tahoe`:**
   - В [font.rs](file:///home/anc/RustyOS_Main/apps/rusty-terminal/src/font.rs) модуль `FontSet` загружает системные моноширинные шрифты (`Adwaita Mono`, `Hack`, `JetBrains Mono`, `DejaVu Sans Mono`, `Liberation Mono`, `fc-match monospace`) вместе со встроенным `Inter` для UI-хрома и процедурно растеризует без швов `Box Drawing` (`U+2500..U+257F`), `Block Elements` (`U+2580..U+259F`) и `Powerline` (`U+E0B0..U+E0B3`).
   - В [theme.rs](file:///home/anc/RustyOS_Main/apps/rusty-terminal/src/theme.rs) и [liquid_glass.rs](file:///home/anc/RustyOS_Main/apps/rusty-terminal/src/liquid_glass.rs) реализованы палитры `Clear Dark` / `Clear Light` + непрозрачный режим при `reduce_transparency = true` (через `rustyos_liquid_glass_v1`, `GlassStyle::Sidebar`, `r = 14`), Golden Gate акценты (`#007AFF` / `#0A84FF`) и горячая перезагрузка `config.toml` (`ConfigWatcher`).

3. **Собственный `VT`-парсер, неблокирующий `PTY` и UX:**
   - В [vt.rs](file:///home/anc/RustyOS_Main/apps/rusty-terminal/src/vt.rs) реализован `TerminalEngine`: потоковый UTF-8, `16`/`256`/24-bit `TrueColor` SGR (`;` и `:`), `DECSTBM` регионы, alternate screen (`?47`/`?1047`/`?1049`), `DECCKM`, `?2004` bracketed paste, `?1004` focus reporting, `SGR` mouse (`?1000`/`?1002`/`?1006`), `OSC 0/2/7`, `DSR`/`DA1`, кольцевой scrollback на `10 000` строк.
   - В [pty.rs](file:///home/anc/RustyOS_Main/apps/rusty-terminal/src/pty.rs) модуль `PtySession` открывает POSIX PTY через `rustix::pty` + `tcsetwinsize`, настраивает `setsid` + `TIOCSCTTY`, обслуживает чтение/крупные вставки в фоновом потоке `poll` и будит цикл `calloop` через `Ping`.
   - В [state.rs](file:///home/anc/RustyOS_Main/apps/rusty-terminal/src/state.rs), [layout.rs](file:///home/anc/RustyOS_Main/apps/rusty-terminal/src/layout.rs) и [wayland.rs](file:///home/anc/RustyOS_Main/apps/rusty-terminal/src/wayland.rs) реализованы вкладки (`Ctrl+Shift+T`/`W`), плавная прокрутка (`rusty_animation::AnimatedValue`), поиск по истории (`Ctrl+Shift+F`), масштабирование шрифта (`Ctrl++`/`-`/`0`), выделение (drag / double / triple click) и системный Wayland буфер обмена (`DataDeviceManagerState`).

4. **Интеграция в DE:**
   - Обновлены [launcher.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/launcher.rs), [dock.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/dock.rs), [shell_actions.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/state/shell_actions.rs) (`launch_terminal_in_dir`), [actions.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/context_menu/actions.rs) и [apps/rusty-shell/src/lib.rs](file:///home/anc/RustyOS_Main/apps/rusty-shell/src/lib.rs): `rusty-terminal` стал терминалом по умолчанию в `Dock`, `Launcher` и контекстном меню рабочего стола с сохранением fallback на `foot` / `kitty` / `alacritty`.

## Consequences

- `RustyOS` получает нативный, визуально согласованный с macOS Tahoe GPU-терминал без добавления новых прямых внешних зависимостей в `Cargo.toml` (фича `xkbcommon` у `smithay-client-toolkit 0.19.2` добавляет в `Cargo.lock` только транзитивные `xkbcommon 0.7.0` и `memmap2 0.8.0`).
- Поддержан как zero-copy GPU путь (`Wayland-EGL` + `GLES 2.0`), так и детерминированный `wl_shm` CPU fallback.
- Вкладку, тему и `Liquid Glass` можно расширять в рамках уже принятой протокольной архитектуры `rustyos_liquid_glass_v1` без изменений в ядре композитора.
