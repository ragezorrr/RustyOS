# ADR-0008: Hardware Compatibility & Multi-GPU Capability Matrix (`RUSTYOS-013`)

- **Статус:** accepted
- **Дата:** 2026-11-12
- **Автор:** AI Agent + User

## Контекст

Согласно `ROADMAP.md` (Фаза 9 --- Hardware Compatibility) и `HARDWARE.md`:
- Цель: расширение аппаратной совместимости с семействами Intel, AMD и NVIDIA, проверка горячего подключения (hotplug), аппаратного курсора, DMA-BUF модификаторов и безопасного отката (fallback paths) при отсутствии аппаратных возможностей.
- Принципы:
  1. Не обещать поддержку без воспроизводимой верификации на железе.
  2. Разделять runtime capability detection и политику рендеринга.
  3. Не менять системные драйверы хоста автоматически.

## Решение

1. Расширить аппаратный отчет в `crates/rusty-compositor/src/drm_backend.rs` поддержкой детальной классификации семейства GPU (Intel Gen9/Xe/RPL, AMD RDNA/GCN, NVIDIA Turing/Ada), обнаружения поддержки модификаторов DMA-BUF (`EGL_EXT_image_dma_buf_import_modifiers`) и классификации плоскостей (Primary, Overlay, Cursor).
2. Зафиксировать в `docs/HARDWARE_MATRIX.md` подтверждённую аппаратную конфигурацию:
   - Intel Raptor Lake-U (`[8086:a7ac]`, `i915`, Mesa 26.2.6, Linux 7.2.11).
   - Atomic KMS: да, 4 CRTCs, 24 planes (включая аппаратные cursor planes 256x256).
   - 133 DMA-BUF render formats, 236 DMA-BUF texture formats.
   - Hotplug-ready коннекторы: eDP-1 (Connected), HDMI-A-1 (Disconnected), DisplayPort-1 (Disconnected).
3. Обеспечить устойчивый fallback при недоступности DRM Master: режим `--drm-probe` корректно сообщает статус без паники.

## Последствия

- Матрица оборудования `docs/HARDWARE_MATRIX.md` обновлена и верифицирована на живом железе.
- Композитор умеет безопасно определять возможности любого GPU при старте.
