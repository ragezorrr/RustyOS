# RustyOS --- Hardware Test Matrix

Заполнять только по реальным тестам. Не переносить результат с одной
модели GPU на другую.

  --------------------------------------------------------------------------------------------------------------------
  GPU    Kernel   Driver   Userspace   Backend   Mode/Hz   Displays   Cursor   Suspend/resume   Status     Notes
  ------ -------- -------- ----------- --------- --------- ---------- -------- ---------------- ---------- -----------
  Intel  7.2.11   i915     Mesa        Winit     1280x800  1 (nested) Working  N/A              working    Tested
  RPL-U  -arch1-1          26.2.6 /    (EGL /    @ 60 Hz              (host)                               2026-11-11
  [8086:                   GLES 3.2    GLES)                                                               with
  a7ac]                                                                                                    `es2gears_wayland`

  Intel  7.2.11   i915     Mesa        DRM/KMS + 1920x1080 1 (eDP-1   256x256  N/A              experimental Tested
  RPL-U  -arch1-1          26.2.6 /    GBM /     @ 60 Hz   detected,  cursor                                 2026-11-12
  [8086:                   GLES 3.2    EGL /               4 CRTCs,   plane                                  (`--drm-probe`):
  a7ac]                                DMA-BUF             24 planes)                                        Atomic KMS,
                                                                                                             GBM, EGL 1.5,
                                                                                                             133 render /
                                                                                                             236 texture
                                                                                                             DMA-BUF
                                                                                                             formats,
                                                                                                             offscreen
                                                                                                             DMA-BUF
                                                                                                             render+import
                                                                                                             verified
  --------------------------------------------------------------------------------------------------------------------

Статусы: `untested`, `experimental`, `working`, `degraded`,
`unsupported`.

Для каждой записи сохранять дату, commit hash и команды тестирования.
