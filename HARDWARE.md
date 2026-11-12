# RustyOS --- Hardware Compatibility

## 1. Цель

AMD, Intel и NVIDIA --- целевые семейства. Компиляция или один успешный
запуск не означает поддержку GPU.

## 2. Цепочка совместимости

Проверять: 1. Kernel DRM driver. 2. Userspace driver (Mesa/NVIDIA
stack). 3. GBM/EGL или выбранные graphics interfaces. 4. DMA-BUF
import/export и synchronization. 5. DRM/KMS atomic modesetting. 6.
Presentation/page flip. 7. Cursor planes. 8. Multi-monitor и mixed
refresh. 9. Hotplug. 10. Suspend/resume.

## 3. AMD

Проверить amdgpu, userspace stack, EGL/GLES, GBM, DMA-BUF, modifiers и
synchronization. Записывать точную модель и версии.

## 4. Intel

Проверить i915 или xe в зависимости от поколения, форматы буферов,
modifiers, synchronization и outputs.

## 5. NVIDIA

Отдельно тестировать proprietary driver и альтернативы, если они
заявляются. Проверять GBM, DMA-BUF, explicit sync, KMS modesetting и
cursor behavior. Не экстраполировать один результат на всё семейство.

## 6. Capability-based design

Runtime detect: - hardware cursor; - direct scanout; - explicit sync; -
modifiers; - VRR; - overlays.

Если capability отсутствует --- безопасный fallback и понятная
диагностика.

## 7. Матрица

Вести `docs/HARDWARE_MATRIX.md`: GPU, kernel, driver, userspace,
backend, resolution/Hz, monitor count, cursor, sync, suspend/resume,
date, commit, status и issues.

Статусы: `untested`, `experimental`, `working`, `degraded`,
`unsupported`.

Нельзя автоматически менять системный драйвер, bootloader, display
manager или настройки ОС.
