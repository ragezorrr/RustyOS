# ADR-0066: Безопасный Dual-Boot режим в `rusty-installer` и `scripts/rustyos-install.sh` (`RUSTYOS-116`)

## Status

accepted

## Context

Первая версия установщика (`RUSTYOS-115`, ADR-0065) поддерживала только сценарий стирания всего физического диска (`sgdisk --zap-all`), что делало невозможной безопасную установку `RustyOS` рядом с уже установленными ОС (например, Windows или EndeavourOS/Linux) на одном накопителе:

1. При загрузке с Live USB разделы другой ОС на том же диске не смонтированы, поэтому простая проверка `disk.mounted` не защищала соседние разделы от полного стирания при выборе диска целиком.
2. При разделении одного диска критически важно не форматировать существующий системный EFI-раздел (`mkfs.fat`) и учитывать ограничение по размеру Windows ESP (`~100 MiB`), в который не помещаются ядро Linux и `initramfs`.

## Decision

Добавить явный режим **Dual-Boot** (`--install-mode dual-boot`) в `apps/rusty-installer` и [scripts/rustyos-install.sh](file:///home/anc/RustyOS_Main/scripts/rustyos-install.sh):

1. **Обнаружение разделов и неразмеченных областей без прав root ([disks.rs](file:///home/anc/RustyOS_Main/apps/rusty-installer/src/disks.rs)):**
   - Расширен разбор `lsblk -Jpb` с колонками `START`, `LOG-SEC`, `FSTYPE`, `PARTTYPE`, `LABEL`, `PTTYPE`, `children`: неблокирующий скан определяет EFI-разделы (`c12a7328-f81f-11d2-ba4b-00a0c93ec93b` / `vfat` ESP), существующие разделы `>= 12 GB` (блокируя смонтированные) и свободные GPT-интервалы `compute_unallocated_gaps` (`>= 12 GB`).
   - `DualBootTargetInfo::collect_from_disks` строит цели для неразмеченного пространства (`UnallocatedGap`) и уже выделенных разделов (`ExistingPartition`). Если на диске найден только маленький EFI (`< 260 MiB`, типичный Windows ESP), для `UnallocatedGap` автоматически планируется отдельный `512 MiB` `RUSTYOS_EFI` без вмешательства в существующий ESP, а при EFI `>= 260 MiB` — безопасное переиспользование без форматирования.

2. **UI шага `SelectDisk` и клавиатурная навигация ([state.rs](file:///home/anc/RustyOS_Main/apps/rusty-installer/src/state.rs), [layout.rs](file:///home/anc/RustyOS_Main/apps/rusty-installer/src/layout.rs), [render.rs](file:///home/anc/RustyOS_Main/apps/rusty-installer/src/render.rs), [wayland.rs](file:///home/anc/RustyOS_Main/apps/rusty-installer/src/wayland.rs)):**
   - Добавлен переключатель `DiskInstallMode::{WholeDisk, DualBoot}` (`Erase Entire Disk` vs `Dual-Boot (Partition / Free)`), а также стрелки `Left`/`Right` и обновлённый summary на шаге `Installing`.

3. **Безопасный backend и сохранение соседних загрузчиков ([scripts/rustyos-install.sh](file:///home/anc/RustyOS_Main/scripts/rustyos-install.sh), [packages.x86_64](file:///home/anc/RustyOS_Main/packaging/archiso/packages.x86_64)):**
   - В `--install-mode dual-boot` (с `--root-part` или `--free-space <disk:start:size>`, `--efi-part` и `--confirm-dualboot`) скрипт **никогда** не вызывает `sgdisk --zap-all`: создаёт GPT-разделы строго внутри указанного неразмеченного диапазона (определяя новые устройства по точному `start` сектору в `/sys/block`), проверяет `>= 120 MiB` свободного места при переиспользовании EFI и не выполняет `mkfs.fat` на существующем ESP.
   - В `packages.x86_64` и `pacstrap` добавлен `os-prober`; в режиме `grub` включается `GRUB_DISABLE_OS_PROBER=false`, а в `systemd-boot` в dual-boot сохраняется существующий `loader.conf` и дописывается только `entries/rustyos.conf`.

## Consequences

- Установка рядом с существующей ОС на одном накопителе безопасна как через предварительно созданный пустой раздел, так и через оставленное неразмеченное место (`Unallocated`).
- Появился явный разделённый контракт подтверждения: `--confirm-erase` для стирания диска и `--confirm-dualboot` для dual-boot без `sgdisk --zap-all`.
