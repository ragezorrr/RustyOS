#!/usr/bin/env bash
# Automated QEMU/KVM boot smoke-test for RustyOS Arch Live ISO.
set -euo pipefail

ISO_PATH=""
TIMEOUT_SEC=120
MEMORY_MB=2048
CPUS=2
USE_UEFI=1
DRY_RUN=0
LOG_FILE="/tmp/rustyos-qemu-smoke.log"

usage() {
    cat <<'EOF'
Usage: qemu-iso-smoke.sh --iso <path/to/rustyos-live.iso> [OPTIONS]

Options:
  --iso <path>        Path to the RustyOS Live ISO image (required unless --dry-run)
  --timeout <sec>     Maximum boot wait timeout in seconds (default: 120)
  --memory <mb>       Guest RAM in MiB (default: 2048)
  --cpus <n>          Guest vCPU count (default: 2)
  --bios              Boot legacy BIOS instead of UEFI/OVMF
  --log <path>        Serial console log path (default: /tmp/rustyos-qemu-smoke.log)
  --dry-run           Validate arguments and print the constructed QEMU command without booting
  -h, --help          Show this help message
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --iso)
            ISO_PATH="${2:-}"
            shift 2
            ;;
        --timeout)
            TIMEOUT_SEC="${2:-120}"
            shift 2
            ;;
        --memory)
            MEMORY_MB="${2:-2048}"
            shift 2
            ;;
        --cpus)
            CPUS="${2:-2}"
            shift 2
            ;;
        --bios)
            USE_UEFI=0
            shift
            ;;
        --log)
            LOG_FILE="${2:-/tmp/rustyos-qemu-smoke.log}"
            shift 2
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Error: unknown argument '$1'" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ -z "${ISO_PATH}" ]]; then
    if [[ "${DRY_RUN}" -eq 1 ]]; then
        ISO_PATH="/tmp/rustyos-live-dry-run.iso"
    else
        echo "Error: --iso <path> is required" >&2
        usage >&2
        exit 2
    fi
fi

if [[ "${DRY_RUN}" -eq 0 && ! -f "${ISO_PATH}" ]]; then
    echo "Error: ISO file not found: ${ISO_PATH}" >&2
    exit 2
fi

QEMU_BIN="${QEMU_BIN:-qemu-system-x86_64}"
if [[ "${DRY_RUN}" -eq 0 ]] && ! command -v "${QEMU_BIN}" >/dev/null 2>&1; then
    echo "Error: ${QEMU_BIN} not found (install qemu-full or qemu-system-x86)" >&2
    exit 1
fi

ACCEL_ARGS=("-accel" "tcg")
if [[ -r /dev/kvm && -w /dev/kvm ]]; then
    ACCEL_ARGS=("-enable-kvm" "-cpu" "host")
fi

UEFI_ARGS=()
if [[ "${USE_UEFI}" -eq 1 ]]; then
    for candidate in \
        /usr/share/edk2/x64/OVMF_CODE.4m.fd \
        /usr/share/edk2/x64/OVMF_CODE.fd \
        /usr/share/OVMF/OVMF_CODE.fd; do
        if [[ -f "${candidate}" ]]; then
            UEFI_ARGS=("-drive" "if=pflash,format=raw,readonly=on,file=${candidate}")
            break
        fi
    done
fi

QEMU_CMD=(
    "${QEMU_BIN}"
    "${ACCEL_ARGS[@]}"
    -m "${MEMORY_MB}"
    -smp "${CPUS}"
    -cdrom "${ISO_PATH}"
    -boot d
    -display none
    -vga virtio
    -serial "file:${LOG_FILE}"
    -monitor none
    -no-reboot
    "${UEFI_ARGS[@]}"
)

if [[ "${DRY_RUN}" -eq 1 ]]; then
    echo "Dry-run QEMU command:"
    printf ' %q' "${QEMU_CMD[@]}"
    echo
    exit 0
fi

rm -f "${LOG_FILE}"
touch "${LOG_FILE}"

echo "Booting RustyOS Live ISO in QEMU (timeout: ${TIMEOUT_SEC}s, log: ${LOG_FILE})..."
"${QEMU_CMD[@]}" &
QEMU_PID=$!
trap 'kill "${QEMU_PID}" >/dev/null 2>&1 || true; wait "${QEMU_PID}" >/dev/null 2>&1 || true' EXIT

DEADLINE=$((SECONDS + TIMEOUT_SEC))
BOOT_OK=0
while (( SECONDS < DEADLINE )); do
    if ! kill -0 "${QEMU_PID}" 2>/dev/null; then
        echo "Error: QEMU exited prematurely before boot markers appeared" >&2
        tail -n 40 "${LOG_FILE}" >&2 || true
        exit 1
    fi
    if grep -qE "Reached target Graphical Interface|greetd|rusty-greeter|rusty-compositor|rustyos login:" "${LOG_FILE}" 2>/dev/null; then
        BOOT_OK=1
        break
    fi
    sleep 2
done

if [[ "${BOOT_OK}" -eq 1 ]]; then
    echo "SUCCESS: RustyOS Live ISO reached graphical/login boot stage."
    exit 0
fi

echo "Error: timed out after ${TIMEOUT_SEC}s waiting for RustyOS boot markers." >&2
tail -n 40 "${LOG_FILE}" >&2 || true
exit 1
