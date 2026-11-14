# ADR-0032: Переход с темы Purple Glass на двойную цветовую схему Golden Gate (Light & Dark)

- **Статус:** accepted
- **Дата:** 2026-11-14
- **Задача:** `RUSTYOS-067`
- **Связанные решения:** `D-040`, `ADR-0023`, `ADR-0028`, `ADR-0030`

## Контекст

Изначальная тема `RustyOS Purple Glass` (`#0F0A19`, `#161023`, `#20182E`, `#292037`, акцент `#A56EFF`) расходилась с визуальными референсами `docs/design/references/` и `docs/design/references/macos-27-golden-gate/` (`context-menu.jpeg`, `desktop.jpeg`, `finder-window-dark.jpeg`, `27-Golden-Gate-Apperance-Light-Automatic.png`, `27-Golden-Gate-Apperance-Dark-Automatic.png`), а контекстные меню, выпадающие меню `TopBar`, панели оболочки и тёмная тема `rusty-files` не переключались согласованно между светлой и тёмной темами.

## Решение

1. **Полный отказ от `Purple Glass`:**
   - Фиолетовые базовые и акцентные константы в `crates/rusty-compositor`, `apps/rusty-files`, `apps/rusty-shell` и `DESIGN.md` заменены на нейтральную графитовую и светлую палитру `Golden Gate` с системным синим акцентом (`#007AFF` в Light и `#0A84FF` в Dark).
2. **`AppearanceMode::{Light, Dark}` в `UserConfig` и `System Settings`:**
   - В `crates/rusty-compositor/src/config.rs` добавлен `AppearanceMode::{Light, Dark}` (по умолчанию `Light`), сериализуемый как `theme = "light" | "dark"` в секции `[ui]` файла `~/.config/rustyos/config.toml`.
   - Во вкладку `System Settings -> Appearance` (`modals.rs`) добавлена строка `Appearance Theme` (`Light` / `Dark`, `SettingsHit::ToggleAppearance`), мгновенно переключающая тему оболочки и сохраняющая настройку на диск.
3. **Темизация стеклянных поверхностей, меню и обоев (`ThemePalette`, `GlassPanelKey`):**
   - В `crates/rusty-compositor/src/shell_render/tokens.rs` введена структура `ThemePalette::{current_for, active}` и активный режим кадра `set_active_appearance`.
   - В `UiShapeCache` ключ `GlassPanelKey` расширен полем `is_dark: bool`; `sample_glass_panel_pixel_themed` генерирует матовое белое стекло (`#F5F6F8`, alpha ~`0.86`) для `Light` и матовое нейтрально-графитовое стекло (`#262629`, alpha ~`0.82`) для `Dark`.
   - Контекстные меню (`context_menu.rs`), выпадающие меню `TopBar` (`shell_render/mod.rs`), `Control Center`, `Spotlight`, `Notification HUD`, `System Settings`, `Polkit`/`Portal` и `SSD` заголовки окон используют `ThemePalette::active()`, обеспечивая тёмный текст на светлом стекле + синюю пилюлю ховера `#007AFF` с белым текстом/иконкой в `Light` и светлый текст на тёмном графите + `#0A84FF` в `Dark`.
   - Процедурные обои `sample_wallpaper_pixel_themed` переведены на палитру шёлковой ленты `Golden Gate` (`desktop.jpeg`: тёплый золотисто-песочный и прибрежный сине-сланцевый градиент в дневном и ночном вариантах).
4. **Синхронизация `apps/rusty-files`:**
   - Тёмная палитра `apps/rusty-files/src/render.rs` переведена на нейтральный графит `Golden Gate Dark` (`#1E1E20` окно, `#26262A` сайдбар/попапы, `#0A84FF` акцент) по референсу `finder-window-dark.jpeg`.
   - При запуске без флагов `--light`/`--dark` `rusty-files` автоматически считывает `[ui] theme` из `~/.config/rustyos/config.toml` (`UserFileLocations::detect_system_theme`).

## Последствия

- Единый визуальный язык `Golden Gate Light & Dark` во всех компонентах композитора и `rusty-files` без следов старой фиолетовой палитры.
- Нулевые накладные расходы в steady-state рендере: стеклянные панели и обои для `Light` и `Dark` кэшируются в `UiShapeCache`.
