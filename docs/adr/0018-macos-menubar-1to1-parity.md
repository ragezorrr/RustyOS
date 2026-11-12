# ADR-0018: 1:1 macOS Menu Bar Visual Parity, Bundled Inter Typography & Minimal Bar Configuration (`RUSTYOS-049`)

- **Статус:** accepted
- **Дата:** 2026-11-13
- **Автор:** AI Agent + User

## Контекст

После `RUSTYOS-048` визуальное сравнение живого `TopBar` со скриншотом macOS Menu Bar выявило оставшиеся расхождения:
1. Верхняя полоса отрисовывала трёхступенчатый тёмный скрайм (`alpha = 118/76/36`) поверх обоев вместо полностью прозрачного macOS-бара поверх картинки.
2. Полоса загружала разные кегли (`12.5` / `11.5` / `10.5`) в одном Regular-начертании без единой базовой линии; системного шрифта Inter на хосте не было, эмблема RustyOS была фиолетовой вместо чисто-белой монохромной, а порядок правых глифов (`tray · kbd · Search · Wi-Fi · Volume · Battery · Control Center · Clock`) и локализованный русский формат даты (`пт 13 ноя  00:52`) визуально перегружали панель.
3. Индикаторы воркспейсов `1..=9`, `Float`/`Tile`, SNI tray, иконка `Volume` и бейдж `EN` занимали середину и правую часть полосы по умолчанию.
4. Отдельный слойный клиент `apps/rusty-shell/src/wayland.rs` по-прежнему рисовал непрозрачную панель высотой `32 px` с плитками воркспейсов и капсулой статуса справа.

## Рассмотренные варианты

- **Вариант A (Выбран):**
  - Встроить статические шрифты **Inter** (`Inter-Regular.ttf`, `Inter-SemiBold.ttf`, лицензия OFL-1.1) в `assets/fonts/` через `include_bytes!` (без новых Rust-зависимостей и без `unsafe`), добавить поддержку веса `FontWeight::{Regular, Bold}` в `crates/rusty-compositor/src/text.rs` с общей оптической базовой линией по регулярному начертанию, единым кеглем `13.0 px` для активного приложения (`Bold`), глобальных меню (`File`, `Edit`, `View`, `Go`, `Window`, `Help`) и часов (`Regular`), а также деликатным 1 px текстовым/иконочным drop-shadow при `reduce_transparency = false`.
  - В `crates/rusty-compositor/src/shell_render.rs` уменьшить высоту бара до `TOP_BAR_HEIGHT = 24`, полностью убрать тёмный скрайм при `reduce_transparency = false` (оставляя сплошной высококонтрастный фон только при `reduce_transparency = true`), перекрасить оригинальную монохромную шестиугольную эмблему `RustyLogo` в чистый белый `#FCFAFF` и выстроить правый кластер в точном порядке macOS (`[Battery] · Wi-Fi · Search · Control Center · Clock`).
  - В `crates/rusty-compositor/src/surfaces.rs` повысить суперсэмплинг UI-глифов до `4x4` (16 выборок на пиксель), обновить геометрию `RustyLogo`, скруглённые дуги `Wi-Fi` (`sd_arc_up`), монохромную капсулу `Battery` (красный при `<= 20%`), яркую белую лупу `Search` и двойные переключатели `Control Center`.
  - В `crates/rusty-compositor/src/config.rs`, `state.rs`, `input.rs` и `winit_backend.rs` скрыть по умолчанию полосу `1..=9`/`Float`/`Tile`, SNI tray, `EN` и `Volume` (`menubar_show_workspaces`, `menubar_show_tray`, `menubar_show_input_source`, `menubar_show_volume`, `clock_24h` = `false` в `[ui]` `config.toml`), сохранив доступ к воркспейсам и раскладке через меню `Go`/`Window` (`NextWorkspace`, `PrevWorkspace`, `ToggleLayout`, `ShowShortcuts`), Overview и Control Center.
  - В `crates/rusty-services/src/lib.rs` и `telemetry.rs` перевести часы на нейтральный `LC_ALL=C` macOS-формат (`"Wed Apr 1  9:41 AM"` и опциональный 24h `"Wed Apr 1  09:41"`), а в `apps/rusty-shell/src/wayland.rs` синхронизировать `24 px` прозрачный рендер Menu Bar и встроенный Inter.
- **Вариант B:** Оставить системный шрифт хоста (DejaVu/Adwaita), скрыв только скрайм и `1..9`: приводило бы к расхождению пропорций, обрезанным контурам `2x2` глифов и отсутствию единой базовой линии между жирным и обычным начертанием.
- **Вариант C:** Подключить `harfbuzz`/`pango` + рендер Menu Bar в отдельном Vulkan/Skia процессе: избыточные C-зависимости и нарушение принятых границ `RUSTYOS-048`.

## Решение

Утверждён **Вариант A** (`APPROVE: RUSTYOS-049 Option A` + требование использования шрифта Inter от 2026-11-13).

## Последствия

- `TopBar` визуально соответствует чистому прозрачному macOS Menu Bar `24 px` с оригинальной монохромной эмблемой RustyOS и без сторонних брендовых ресурсов Apple.
- Шрифт Inter (`OFL-1.1`) встроен непосредственно в бинарные артефакты `rusty-compositor` и `rusty-shell`, гарантируя идентичную типографику (`Latin` + `Cyrillic`) независимо от установленных в системе шрифтов и `CWD`.
- Показатели воркспейсов, SNI tray, раскладка клавиатуры, громкость и 24h-время полностью настраиваются через секцию `[ui]` в `config.toml`.
