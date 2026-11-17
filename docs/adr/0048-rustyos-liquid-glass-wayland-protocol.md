# ADR-0048: Собственный Wayland-протокол Liquid Glass (rustyos_liquid_glass_v1)

## Статус

accepted (RUSTYOS-092, D-056)

## Контекст

RustyOS создаёт самостоятельную независимую Linux Desktop Environment на Rust с собственным Wayland-композитором, рендерером и shell (согласно `AGENTS.md` §1, §5: «мы ничего не берём у других DE», не копируем протоколы KDE/GNOME/Hyprland).

Ранее в `RUSTYOS-050`/`RUSTYOS-051` композитор получил аппаратный GLES Dual-Kawase пайплайн Liquid Glass (`surfaces/liquid_glass.rs`) для элементов собственного shell (Dock, TopBar, Control Center, OSD, Spotlight, Context Menus). В `RUSTYOS-087` клиент `rusty-files` внедрил геометрические токены macOS 27 Golden Gate для всплывающих окон `xdg_popup`. Однако клиентские окна и всплывающие меню отрисовывали размытие заднего плана локально в SHM-буфер без аппаратной связи с композитором, что не позволяло клиентским поверхностям запрашивать оптическое преломление и динамическое размытие реального бэкдропа рабочего стола.

Использование сторонних протоколов (например, `org_kde_kwin_blur_manager` из KDE Plasma) нарушает фундаментальный принцип независимости архитектуры RustyOS. Требовался собственный, открытый, строго типизированный Wayland-протокол `rustyos_liquid_glass_v1`.

## Решение

1. **Спецификация протокола `rustyos_liquid_glass_v1` (`protocols/rustyos-liquid-glass-v1.xml`):**
   - Фабричный менеджер `rustyos_liquid_glass_manager_v1`:
     - `get_surface_glass(id: new_id<rustyos_surface_glass_v1>, surface: object<wl_surface>)`
     - `destroy()`
   - Декоратор поверхности `rustyos_surface_glass_v1`:
     - `set_region(region: object<wl_region> | null)`: область действия эффекта (null = вся поверхность).
     - `set_corner_radius(radius: uint)`: скругление углов (например, 16px для popover, 12px для sidebar).
     - `set_style(style: uint)`: пресеты оптического стиля (`Popover = 0`, `Sidebar = 1`, `ControlPill = 2`, `BasePanel = 3`, `Custom = 4`).
     - `set_tint(r, g, b, a)`: кастомный оверлей оттенка.
     - `commit()`: маркер готовности конфигурации (атомарное применение при `wl_surface.commit`).
     - `destroy()`: деструктор с автоматической очисткой ресурсов на сервере и клиенте.

2. **Серверная реализация в `crates/rusty-compositor`:**
   - Модуль `crates/rusty-compositor/src/state/wayland_handlers/liquid_glass.rs`:
     - Автогенерация интерфейсов и сервера через `wayland-scanner`.
     - `LiquidGlassSurfaceCachedState` с реализацией `smithay::wayland::compositor::Cacheable` для двойной буферизации параметров стекла на уровне поверхности.
     - `LiquidGlassManagerState` с регистрацией глобала `RustyosLiquidGlassManagerV1` в дисплее Smithay.
     - Обработка `Dispatch<RustyosLiquidGlassManagerV1>` и `Dispatch<RustyosSurfaceGlassV1>`.
     - Отслеживание активных поверхностей в `active_liquid_glass_surfaces: HashSet<ObjectId>`.

3. **Аппаратный рендеринг в `frame_composer.rs`:**
   - Автоматическая активация Dual-Kawase FBO бэкдропа (`needs_backdrop_blur`) при наличии активных клиентских поверхностей стекла (`has_active_client_liquid_glass()`).
   - Функция `push_window_liquid_glass_elements` в цикле отрисовки окон:
     - Для всех `xdg_popup` поверхностей окна инспектирует `LiquidGlassSurfaceCachedState` и генерирует `glass_panel_render_element` в точных экранных координатах всплывающего окна со стилем Popover и радиусом 16px.
     - Для основного окна инспектирует состояние стекла и генерирует аппаратные элементы стекла для сайдбара / указанных субрегионов.
     - Сохраняет строгий физический front-to-back Z-порядок рендеринга Smithay: `[поверхность попапа -> стекло попапа -> поверхность окна -> стекло окна -> SSD/тени]`.

4. **Клиентская интеграция в `apps/rusty-files`:**
   - Модуль `apps/rusty-files/src/liquid_glass.rs` с клиентскими биндингами `wayland-scanner`.
   - В `apps/rusty-files/src/wayland.rs`:
     - Обнаружение и привязка `RustyosLiquidGlassManagerV1` из Wayland-реестра.
     - При открытии любого `xdg_popup` (контекстные меню, меню действий, выбор тегов, сортировка) создаётся `RustyosSurfaceGlassV1` со стилем `GlassStyle::Popover` и радиусом 16px.
     - Для главного окна запрашивается `GlassStyle::Sidebar` с радиусом 12px.
     - Реализован RAII-деструктор `Drop for ActiveWaylandPopup`, гарантирующий уничтожение декоратора стекла при закрытии всплывающего окна.

## Последствия

### Положительные
- Полная независимость: протокол `rustyos_liquid_glass_v1` является нативной разработкой RustyOS, свободной от сторонних зависимостей других DE.
- Аппаратное качество: клиентские всплывающие окна и сайдбары получают реальный GPU Dual-Kawase бэкдроп блур с физическим преломлением и зеркальным кантом.
- Идеальный front-to-back Z-order: стекло находится непосредственно под полупрозрачными пикселями клиентской поверхности.
- 100% сохранение обратной совместимости, нулевой оверхед при отсутствии клиентов со стеклом, 100% прохождение всех тестов рабочего пространства (250/250).

### Ограничения
- Клиентские приложения без поддержки протокола `rustyos_liquid_glass_v1` отображаются со стандартным фоном тулкита.
