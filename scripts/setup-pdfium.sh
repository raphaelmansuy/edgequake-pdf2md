#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# setup-pdfium.sh — Download the correct libpdfium for your platform
# ─────────────────────────────────────────────────────────────────────────────
# Usage:
#   curl -sSf https://raw.githubusercontent.com/raphaelmansuy/edgequake-pdf2md/main/scripts/setup-pdfium.sh | bash
#   # or
#   ./scripts/setup-pdfium.sh [--install-dir /path/to/dir]
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

PDFIUM_VERSION="chromium/7690"
PDFIUM_BASE_URL="https://github.com/bblanchon/pdfium-binaries/releases/download"

# ── Colours ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

info()  { printf "${CYAN}▸${RESET} %s\n" "$*"; }
ok()    { printf "${GREEN}✓${RESET} %s\n" "$*"; }
warn()  { printf "${YELLOW}⚠${RESET} %s\n" "$*"; }
err()   { printf "${RED}✗${RESET} %s\n" "$*" >&2; }

# ── Parse args ───────────────────────────────────────────────────────────────
INSTALL_DIR="${1:-$(pwd)}"
if [[ "${1:-}" == "--install-dir" ]]; then
    INSTALL_DIR="${2:-.}"
    shift 2
fi

# ── Detect OS + arch ────────────────────────────────────────────────────────
detect_platform() {
    local os arch asset lib_name lib_subpath

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Darwin)
            lib_name="libpdfium.dylib"
            lib_subpath="lib/libpdfium.dylib"
            case "$arch" in
                arm64|aarch64) asset="pdfium-mac-arm64.tgz" ;;
                x86_64)        asset="pdfium-mac-x64.tgz" ;;
                *)             err "Unsupported macOS architecture: $arch"; exit 1 ;;
            esac
            ;;
        Linux)
            lib_name="libpdfium.so"
            lib_subpath="lib/libpdfium.so"
            case "$arch" in
                x86_64|amd64)  asset="pdfium-linux-x64.tgz" ;;
                aarch64|arm64) asset="pdfium-linux-arm64.tgz" ;;
                armv7l|armhf)  asset="pdfium-linux-arm.tgz" ;;
                *)             err "Unsupported Linux architecture: $arch"; exit 1 ;;
            esac
            ;;
        MINGW*|MSYS*|CYGWIN*|Windows_NT)
            lib_name="pdfium.dll"
            lib_subpath="bin/pdfium.dll"
            case "$arch" in
                x86_64|AMD64)  asset="pdfium-win-x64.tgz" ;;
                x86|i686)      asset="pdfium-win-x86.tgz" ;;
                aarch64|arm64) asset="pdfium-win-arm64.tgz" ;;
                *)             err "Unsupported Windows architecture: $arch"; exit 1 ;;
            esac
            ;;
        *)
            err "Unsupported OS: $os"
            exit 1
            ;;
    esac

    echo "$asset|$lib_name|$lib_subpath"
}

# ── Main ─────────────────────────────────────────────────────────────────────
main() {
    printf "\n${BOLD}setup-pdfium${RESET} — Automatic pdfium library installer\n\n"

    local platform_info asset lib_name lib_subpath
    platform_info="$(detect_platform)"
    asset="$(echo "$platform_info" | cut -d'|' -f1)"
    lib_name="$(echo "$platform_info" | cut -d'|' -f2)"
    lib_subpath="$(echo "$platform_info" | cut -d'|' -f3)"

    local url="${PDFIUM_BASE_URL}/$(echo "$PDFIUM_VERSION" | sed 's|/|%2F|g')/${asset}"
    local dest="${INSTALL_DIR}/${lib_name}"

    info "OS:       $(uname -s)"
    info "Arch:     $(uname -m)"
    info "Asset:    ${asset}"
    info "Version:  ${PDFIUM_VERSION}"
    info "Target:   ${dest}"
    echo

    # Check if already present
    if [[ -f "$dest" ]]; then
        local size
        size="$(ls -lh "$dest" | awk '{print $5}')"
        ok "pdfium already installed: ${dest} (${size})"
        printf "\n  To force re-download, remove the file first:\n"
        printf "  ${CYAN}rm ${dest} && ./scripts/setup-pdfium.sh${RESET}\n\n"
        exit 0
    fi

    # Download
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    local tmp_tgz="${tmp_dir}/${asset}"

    info "Downloading ${asset}..."
    if command -v curl &>/dev/null; then
        curl -fSL "$url" -o "$tmp_tgz"
    elif command -v wget &>/dev/null; then
        wget -q "$url" -O "$tmp_tgz"
    else
        err "Neither curl nor wget found. Please install one."
        exit 1
    fi

    # Extract
    info "Extracting ${lib_name}..."
    tar -xzf "$tmp_tgz" -C "$tmp_dir" "$lib_subpath"

    # Install
    mkdir -p "$INSTALL_DIR"
    mv "${tmp_dir}/${lib_subpath}" "$dest"

    # Cleanup
    rm -rf "$tmp_dir"

    ok "Installed: ${dest}"
    echo

    # Platform-specific hints
    case "$(uname -s)" in
        Darwin)
            printf "  ${BOLD}macOS hint:${RESET} Set library path before running pdf2md:\n"
            printf "  ${CYAN}export DYLD_LIBRARY_PATH=\"\$(pwd)\"${RESET}\n"
            printf "  ${CYAN}pdf2md document.pdf${RESET}\n\n"
            ;;
        Linux)
            printf "  ${BOLD}Linux hint:${RESET} Set library path before running pdf2md:\n"
            printf "  ${CYAN}export LD_LIBRARY_PATH=\"\$(pwd)\"${RESET}\n"
            printf "  ${CYAN}pdf2md document.pdf${RESET}\n\n"
            printf "  Or install system-wide:\n"
            printf "  ${CYAN}sudo mv ${dest} /usr/local/lib/ && sudo ldconfig${RESET}\n\n"
            ;;
        MINGW*|MSYS*|CYGWIN*|Windows_NT)
            printf "  ${BOLD}Windows hint:${RESET} Place pdfium.dll in the same directory as pdf2md.exe\n"
            printf "  or add its location to your PATH.\n\n"
            ;;
    esac
}

main "$@"
