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
