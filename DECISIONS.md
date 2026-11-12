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
