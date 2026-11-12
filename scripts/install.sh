#!/usr/bin/env bash
# RustyOS Installation Script (`RUSTYOS-014`, ADR-0009)
set -euo pipefail

PREFIX="${HOME}/.local"
if [ "${1:-}" = "--system" ]; then
    PREFIX="/usr/local"
fi

BIN_DIR="${PREFIX}/bin"
SESSIONS_DIR="${PREFIX}/share/wayland-sessions"

echo "=== Installing RustyOS Desktop Environment to ${PREFIX} ==="
mkdir -p "${BIN_DIR}" "${SESSIONS_DIR}"

# Build workspace release binaries
echo "Building release artifacts..."
cargo build --release --workspace --offline

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${ROOT_DIR}/target/release"

echo "Installing binaries to ${BIN_DIR}..."
install -m 755 "${TARGET_DIR}/rusty-compositor" "${BIN_DIR}/rusty-compositor"
install -m 755 "${TARGET_DIR}/rusty-shell" "${BIN_DIR}/rusty-shell"
install -m 755 "${ROOT_DIR}/bin/rustyos-session" "${BIN_DIR}/rustyos-session"

echo "Installing session entry to ${SESSIONS_DIR}..."
install -m 644 "${ROOT_DIR}/packaging/wayland-sessions/rustyos.desktop" "${SESSIONS_DIR}/rustyos.desktop"

echo ""
echo "=== RustyOS installation completed successfully! ==="
echo "Binaries: ${BIN_DIR}"
echo "Wayland Session: ${SESSIONS_DIR}/rustyos.desktop"
echo "To test immediately: ${BIN_DIR}/rustyos-session"
