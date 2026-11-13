#!/usr/bin/env bash
# RustyOS Installation Script (`RUSTYOS-014`, ADR-0009)
set -euo pipefail

case "${1:-}" in
    ""|"--user")
        PREFIX="${HOME}/.local"
        BIN_DIR="${PREFIX}/bin"
        SESSIONS_DIR="${PREFIX}/share/wayland-sessions"
        ;;
    "--system")
        PREFIX="/usr/local"
        BIN_DIR="${PREFIX}/bin"
        SESSIONS_DIR="/usr/share/wayland-sessions"
        ;;
    *)
        echo "Usage: $0 [--user|--system]" >&2
        exit 2
        ;;
esac

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${ROOT_DIR}/target/release"

echo "=== Installing RustyOS Desktop Environment (${BIN_DIR}, ${SESSIONS_DIR}) ==="
mkdir -p "${BIN_DIR}" "${SESSIONS_DIR}"

# Build workspace release binaries
echo "Building release artifacts..."
cargo build --release --workspace --offline --manifest-path "${ROOT_DIR}/Cargo.toml"

echo "Installing binaries to ${BIN_DIR}..."
install -m 755 "${TARGET_DIR}/rusty-compositor" "${BIN_DIR}/rusty-compositor"
install -m 755 "${TARGET_DIR}/rusty-shell" "${BIN_DIR}/rusty-shell"
install -m 755 "${ROOT_DIR}/bin/rustyos-session" "${BIN_DIR}/rustyos-session"

echo "Installing session entry to ${SESSIONS_DIR}/rustyos.desktop..."
TMP_DESKTOP="$(mktemp)"
trap 'rm -f "${TMP_DESKTOP}"' EXIT
sed "s|^Exec=.*|Exec=${BIN_DIR}/rustyos-session|" \
    "${ROOT_DIR}/packaging/wayland-sessions/rustyos.desktop" > "${TMP_DESKTOP}"
install -m 644 "${TMP_DESKTOP}" "${SESSIONS_DIR}/rustyos.desktop"

echo ""
echo "=== RustyOS installation completed successfully! ==="
echo "Binaries: ${BIN_DIR}"
echo "Wayland Session: ${SESSIONS_DIR}/rustyos.desktop"
echo "To test nested: ${BIN_DIR}/rustyos-session --winit"
echo "For bare-metal TTY test: ${BIN_DIR}/rustyos-session --drm"
