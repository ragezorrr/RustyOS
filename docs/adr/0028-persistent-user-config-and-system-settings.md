# ADR-0028: Сериализация `UserConfig`, live config reload и встроенная панель System Settings

- **Статус:** accepted
- **Дата:** 2026-11-14
- **Автор:** AI Agent + User
- **Связанные задачи:** RUSTYOS-061 (развивает ADR-0017, ADR-0022, ADR-0024, ADR-0027)

## Контекст

1. Модуль `crates/rusty-compositor/src/config.rs` умел только загружать и парсить `~/.config/rustyos/config.toml` на старте сессии, но не поддерживал сериализацию `UserConfig` обратно в TOML и атомарное сохранение при изменении параметров во время работы.
2. Изменения визуальных/UI параметров (`glass_opacity`, `reduce_transparency`, `reduced_motion`, индикаторов Menu Bar), конфигурации дисплеев (`scale`, `rotation`, `vrr_mode`, `prefer_max_refresh`) и политик безопасности (`require_password_on_lock`, `lock_on_sleep`, `enable_polkit_agent`) не сохранялись между запусками и требовали ручного перезапуска композитора при правке `config.toml`.
3. В проекте отсутствовал единый встроенный интерфейс настроек для интерактивного управления всеми ключевыми секциями `UserConfig` внутри композиторного shell без привлечения сторонних GTK/Qt-панелей.

## Решение

1. **Сериализация и атомарное сохранение `UserConfig` (`crates/rusty-compositor/src/config.rs`):**
   - Реализованы `VrrMode::as_str`, `escape_toml_string`, `UserConfig::to_toml_string` и `UserConfig::save_to_path`.
   - Сохранение выполняется атомарно через временный файл `.config.toml.tmp.<pid>` в целевой директории с созданием родительских каталогов и `fs::rename`.
   - Парсер `UserConfig::parse_toml` сохраняет исходный регистр имени коннектора в заголовках `[display.outputs."<NAME>"]` (при сохранении регистронезависимого префикса и поиска через `lookup_per_output`), обеспечивая точный roundtrip всех секций `[keyboard]`, `[ui]`, `[display]`, `[display.outputs."..."]`, `[security]` и `[autostart]`.
   - На `RustyCompositorState` добавлено поле `config_path: Option<PathBuf>` (`None` в unit-тестах для изоляции от реального `$HOME`; в `winit_backend.rs` и `drm_backend.rs` инициализируется `default_user_config_path()`).

2. **Горячая перезагрузка конфигурации (`ShellRequest::ReloadConfig`, Apple/System menu и хоткей):**
   - В `crates/rusty-ipc/src/lib.rs` добавлен запрос `ShellRequest::ReloadConfig` (`RELOAD_CONFIG`), обрабатываемый в `state/wm_windows.rs`.
   - В `state/shell_actions.rs` добавлены `apply_user_config`, `reload_user_config` и `sync_and_save_user_config`: при перезагрузке обновляются параметры анимаций и стекла/прозрачности, видимость элементов Menu Bar, `scale` и `rotation` подключённых outputs (`update_output_geometry` / `set_output_rotation` с корректным флагом FBO-ориентации для `winit-0` vs DRM), политики VRR/безопасности, а также `keyboard.change_repeat_info(rate, delay)` и `keyboard.set_xkb_config` при изменении XKB-полей.
   - В меню `MENU_SYSTEM` добавлены пункты `System Settings...` (`Super+,`) и `Reload Config` (`Super+Shift+R`) с действиями `TopBarMenuAction::{OpenSettings, ReloadConfig}` и клавиатурными перехватами `KeyIntercept::{ToggleSettings, SettingsClose, SettingsNextTab, SettingsPrevTab, ReloadConfig}`.

3. **Встроенная панель `System Settings` (`ShellOverlayView::Settings`):**
   - В `crates/rusty-compositor/src/shell_render/modals.rs` реализовано центрированное Liquid Glass окно настроек с боковым меню из четырёх вкладок (`SettingsTab::{Appearance, MenuBar, Displays, Security}`), управлением `SettingsHit`, кнопками `Reload TOML` и `Done`/`×`, и индикатором статуса сохранения/перезагрузки.
   - Оверлей `ShellOverlayView::Settings` интегрирован в `shell_render/mod.rs` и `frame_composer.rs` в слой `foreground_overlay` с учётом в `has_blocking_shell_overlay`, `should_hide_shell_chrome_for_fullscreen_on` и `active_unoccluded_fullscreen_window_on`, а pointer-события маршрутизируются через `input/pointer.rs` и `input/shell_pointer.rs` в логической геометрии вывода.

## Последствия

- Изменения параметров в Control Center, Menu Bar, IPC и System Settings мгновенно применяются и автоматически сохраняются в `~/.config/rustyos/config.toml` без блокировки event loop тяжёлыми GUI-фреймворками.
- Внешние правки `~/.config/rustyos/config.toml` можно применить без перезапуска композитора и потери клиентских окон через `Super+Shift+R`, System Menu → `Reload Config`, кнопку `Reload TOML` в System Settings или IPC-команду `RELOAD_CONFIG`.
- Новые внешние зависимости и `unsafe`-код не вводились (`unsafe_code = "deny"` сохранён по всему workspace).
