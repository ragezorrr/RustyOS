#!/usr/bin/env bash
# RustyOS Installation Script (`RUSTYOS-014` / `RUSTYOS-076`, ADR-0009)
set -euo pipefail

ACTION="install"
MODE="user"

case "${1:-}" in
    ""|"--user")
        MODE="user"
        ;;
    "--system")
        MODE="system"
        ;;
    "--uninstall-user")
        ACTION="uninstall"
        MODE="user"
        ;;
    "--uninstall-system")
        ACTION="uninstall"
        MODE="system"
        ;;
    *)
        echo "Usage: $0 [--user|--system|--uninstall-user|--uninstall-system]" >&2
        exit 2
        ;;
esac

if [ "${MODE}" = "user" ]; then
    PREFIX="${HOME}/.local"
    BIN_DIR="${PREFIX}/bin"
    SESSIONS_DIR="${PREFIX}/share/wayland-sessions"
    PORTALS_DIR="${XDG_DATA_HOME:-${HOME}/.local/share}/xdg-desktop-portal/portals"
    PORTALS_CONF_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/xdg-desktop-portal"
else
    PREFIX="/usr/local"
    BIN_DIR="${PREFIX}/bin"
    SESSIONS_DIR="/usr/share/wayland-sessions"
    PORTALS_DIR="/usr/share/xdg-desktop-portal/portals"
    PORTALS_CONF_DIR="/etc/xdg-desktop-portal"
fi

if [ "${ACTION}" = "uninstall" ]; then
    echo "=== Removing RustyOS (${BIN_DIR}, ${SESSIONS_DIR}) ==="
    rm -f \
        "${BIN_DIR}/rusty-compositor" \
        "${BIN_DIR}/rusty-shell" \
        "${BIN_DIR}/rusty-files" \
        "${BIN_DIR}/rusty-pipewire-stream" \
        "${BIN_DIR}/rustyos-session" \
        "${SESSIONS_DIR}/rustyos.desktop" \
        "${PORTALS_DIR}/rustyos.portal" \
        "${PORTALS_CONF_DIR}/rustyos-portals.conf"
    echo "Uninstall completed cleanly."
    exit 0
fi

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${ROOT_DIR}/target/release"

echo "=== Installing RustyOS Desktop Environment (${BIN_DIR}, ${SESSIONS_DIR}) ==="
mkdir -p "${BIN_DIR}" "${SESSIONS_DIR}" "${PORTALS_DIR}" "${PORTALS_CONF_DIR}"

# Build release artifacts unprivileged when invoked via sudo so rustup/cargo caches remain intact.
echo "Building release artifacts..."
if [ -n "${SUDO_USER:-}" ] && [ "$(id -u)" -eq 0 ]; then
    sudo -u "${SUDO_USER}" -H env "PATH=${PATH}" \
        cargo build --release --workspace --locked --manifest-path "${ROOT_DIR}/Cargo.toml"
else
    cargo build --release --workspace --locked --manifest-path "${ROOT_DIR}/Cargo.toml"
fi

echo "Installing binaries to ${BIN_DIR}..."
install -m 755 "${TARGET_DIR}/rusty-compositor" "${BIN_DIR}/rusty-compositor"
install -m 755 "${TARGET_DIR}/rusty-shell" "${BIN_DIR}/rusty-shell"
install -m 755 "${TARGET_DIR}/rusty-files" "${BIN_DIR}/rusty-files"
install -m 755 "${ROOT_DIR}/bin/rustyos-session" "${BIN_DIR}/rustyos-session"
PW_WORKER="$(ls -1t "${TARGET_DIR}"/build/rusty-services-*/out/rusty-pipewire-stream 2>/dev/null | head -n 1 || true)"
if [ -n "${PW_WORKER}" ] && [ -x "${PW_WORKER}" ]; then
    install -m 755 "${PW_WORKER}" "${BIN_DIR}/rusty-pipewire-stream"
fi

echo "Installing portal backend configuration to ${PORTALS_DIR} and ${PORTALS_CONF_DIR}..."
install -m 644 "${ROOT_DIR}/packaging/portals/rustyos.portal" "${PORTALS_DIR}/rustyos.portal"
install -m 644 "${ROOT_DIR}/packaging/portals/rustyos-portals.conf" "${PORTALS_CONF_DIR}/rustyos-portals.conf"

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
echo "Arch package alternative: cd ${ROOT_DIR}/packaging/arch && makepkg -si"
echo "For menu-bar Arch update badge, ensure 'pacman-contrib' (checkupdates) is installed."
echo "To test nested: ${BIN_DIR}/rustyos-session --winit"
echo "For bare-metal TTY test: ${BIN_DIR}/rustyos-session --drm"
