#!/usr/bin/env bash
# Reproducible builder wrapper for RustyOS Live ISO (`RUSTYOS-115`, ADR-0065)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORK_DIR="/tmp/rustyos-archiso-work"
OUT_DIR="/tmp/rustyos-archiso-out"
LOCAL_REPO_DIR=""

usage() {
    cat <<'EOF'
Usage: build-iso.sh [--work-dir <dir>] [--out-dir <dir>] [--local-repo-dir <dir>]
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --help|-h)
            usage
            exit 0
            ;;
        --work-dir)
            WORK_DIR="${2:-}"
            shift 2
            ;;
        --out-dir)
            OUT_DIR="${2:-}"
            shift 2
            ;;
        --local-repo-dir)
            LOCAL_REPO_DIR="${2:-}"
            shift 2
            ;;
        *)
            echo "Error: unknown argument '$1'" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ "$(id -u)" -ne 0 ]]; then
    echo "Error: mkarchiso requires root privileges" >&2
    exit 1
fi
if ! command -v mkarchiso >/dev/null 2>&1; then
    echo "Error: mkarchiso not found (install archiso package on Arch Linux)" >&2
    exit 1
fi

PROFILE_STAGE="$(mktemp -d)"
trap 'rm -rf "${PROFILE_STAGE}"' EXIT
cp -a "${SCRIPT_DIR}/." "${PROFILE_STAGE}/"

if [[ -n "${LOCAL_REPO_DIR}" ]]; then
    if [[ ! -d "${LOCAL_REPO_DIR}" ]]; then
        echo "Error: --local-repo-dir '${LOCAL_REPO_DIR}' does not exist" >&2
        exit 2
    fi
    LOCAL_PKG_STAGE="${PROFILE_STAGE}/local-repo"
    mkdir -p "${LOCAL_PKG_STAGE}"
    for pkg in "${LOCAL_REPO_DIR}"/rustyos-git-*.pkg.tar.*; do
        [[ -e "${pkg}" ]] || continue
        case "${pkg}" in
            *-debug-*.pkg.tar.*|*.sig) continue ;;
        esac
        cp -f "${pkg}" "${LOCAL_PKG_STAGE}/"
    done
    repo-add "${LOCAL_PKG_STAGE}/rustyos-local.db.tar.zst" "${LOCAL_PKG_STAGE}"/rustyos-git-*.pkg.tar.*
    awk -v repo_path="${LOCAL_PKG_STAGE}" '
        /^\[rustyos\]/ {
            print "[rustyos-local]"
            print "SigLevel = Optional TrustAll"
            print "Server = file://" repo_path
            print ""
        }
        { print }
    ' "${SCRIPT_DIR}/pacman.conf" > "${PROFILE_STAGE}/pacman.conf"

    mkdir -p "${PROFILE_STAGE}/airootfs/opt/rustyos-repo"
    cp -a "${LOCAL_PKG_STAGE}/." "${PROFILE_STAGE}/airootfs/opt/rustyos-repo/"
    cat <<'EOF' > "${PROFILE_STAGE}/airootfs/etc/pacman-rustyos-install.conf"
[options]
HoldPkg     = pacman glibc
Architecture = auto
SigLevel    = Required DatabaseOptional
LocalFileSigLevel = Optional

[rustyos-local]
SigLevel = Optional TrustAll
Server = file:///opt/rustyos-repo

[core]
Include = /etc/pacman.d/mirrorlist

[extra]
Include = /etc/pacman.d/mirrorlist

[rustyos]
SigLevel = Optional TrustAll
Server = https://ragezorrr.github.io/RustyOS/repo/$arch
EOF
fi

mkdir -p "${WORK_DIR}" "${OUT_DIR}"
mkarchiso -v -w "${WORK_DIR}" -o "${OUT_DIR}" "${PROFILE_STAGE}"
ls -lh "${OUT_DIR}"/*.iso
