# ADR-0047: Модульная декомпозиция подсистемы модальных диалогов crates/rusty-compositor/src/shell_render/modals по стандартам CutefishOS и macOS 27

- **Статус:** accepted
- **Дата:** 2026-11-17
- **Задача:** `RUSTYOS-091`
- **Связанные решения:** `D-055`, `D-054`, `D-053`, `D-052`, `D-051`, `D-050`, `D-030`, `ADR-0046`, `ADR-0045`, `ADR-0044`, `ADR-0043`, `ADR-0042`

## Контекст

1. **Разрастание монолита `modals.rs`:**
   В подсистеме отрисовки композитора `crates/rusty-compositor/src/shell_render/modals.rs` разросся до 1,994 строк кода, объединяя четыре совершенно разные подсистемы системного интерфейса:
   - Аутентификационный диалог PolicyKit (запрос прав root/sudo, анимация проверки, маскирование пароля, баннеры ошибок PAM);
   - Диалог согласия XDG Desktop Portal (запрос доступа к экранам и окнам при захвате ScreenCast / Screenshot, тумблер курсора);
   - Увеличительная лупа и HUD-панель пипетки `PickColor` (8x сетка субпикселей, прицел, плавающая информационная пилюля);
   - Многостраничный лист системных настроек System Settings (6 категорийных вкладок: Appearance, Menu Bar, Displays, Keyboard, Security, Desktop & Dock; интерактивные переключатели, степперы параметров и чтение/запись `config.toml`).

2. **Опыт CutefishOS и стандарты macOS 27:**
   В CutefishOS и macOS системные диалоги и настройки изолированы по независимым компонентам (`polkit`, `portal`, `settings`, `magnifier`), каждый из которых инкапсулирует свою внутреннюю геометрию раскладки и хит-тестинг.

3. **Требование:**
   - Декомпозировать `crates/rusty-compositor/src/shell_render/modals.rs` в модульный пакет `crates/rusty-compositor/src/shell_render/modals/` (`polkit.rs`, `portal.rs`, `magnifier.rs`, `settings.rs`, `mod.rs`);
   - Обеспечить 100% обратную совместимость всех публичных структур, перечислений и функций;
   - Сохранить прохождение 100% тестов рабочего пространства (244/244), нулевой уровень предупреждений `cargo clippy` (`-D warnings`) и чистоту `cargo fmt`.

## Решение

Монолитный файл `crates/rusty-compositor/src/shell_render/modals.rs` упразднён и заменён на модульный пакет `crates/rusty-compositor/src/shell_render/modals/`:

1. **`polkit.rs` (408 строк):**
   - Структуры: `PolkitPromptRenderState`, `PolkitHit`, `PolkitModalLayout`;
   - Геометрия и хит-тестинг: `polkit_modal_layout`, `hit_test_polkit_dialog`;
   - Текстовый лейаут и обёртка сообщений: `wrap_polkit_two_lines`;
   - Отрисовка текста и Liquid Glass композита: `render_polkit_text`, `render_polkit_header_text`, `render_polkit_controls_text`, `render_polkit_composite`.

2. **`portal.rs` (442 строки):**
   - Структуры: `PortalSourceRow`, `PortalPromptRenderState`, `PortalHit`, `PortalModalLayout`, константа `PORTAL_MAX_VISIBLE_ROWS`;
   - Геометрия списка и хит-тестинг: `portal_visible_range`, `portal_modal_layout`, `hit_test_portal_dialog`;
   - Отрисовка: `render_portal_text`, `render_portal_footer_text`, `render_portal_composite`.

3. **`magnifier.rs` (73 строки):**
   - Геометрия плавающей пилюли: `pick_color_pill_rect`;
   - Отрисовка текста и Liquid Glass оверлея: `render_pick_color_text`, `render_pick_color_composite`.

4. **`settings.rs` (1,104 строки):**
   - Перечисления и модели состояния: `SettingsTab`, `SettingsRenderState`, `SettingsHit`, `SettingsRowKind`, `SettingsRowEntry`;
   - Геометрия окна и контролов: `settings_modal_layout`, `settings_row_pill_rect`, `settings_row_stepper_rects`;
   - Спецификация строк категорий: `settings_tab_rows`, `settings_dock_rows`, `settings_appearance_rows`, `settings_display_rows`, `settings_keyboard_rows`;
   - Хит-тестинг кликов: `hit_test_settings_modal`;
   - Отрисовка текста и композита Liquid Glass: `render_settings_text`, `render_settings_rows_text`, `render_settings_footer_text`, `render_settings_composite`, `render_settings_controls_composite`.

5. **`mod.rs` (28 строк):**
   - Общий вспомогательный метод `phys_to_logical_rect`;
   - Объявление внутренних модулей `pub(crate) mod magnifier; pub(crate) mod polkit; pub(crate) mod portal; pub(crate) mod settings;`;
   - Полный реэкспорт наружу всех публичных контрактов.

## Последствия

- Последний крупный монолит в подсистеме отрисовки композитора (1,994 строки) декомпозирован на 4 сфокусированных файла.
- Код полностью изолирован по доменным областям (безопасность, порталы, настройки, пипетка).
- Все 244 unit-теста рабочего пространства проходят успешно (`cargo test --workspace`).
- Код соответствует строгим правилам `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt`.
