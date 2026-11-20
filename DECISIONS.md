# RustyOS --- Decision Log

Здесь хранятся только принятые решения. Предложения оформляются ADR со
статусом `proposed`.

## Принятые решения

  -----------------------------------------------------------------------
  ID                      Решение                 Статус
  ----------------------- ----------------------- -----------------------
  D-001                   Название проекта ---    accepted
                          RustyOS                 

  D-002                   Основной язык --- Rust  accepted

  D-003                   Самостоятельная DE, не  accepted
                          форк KDE/GNOME/Hyprland 

  D-004                   Основной display        accepted
                          protocol --- Wayland    

  D-005                   Smithay + Winit         accepted for prototype
                          исследуется как первый  
                          compositor prototype    

  D-006                   Coding agent --- Gemini accepted
                          через Google            
                          Antigravity             

  D-007                   Существенные            accepted
                          архитектурные изменения 
                          только после approval   
                          пользователя            

  D-008                   Экономия токенов        accepted
                          обязательна, но не в    
                          ущерб качеству и        
                          проверкам               

  D-009                   Фаза 1 (RUSTYOS-002):   accepted
                          `smithay 0.7`           
                          (`backend_winit`,       
                          `renderer_gl`,          
                          `wayland_frontend`,     
                          `desktop`), `calloop`   
                          event loop и `tracing`  
                          (ADR-0001)              

  D-010                   Фаза 2                  accepted
                          (RUSTYOS-004/005):      
                          чистый крейт            
                          `crates/rusty-wm`       
                          (`std`-only) для        
                          политик WM + интеграция 
                          в `rusty-compositor`    
                          (ADR-0002)              

  D-011                   Фаза 3 (RUSTYOS-006):   accepted
                          Native DRM/KMS +        
                          GBM/EGL/DMA-BUF         
                          capability discovery    
                          (`--drm-probe` /        
                          `--drm`) (ADR-0003)     

  D-012                   Фаза 5 (RUSTYOS-008):   accepted
                          чистый `std`-only крейт 
                          `crates/rusty-animation`
                          (monotonic clock,       
                          easing, retarget,       
                          reduced-motion) +       
                          интеграция в композитор 
                          (ADR-0004)              

  D-013                   Фаза 6                  accepted
                          (RUSTYOS-009/010):      
                          версионированный IPC    
                          `crates/rusty-ipc` (UDS 
                          `0600`, `v1`) +         
                          независимый процесс     
                          `apps/rusty-shell`      
                          (ADR-0005)              

  D-014                   Фаза 7 (RUSTYOS-011):   accepted
                          адаптеры системных      
                          сервисов Linux          
                          `crates/rusty-services` 
                          (sysfs, audio, network, 
                          logind, fallback)       
                          (ADR-0006)              

  D-015                   Фаза 8 (RUSTYOS-012):   accepted
                          нативный GLES           
                          композитный рендеринг   
                          TopBar и Dock поверх    
                          окон с эксклюзивными    
                          зонами и кликабельностью
                          (ADR-0007)              

  D-016                   Фаза 9 (RUSTYOS-013):   accepted
                          аппаратная матрица      
                          совместимости GPU       
                          (Intel/AMD/NVIDIA),     
                          Atomic KMS, DMA-BUF     
                          modifiers, fallback     
                          (ADR-0008)              

  D-017                   Фаза 10 (RUSTYOS-014):  accepted
                          сессионный лаунчер,     
                          wayland-session entry,  
                          пользовательский        
                          инсталлятор             
                          `scripts/install.sh`    
                          (ADR-0009)              

  D-018                   Фазы 3/4/9              accepted
                          (RUSTYOS-041):          
                          `zwp_linux_dmabuf_v1`   
                          + полноэкранный Direct  
                          Scanout через           
                          `DrmCompositor` с GLES  
                          fallback (ADR-0010)     
  -----------------------------------------------------------------------
  D-019                   Фазы 3/4/9              accepted
                          (RUSTYOS-042):          
                          Per-CRTC `DrmOutputState`
                          + Udev hotplug + max    
                          refresh mode selection +
                          runtime VRR + per-output
                          render list (ADR-0011)  
  -----------------------------------------------------------------------
  D-020                   Фазы 6/8/9              accepted
                          (RUSTYOS-043):          
                          `wp_fractional_scale_v1`
                          + `wp_viewporter` +     
                          per-output `[display]`  
                          HiDPI scaling (ADR-0012)

  D-021                   Фазы 6/8                accepted
                          (RUSTYOS-044):          
                          `zwp_relative_pointer_v1`
                          + `zwp_pointer_constraints_v1`
                          + `wp_presentation`     
                          (ADR-0013)              

  D-022                   Фазы 6/7/8              accepted
                          (RUSTYOS-045):          
                          `wp_cursor_shape_v1` +  
                          `xdg_activation_v1` +   
                          `ext_idle_notify_v1` +  
                          `idle_inhibit` +        
                          `ext_session_lock_v1`   
                          (ADR-0014)              

  D-023                   Фазы 3/4/5              accepted
                          (RUSTYOS-046):          
                          `wp_linux_drm_syncobj_v1`
                          + Observable Hardware   
                          Cursor Plane + HiDPI    
                          Cursor Sizing (ADR-0015)

  D-024                   Фазы 6/7/8              accepted
                          (RUSTYOS-047):          
                          `zwp_pointer_gestures_v1`
                          + `zwp_text_input_v3` + 
                          `zwp_input_method_v2` + 
                          `keyboard_shortcuts_inhibit`
                          +                       
                          `ext_foreign_toplevel_list_v1`
                          + `xdg_toplevel_icon_v1`
                          (ADR-0016)              

  D-025                   Фаза 7/UX               accepted
                          (RUSTYOS-048):          
                          macOS 27 Frontend       
                          Visual Alignment, SDF   
                          Vector Chrome & Glass   
                          Surface Controls        
                          (ADR-0017)              

  D-026                   Фаза 7/UX               accepted
                          (RUSTYOS-049):          
                          1:1 macOS Menu Bar      
                          Visual Parity, Bundled  
                          Inter Typography &      
                          Minimal Bar Config      
                          (ADR-0018)              

  D-027                   Фаза 7/UX               accepted
                          (RUSTYOS-050):          
                          1:1 macOS Dock Visual   
                          Parity, Top-Lit Glass & 
                          4x4 SSAA Squircle Icons 
                          (ADR-0019)              

  D-028                   Фаза 7/UX               accepted
                          (RUSTYOS-051):          
                          1:1 macOS Shell         
                          Overlays, Modular       
                          Control Center,         
                          Overview Spaces &       
                          Window Chrome           
                          (ADR-0020)              

  D-029                   Фаза 7/UX               accepted
                          (RUSTYOS-052):          
                          1:1 macOS TopBar 3-Band 
                          Live Wi-Fi & AC Cutout  
                          Battery Bolt            
                          (ADR-0021)              
  -----------------------------------------------------------------------
  D-030                   Фаза 7/UX               accepted
                          (RUSTYOS-053):          
                          Floating Liquid Glass   
                          Control Center Islands  
                          & Live MPRIS2/Display   
                          Parity (ADR-0022)       
  -----------------------------------------------------------------------
  D-031                   Фаза 7/UX               accepted
                          (RUSTYOS-054):          
                          1:1 macOS Lock Screen   
                          Wallpaper Backdrop,     
                          Frosted Clock & User    
                          Selector (ADR-0023)     
  -----------------------------------------------------------------------
  D-032                   Фаза 7/Display          accepted
                          (RUSTYOS-055):          
                          Display Output Rotation 
                          (0/90/180/270°) &       
                          Pointer Mapping         
                          (ADR-0024)              
  -----------------------------------------------------------------------
  D-033                   Фаза 7/Security         accepted
                          (RUSTYOS-056):          
                          Неблокирующая PAM       
                          аутентификация Lock     
                          Screen, systemd-logind  
                          и Polkit Agent          
                          (ADR-0025)              
  D-034                   Фаза 7/Portals          accepted
                          (RUSTYOS-057): Нативный 
                          XDG Desktop Portal      
                          backend (Screenshot v3, 
                          PickColor, ScreenCast   
                          v6) и изолированный     
                          PipeWire воркер         
                          rusty-pipewire-stream   
                          (ADR-0026)              
  -----------------------------------------------------------------------
  D-035                   Фаза 8/Architecture &   accepted
                          Session (RUSTYOS-060):  
                          Разделение системных/   
                          интерактивных overlay и 
                          persistent shell chrome 
                          в Z-order,              
                          backend-независимый     
                          frame_composer.rs, CI   
                          dev-пакеты и встроенный 
                          shell по умолчанию с    
                          opt-in                  
                          RUSTYOS_EXTERNAL_SHELL= 
                          1 (ADR-0027)            
  -----------------------------------------------------------------------
  D-036                   Фаза 8/Core UX &        accepted
                          Settings (RUSTYOS-061): 
                          Сериализация UserConfig 
                          и атомарное сохранение  
                          в config.toml, горячая  
                          перезагрузка            
                          конфигурации            
                          (ReloadConfig в System  
                          Menu, Super+Shift+R и   
                          rusty-ipc) и            
                          встроенная панель       
                          System Settings         
                          (Appearance, Menu Bar,  
                          Displays, Security)     
                          (ADR-0028)              
  -----------------------------------------------------------------------
  D-037                   Фаза 8/Core UX &        accepted
                          Desktop/Dock            
                          (RUSTYOS-064): Единая   
                          подсистема              
                          ShellOverlayView::Conte 
                          xtMenu (Liquid Glass    
                          Popover, подменю >,     
                          клавиатура,             
                          мультимониторный        
                          clamping), ПКМ-меню     
                          Desktop и всех слотов   
                          Dock (Open at Login,    
                          Alt->Force Quit,        
                          Downloads Sort By,      
                          неблокирующий Empty     
                          Trash) и секция [dock]  
                          в UserConfig            
                          (ADR-0029)              
  -----------------------------------------------------------------------
  D-038                   Фаза 8/Системные        accepted
                          приложения              
                          (RUSTYOS-065):          
                          Самостоятельный Wayland 
                          файловый менеджер       
                          apps/rusty-files        
                          (Этап A: Golden Gate    
                          CSD, Columns/List,      
                          Light по умолчанию <->  
                          Dark Purple Glass) и    
                          бэкенд                  
                          rusty-services::file_ma 
                          nager (XDG, Trash,      
                          file_tags.toml); полный 
                          перевод Dock/Desktop/   
                          Launcher на rusty-files 
                          без сторонних ФМ        
                          (ADR-0030)              
  -----------------------------------------------------------------------
  D-039                   Фаза 8/Системные        accepted
                          приложения              
                          (RUSTYOS-066):          
                          Этап B apps/rusty-files 
                          (Icons, Gallery,        
                          Quick Look, Tabs,       
                          Path Bar, Get Info,     
                          Go to Folder, Rename,   
                          фоновый Copy/Move с     
                          конфликтами, чёткие     
                          иконки + 4x4 SSAA       
                          папки) и общий декодер  
                          rusty_services::image_d 
                          ecode (ADR-0031)        
  -----------------------------------------------------------------------
  D-040                   Фаза 8/Design System &  accepted
                          Shell (RUSTYOS-067):    
                          Полный отказ от темы    
                          Purple Glass и переход  
                          на светлую и тёмную     
                          темы Golden Gate        
                          (AppearanceMode::Light/ 
                          Dark, ThemePalette,     
                          темизация контекстных   
                          меню, меню TopBar,      
                          панелей оболочки,       
                          процедурных обоев и     
                          apps/rusty-files по     
                          референсам) (ADR-0032)  
  -----------------------------------------------------------------------
  D-041                   Фаза 8/Shell & TopBar   accepted
                          (RUSTYOS-068):          
                          Динамическое app-aware  
                          меню TopBar по профилю  
                          активного окна          
                          (TopBarAppProfile:      
                          Files, Browser,         
                          Terminal, Editor,       
                          GenericApp; скрытие Go  
                          вне Files; реальное имя 
                          в About/Minimize/Quit/  
                          Force Quit; раскладко-  
                          независимая отправка    
                          шорткатов File/Edit/    
                          View через XKB)         
                          (ADR-0033)              
  -----------------------------------------------------------------------
  D-042                   Фаза 8/Compositor &     accepted
                          Liquid Glass            
                          (RUSTYOS-069): GLES     
                          Dual-Kawase FBO         
                          backdrop blur (5        
                          проходов 1->1/2->1/4->  
                          1/8->1/4->1/2) и шейдер 
                          LIQUID_GLASS_FRAG с     
                          edge lensing, damage-   
                          gating и runtime CPU    
                          fallback при lock /     
                          reduce_transparency     
                          (ADR-0034)              

  D-043                   Фаза 8/Compositor &     accepted
                          Dock (RUSTYOS-073):     
                          app-centric Dock        
                          (группировка без дублей,
                          динамические пины,      
                          Recent & свёрнутые      
                          мини-слоты), локальный  
                          hover наведённой иконки 
                          (+8 px / +6 px lift без 
                          волны), Launch bounce,  
                          GLES Genie/Scale,       
                          позиции Bottom/Left/    
                          Right + Auto-hide,      
                          Downloads Stack (Fan/   
                          Grid/List), drag-реордер
                          и System Settings ->    
                          Desktop & Dock          
                          (ADR-0035)              

  D-044                   Фаза 10/Arch Packaging & accepted
                          Updates (RUSTYOS-076    
                          Этап A): Arch PKGBUILD  
                          (`packaging/arch/PKGBUILD`),
                          актуализированный       
                          `scripts/install.sh` (с 
                          `rusty-files` и         
                          uninstall),             
                          непривилегированный     
                          `UpdateCheckerHandle`   
                          (`checkupdates --nocolor`)
                          в `rusty-services`,     
                          секция `[updates]` в    
                          `UserConfig`,           
                          TopBar-индикатор и      
                          `ShellOverlayView::Updates`
                          Liquid Glass поповер    
                          (ADR-0036)              

  D-045                   Фаза 10/Arch Updater    accepted
                          (RUSTYOS-077 Этап B):   
                          standalone Wayland CSD  
                          `apps/rusty-updater`    
                          (Golden Gate Light/Dark),
                          потоковый               
                          `UpdateTransactionHandle`
                          и узкий Polkit-хелпер   
                          `rustyos-update-helper` 
                          (`pkexec` без `sh -c`,  
                          непривилегированный     
                          `makepkg` + `pacman`,   
                          policy XML и            
                          BuiltinAppIcon::Updater)
                          (ADR-0037)              

  D-046                   Фаза 10/Arch Session &  accepted
                          Readiness (`RUSTYOS-078`):
                          асинхронный экспорт     
                          `WAYLAND_DISPLAY` /     
                          `DISPLAY` в             
                          `dbus-update-activation-environment`
                          строго только в         
                          `--drm`, read-only      
                          `rustyos-session --doctor`,
                          `org.rustyos.files.desktop`,
                          `depends`/`--remap-path-prefix`
                          в `PKGBUILD` и Arch job 
                          `makepkg` в CI          
                          (ADR-0038)              

  D-047                   Фаза 10/CI Arch Pacman  accepted
                          Repository (`RUSTYOS-079`):
                          конвейер сборки и       
                          публикации pacman-      
                          репозитория в           
                          `publish-arch-repo` CI, 
                          GPG/откреплённая        
                          подпись и graceful      
                          fallback, раздача через 
                          `gh-pages` branch,      
                          проверка `[rustyos]` в  
                          `SystemReleaseSummary`  
                          и статус сессии в UI    
                          (ADR-0039)              

  D-048                   Фаза 10/Сессия и        accepted
                          Display Manager         
                          (`RUSTYOS-084`):        
                          нативное окружение      
                          тулкитов (Qt, GTK,      
                          Electron, Firefox, SDL) 
                          в `bin/rustyos-session`, 
                          цели `systemd --user`   
                          `rustyos-session.target`
                          и shutdown target с     
                          graceful cleanup при    
                          Log Out, установка      
                          юнитов в PKGBUILD и     
                          проверки в `--doctor`   
                          (ADR-0040)              

  D-050                   Фаза 10/Shell и Окна    accepted
                          (`RUSTYOS-086`):        
                          унифицированное         
                          системное контекстное   
                          меню по стандарту macOS 
                          27, декомпозиция меню   
                          в `context_menu/`,      
                          системное управление    
                          окном при клике на      
                          заголовок/CSD           
                          (`show_window_menu`),   
                          тайлинг, воркспейсы,    
                          закрытие и гибридное    
                          меню в `rusty-files`    
                          (ADR-0042)              
  -----------------------------------------------------------------------
  D-051                   Фаза 8/10 Core UX       accepted
                          (`RUSTYOS-087`):        
                          стандарт 1:1 Liquid     
                          Glass Popover для       
                          клиентских `xdg_popup`  
                          поверхностей в          
                          `rusty-files`,          
                          скругление 16px,        
                          плавающие инсетные      
                          пилюли ховера 8px,      
                          зеркальный кант,        
                          многослойная SDF-тень,  
                          колонка шорткатов,      
                          декомпозиция рендера    
                          в `render/popup.rs`     
                          (ADR-0043)              
  -----------------------------------------------------------------------
  D-052                   Фаза 8/10 Architecture  accepted
                          (`RUSTYOS-088`):        
                          модульная декомпозиция  
                          монолитных подсистем    
                          `crates/rusty-services/src/file_manager/`
                          (types, listing, trash, 
                          operations, metadata) и 
                          `apps/rusty-files/src/state/`
                          (types, tabs, dialogs,  
                          clipboard, search,      
                          input, navigation) со   
                          100% сохранением        
                          публичных контрактов и  
                          всех 244 тестов         
                          (ADR-0044)              
  -----------------------------------------------------------------------
  D-053                   Фаза 8/10 Architecture  accepted
                          (`RUSTYOS-089`):        
                          модульная декомпозиция  
                          подсистемы рендеринга   
                          `apps/rusty-files/src/render/`
                          (canvas, chrome,        
                          sidebar, views, dialogs,
                          popup, mod), перенос    
                          раскладки и хит-тестов  
                          в `layout.rs`,          
                          упразднение монолитных  
                          `render.rs`, `views.rs` 
                          и `dialogs.rs`, 100%    
                          сохранение всех тестов  
                          (ADR-0045)              
  -----------------------------------------------------------------------
  D-054                   Фаза 8/10 Architecture  accepted
                          (`RUSTYOS-090`):        
                          модульная декомпозиция  
                          `crates/rusty-compositor/src/shell_render/top_bar/`
                          (panel, menus, status,  
                          notifications, mod) по  
                          стандартам CutefishOS и 
                          macOS 27, упразднение   
                          монолита top_bar.rs,    
                          100% сохранение всех    
                          публичных контрактов и  
                          всех 244 тестов         
                          (ADR-0046)              
  -----------------------------------------------------------------------
  D-055                   Фаза 8/10 Architecture  accepted
                          (`RUSTYOS-091`):        
                          модульная декомпозиция  
                          `crates/rusty-compositor/src/shell_render/modals/`
                          (polkit, portal,        
                          magnifier, settings,    
                          mod) по стандартам      
                          CutefishOS и macOS 27,  
                          упразднение монолита    
                          modals.rs, 100%         
                          сохранение всех         
                          публичных контрактов и  
                          всех 244 тестов         
                          (ADR-0047)              
  -----------------------------------------------------------------------
  D-056                   Фаза 8/10 Graphics &    accepted
                          Wayland Protocols       
                          (`RUSTYOS-092`):        
                          собственный Wayland-    
                          протокол Liquid Glass   
                          `rustyos_liquid_glass_v1`,
                          серверная реализация    
                          менеджера и двойной     
                          буферизации состояния   
                          стекла в композиторе,   
                          аппаратный рендеринг    
                          Dual-Kawase бэкдропа    
                          в `frame_composer.rs`,  
                          клиентская интеграция в 
                          `apps/rusty-files` для  
                          `xdg_popup` и сайдбара, 
                          сохранение 100% тестов  
                          (ADR-0048)              
  -----------------------------------------------------------------------
  D-057                   Фаза 8/10 Desktop Shell & accepted
                          Liquid Glass (`RUSTYOS-093`):
                          приведение контейнера   
                          Дока к эталону 1:1      
                          macOS 27 Golden Gate:   
                          геометрия Pill Capsule  
                          (`radius = height / 2 = 34px`),
                          top-lit световой фасет  
                          в шейдере и SDF,        
                          контактное притенение   
                          нижней кромки, мягкая   
                          тень `push_soft_shadow` 
                          под доком (ADR-0049)    
  -----------------------------------------------------------------------
  D-058                   Фаза 8/10 Desktop Shell & accepted
                          Liquid Glass (`RUSTYOS-094`):
                          приведение контейнера   
                          Дока к точному          
                          референсу: радиус       
                          скругления 18px (прямые 
                          вертикальные боковины), 
                          плоское матовое стекло  
                          без эффекта выпуклости  
                          (удалены `top_shelf` и  
                          темный `bottom_occlusion`),
                          равномерный 1px кант,   
                          сохранение 100% тестов  
                          (ADR-0050)              
  -----------------------------------------------------------------------
  D-059                   Фаза 8/10 Desktop Shell & accepted
                          Liquid Glass (`RUSTYOS-095`):
                          калибровка контейнера   
                          Дока по стандартам      
                          CutefishOS: полное      
                          удаление внешней тени   
                          `push_soft_shadow`,     
                          устранение угловых      
                          3D-бликов (`arc_tl`/    
                          `arc_br`) и дымки       
                          `sheen`, чистый         
                          равномерный 1px         
                          hairline кант,          
                          сохранение 100% тестов  
                          (ADR-0051)              
  -----------------------------------------------------------------------
   D-060                   Фаза 8/10 Desktop Shell & accepted
                           Liquid Glass (RUSTYOS-096):
                           dock_flat маска в
                           GPU-шейдере и CPU sdf.rs:
                           обнуление lens_band,
                           light_fresnel, rim
                           для Дока через
                           sat_refract.y/z = 0.0
                           -> плоское матовое стекло
                           без Fresnel/линзы
                           (170 тестов ok, D-060)
  -----------------------------------------------------------------------
   D-061                   Фаза 8/10 Desktop Shell & accepted
                           Liquid Glass (RUSTYOS-097):
                           устранение вертикального
                           градиента t_v в
                           base_glass_layer для
                           Dock Light и Dark -> 100%
                           однородный плоский тинт
                           без перепадов яркости
                           (251 тест ok, D-061)
  -----------------------------------------------------------------------
   D-062                   Фаза 8/10 Desktop Shell & accepted
                           Liquid Glass (RUSTYOS-098):
                           калибровка контейнера Дока
                           по CutefishOS: формула
                           радиуса thickness * 28 / 86
                           (compute_dock_radius),
                           нейтральный серый тинт
                           alpha 0.22 и 1px
                           screen-blend hairline кант
                           в активном GPU-шейдере
                           LIQUID_GLASS_FRAG и CPU
                           sdf.rs (251 тест ok, D-062)
  -----------------------------------------------------------------------
   D-063                   Фаза 8/10 Desktop Shell & accepted
                           Liquid Glass (RUSTYOS-099):
                           перенос точной математики
                           CutefishOS в GPU-шейдер
                           LIQUID_GLASS_FRAG
                           (суперэллипса r*1.5, 14px
                           рефракционная линза,
                           3-слойный uRim с lit=1.0/
                           0.35/0.0 без белой боковой
                           рамки), 1-pass half_a blur
                           для Дока и сглаживание
                           складки sheet3 обоев
                           (ADR-0052, 251 тест ok)
  -----------------------------------------------------------------------
   D-064                   Фаза 8/10 Desktop Shell & accepted
                           Liquid Glass (RUSTYOS-100):
                           унификация всех стеклянных
                           поверхностей проекта
                           (Popover = popupGlass,
                           ControlModule = fishuiGlass,
                           клиентские Sidebar/Pill/
                           поповеры rusty-files) на
                           единый суперэллиптический
                           шейдер LIQUID_GLASS_FRAG,
                           единый CPU fallback sdf.rs
                           и направленный uRim
                           (ADR-0053, 251 тест ok)
  -----------------------------------------------------------------------
   D-065                   Фаза 8/10 Модульная       accepted
                           декомпозиция (RUSTYOS-101):
                           разделение монолита
                           shell_render/mod.rs
                           (2014 строк) на text_pass.rs
                           (595), overlay_shapes.rs
                           (398), chrome_shapes.rs
                           (759) и фасад mod.rs (329)
                           с сохранением 100% API и
                           251 тестов (ADR-0054)
  -----------------------------------------------------------------------
   D-066                   Фаза 8/10 UI & System     accepted
                           Settings (RUSTYOS-102):
                           редизайн System Settings
                           (760x520) по архитектуре
                           cutefishos/settings —
                           Liquid Glass сайдбар
                           слева (traffic lights,
                           Search, RowIcon сквирклы,
                           нейтральный выбор таба),
                           непрозрачный холст справа,
                           превью Light/Dark,
                           групповые Card и тумблеры
                           Switch (ADR-0055, 252
                           теста ok)
  -----------------------------------------------------------------------
   D-067                   Фаза 8/10 UI & System     accepted
                           Settings (RUSTYOS-103):
                           вынос System Settings в
                           отдельное Wayland CSD-
                           окно apps/rusty-settings
                           (882x600), удаление
                           UI-упоминаний CutefishOS/
                           Golden Gate, скролл и
                           сквозная синхронизация
                           темы Light/Dark/Auto
                           через rusty-ipc,
                           LiquidGlassPipeline и
                           SSD (ADR-0055)
  -----------------------------------------------------------------------
   D-068                   Фаза 8/10 UI & Control    accepted
                           Center (RUSTYOS-104):
                           калибровка ControlModule
                           (half_out, [1.30, 34.0,
                           1.0, 0.01], Light [0.72,
                           0.74, 0.78, 0.11+0.11g],
                           Dark [0.24, 0.24, 0.27,
                           0.15+0.15g]) и CPU fallback
                           в sdf.rs; капсулы r=h/2,
                           слайдеры с 12px thumb,
                           inset-тумблеры и контраст
                           подписей в quick_settings.rs
                           (ADR-0056)
  -----------------------------------------------------------------------
   D-069                   Фаза 8/10 UI & CSD        accepted
                           Unification (RUSTYOS-105):
                           единый радиус окон и теней
                           14 px (WINDOW_RADIUS и
                           WINDOW_CORNER_RADIUS = 14),
                           14 px traffic lights (шаг
                           22 px, #EE5A6A/#F4B63F/
                           #3CC68A), палитра #ECEDF1/
                           #F5F6FA и #262628/#1C1C1D,
                           карточки и капсулы без
                           рамок в rusty-files и
                           rusty-updater (ADR-0057)
  -----------------------------------------------------------------------
   D-070                   Фаза 8/10 UI & Control    accepted
                           Center Geometry (RUSTYOS-106):
                           чистый евклидов L2 SDF
                           (sdRoundBox) в
                           LIQUID_GLASS_FRAG; точная
                           сетка D=70, G=12: круги
                           70x70 r=35, капсулы 152x70
                           r=35 (иконка 46 px),
                           Now Playing 152x152 r=28,
                           Display/Sound 316x66 r=26,
                           Lock Screen 118x28 r=14 в
                           quick_settings.rs (ADR-0056)
  -----------------------------------------------------------------------
   D-071                   Фаза 8/10 Контекстные и   accepted
                           выпадающие меню (RUSTYOS-107):
                           концентрический r=13 / r_pill=7
                           при 5 px инсете, строки 24 px,
                           разделители 9 px, двухрежимная
                           левая колонка (x=14 без иконки /
                           14 px иконка + x=35 текст),
                           12 px шорткаты в нотации macOS
                           и xdg_popup r=13 в rusty-files
                           без правок Dock/CC (ADR-0058)

   D-072                   Фаза 8/10 Разделение Ethernet и accepted
                           Wi-Fi, TopBar, WifiPopover и
                           страницы Settings > Wi-Fi /
                           Network (RUSTYOS-108):
                           раздельные поля Ethernet и Wi-Fi
                           в NetworkStatus, иконка Ethernet
                           в TopBar при wired && !wifi,
                           поповер WifiPopover (r=13 / r_pill=7
                           без правок Dock/CC и liquid_glass.rs)
                           и отдельные страницы Wi-Fi / Network
                           в rusty-settings (ADR-0059)

   D-073                   Фаза 8/10 Разделение триггеров accepted
                           Control Center и Clock в TopBar,
                           правая панель Notification Center
                           (RUSTYOS-109): история уведомлений
                           после истечения TTL тоста, ведущая
                           карточка + заголовок с очисткой +
                           стопка, виджет Weather и интерактивный
                           Reminders, Edit Widgets, секция
                           [notifications] и страница
                           Settings > Notifications (#FF3B30)
                           без правок Dock/CC/liquid_glass.rs
                           (ADR-0060)

   D-074                   Фаза 8/10 Стиль GlassPanelStyle::  accepted
                           NotificationCard, унифицированные
                           тосты/карточки уведомлений и
                           синяя палитра Golden Gate в
                           icons.rs (RUSTYOS-110): матовый
                           баланс r=22 (refract=3.0, rim=0.22),
                           концентричный отступ иконки x+16,
                           многострочное перенесение текста
                           без "now", без изменений в Dock,
                           ControlModule и Popover (ADR-0061)

   D-075                   Фаза 8/10 Конвейер сглаживания     accepted
                           иконок и глифов (RUSTYOS-111):
                           16-tap R2 + LinearAccum, 2x HiDPI
                           буферы (GLYPH_BUFFER_SCALE = 2),
                           сепарабельный Lanczos-3 downscale
                           в линейном свете для PNG/SNI и
                           4-tap RGSS (aa_cov8) в
                           rusty-settings и rusty-updater
                           (ADR-0062)

   D-076                   Фаза 8/10 Жёсткий min_size при     accepted
                           resize, очистка минимизации/стекла
                           и плавность rusty-settings
                           (RUSTYOS-112): пер-оконный
                           compute_resized_rect_with_min и
                           resize_window_with_min, снапшот
                           минимизации без тени с экранным
                           Liquid Glass backdrop, premul +
                           4-tap префильтрация в шейдере,
                           пропуск хрома незакоммиченных
                           клиентов, фоновый service worker,
                           субпиксельный/плавный скролл,
                           сканлайн-заливки, SPRITE_CACHE и
                           точечный damage_buffer (ADR-0063)

   D-077                   Фаза 8/10 GPU-ускоренный Wayland   accepted
                           CSD-терминал apps/rusty-terminal
                           (RUSTYOS-113): Wayland-EGL + GLES2
                           2-pass batched renderer с
                           1024x1024 GlyphAtlas (ShelfPacker)
                           и wl_shm fallback без новых прямых
                           внешних крейтов,
                           rustyos_liquid_glass_v1 (Clear
                           Light / Clear Dark / Opaque),
                           rustix PTY + calloop::ping, свой
                           UTF-8/256/TrueColor VT-движок,
                           вкладки, плавный скролл, поиск,
                           clipboard и интеграция в
                           Dock/Launcher/Desktop (ADR-0064)

   D-078                   Фаза 8/10 Переработка иконок      accepted
                           macOS 27 (Dock и папки),
                           синхронизация [ui].accent_index
                           и браузер Firefox по умолчанию
                           (RUSTYOS-114): процедурный
                           16-tap R2 модуль icon_art.rs
                           (Light/Dark + [ui].accent_index),
                           единая палитра в rusty-services
                           и живой ReloadConfig из
                           rusty-settings, дебоссированные
                           папки S-силуэта в rusty-files,
                           обрамление XDG-иконки Firefox в
                           макосный сквиркл с объединением
                           Dock-слота browser и зависимость
                           firefox в PKGBUILD

   D-079                   Фаза 10 Инсталлятор             accepted
                           apps/rusty-installer, OOBE
                           Setup Assistant, безопасный
                           helper scripts/rustyos-install.sh
                           и Live ISO packaging/archiso/
                           (RUSTYOS-115, ADR-0065):
                           Wayland CSD-клиент на
                           smithay-client-toolkit без
                           новых зависимостей в Cargo.lock,
                           режимы --install и --oobe,
                           защита смонтированных дисков,
                           Polkit org.rustyos.installer.apply,
                           доупаковка rusty-settings в
                           PKGBUILD/install.sh/session,
                           автозапуск .oobe-pending и
                           профиль archiso на greetd

   D-080                   Фаза 10 Безопасный Dual-Boot     accepted
                           режим в rusty-installer и
                           scripts/rustyos-install.sh
                           (RUSTYOS-116, ADR-0066): режим
                           --install-mode dual-boot с
                           выбором --root-part или
                           --free-space (по START/LOG-SEC
                           из lsblk без root и без
                           sgdisk --zap-all), защитой и
                           переиспользованием EFI >= 260M
                           без mkfs.fat либо отдельным
                           512M EFI при малом ESP,
                           сохранением loader.conf в
                           systemd-boot и GRUB + os-prober

   D-081                   Фаза 10 Общий UI-крейт и        accepted
                           интеграция обновлений в
                           rusty-settings (RUSTYOS-123,
                           ADR-0067): создание crates/rusty-ui
                           с общими светофорами (14 px,
                           центры 21/43/65 px), токенами,
                           WindowFrame и AA-холстом UiCanvas;
                           интеграция страницы Software Update
                           в apps/rusty-settings (--page updates);
                           полное удаление apps/rusty-updater
                           и перевод вызовов оболочки и
                           desktop-файла на rusty-settings

    D-082                  Фаза 10 Сквозное подключение   accepted
                           crates/rusty-ui и модуль
                           виджетов (RUSTYOS-124, ADR-0068):
                           добавление модуля widgets (ToggleSwitch,
                           PushButton, GroupCard, SearchField);
                           подключение rusty-ui во все приложения
                           (rusty-files, rusty-terminal,
                           rusty-installer) и Server-Side
                           Decorations композитора;
                           устранение локального дублирования
                           геометрии светофоров и микроглифов

    D-083                  Фаза 10 Унификация контекстных  accepted
                           меню и заголовков окон
                           (RUSTYOS-125, ADR-0069):
                           создание модулей menu и
                           window_header в crates/rusty-ui
                           с эталонными метриками 26/12/14/9 px
                           и перекрытием каскадных подменю
                           2 px; сквозное применение стиля
                           контекстных меню в rusty-compositor
                           и rusty-files

    D-084                  Фаза 10 Трёхдольная система     accepted
                           теней Гаусса и ликвидация
                           хардкода (RUSTYOS-126, ADR-0070):
                           создание модуля shadow в
                           crates/rusty-ui с аналитической
                           моделью 3-lobe Gaussian shadows
                           (ActiveWindow, InactiveWindow,
                           MenuOrTooltip, Popup, Dock) и
                           отрисовкой на UiCanvas;
                           сквозная замена локального хардкода
                           теней в rusty-compositor, rusty-files
                           и rusty-ui

    D-085                  Фаза 10 Синхронизация структуры  accepted
                           меню TopBar и стилей popover
                           (RUSTYOS-127, ADR-0071):
                           профиль TopBarAppProfile::Desktop
                           по умолчанию (скрытие Go),
                           жизненный цикл Hide/Hide Others/Quit
                           с полем enabled, очищенное меню
                           SystemLogo, плашка 8 px в TopBar,
                           концентрический ховер 9 px в меню
                           14 px и живая диагностика --winit

    D-086                  Фаза 6/8/10 Консолидация      accepted
                           архитектуры и 1:1 UX-паритет
                           (RUSTYOS-128, ADR-0072):
                           удаление дублей (apps/rusty-shell,
                           modals/settings.rs), единый
                           WINDOW_RADIUS=14 и 3-дольный
                           GPU-шейдер теней Гаусса без
                           хардкода, единый кубический
                           суперэллипс и композиторная
                           обрезка неполноэкранных окон
                           (ClippedSurfaceElement с сохранностью
                           Direct Scanout), open/close
                           анимации (180/160 ms, 0.93<->1.0),
                           послойный Liquid Glass сайдбаров и
                           приглушение светофоров без фокуса

    D-087                  Фаза 6/8/10 Калибровка          accepted
                           ShadowElevation и облегчение
                           GlassPanelStyle::Popover
                           (RUSTYOS-129, ADR-0073): удаление
                           contact-обводки (contact=0.0) и
                           ослабление ореола для
                           MenuOrTooltip/Popup, мягкий
                           contact (0.28/0.20) для окон,
                           облегчённый Popover-материал
                           (displacement=6.0, rim=0.22,
                           чистый прохладный тинт Light/Dark),
                           обновлённые токены
                           popover_text_disabled и
                           popover_separator (0.055/0.07)

    D-088                  Фаза 7/8/10 Клиентский          accepted
                           UI-тулкит rusty-ui и dual
                           GLES 2.0 / CPU презентер
                           (RUSTYOS-130, ADR-0074):
                           полноценные контролы (FocusScope,
                           TextField/PasswordField/SearchField,
                           WaylandClipboardBridge,
                           DropdownSelect, SegmentedTabs,
                           SelectableList, ScrollView,
                           DialogSheet, ContextMenuSession,
                           ToggleMotion/ScrollMotion с
                           reduced_motion), единый UiDrawList
                           + GlesSurfaceRenderer (Wayland-EGL +
                           GLES 2.0 с изоляцией unsafe FFI в
                           crates/rusty-ui/src/gpu_gles.rs и
                           CPU wl_shm fallback), перевод
                           rusty-installer, rusty-settings и
                           rusty-files на единый тулкит и
                           Shared UiFontEngine без fontdue-дублей
  -----------------------------------------------------------------------

## Открытые решения

- Initial development distro (Arch Linux --- кандидат).
- MSRV.
- Точные версии и детали graphics backend.
- IPC transport.
- Shell UI toolkit.
- Packaging/distribution base.

## Процесс

Создать `docs/adr/NNNN-title.md` со статусом `proposed`. После явного
approval пользователя изменить статус на `accepted` и добавить запись
сюда. Молчание не считается согласием.
