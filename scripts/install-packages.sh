#!/usr/bin/env bash
set -euo pipefail

# Thin system-only installer (Phase 2 / #216).
#
# Installs ONLY the apt packages that must live in the system layer:
#   - login shell fallback (zsh) — Nix zsh is now the primary login shell (#245)
#   - build toolchain (build-essential, libudev-dev, pkg-config)
#   - cross C toolchain for embedded / C-Rust FFI
#   - fcitx5 client-side immodules (the daemon itself is home-manager's; apt
#     GTK/Qt apps can only load an immodule out of /usr/lib — see ADR-0001's
#     Amendment)
#   - smartcard support (scdaemon, direct CCID)
#
# User-space CLIs are managed by home-manager (home/modules/packages.nix).
# Usage: ./install-packages.sh [--dry-run]

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APT_FILE="${SCRIPT_DIR}/../packages/declarative/apt-packages.txt"
DRY_RUN=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --dry-run) DRY_RUN=true; shift ;;
        --help|-h)
            echo "Usage: $0 [--dry-run]"
            echo ""
            echo "Install system-layer APT packages from apt-packages.txt."
            echo "User-space CLIs are managed by home-manager — see home/modules/packages.nix."
            exit 0
            ;;
        *) echo "Error: Unknown option: $1"; exit 1 ;;
    esac
done

if [[ ! -f "$APT_FILE" ]]; then
    echo "Error: $APT_FILE not found"
    exit 1
fi

# Parse package list (skip comments and blank lines).
packages=()
while IFS= read -r line; do
    [[ "$line" =~ ^[[:space:]]*# ]] && continue
    [[ -z "$line" ]] && continue
    packages+=("$line")
done < "$APT_FILE"

if [[ ${#packages[@]} -eq 0 ]]; then
    echo "No packages to install."
    exit 0
fi

UDEV_RULES="/etc/udev/rules.d/69-probe-rs.rules"

if [[ "$DRY_RUN" == "true" ]]; then
    echo "[dry-run] Would install ${#packages[@]} system packages:"
    printf '  %s\n' "${packages[@]}"
    [[ ! -f "$UDEV_RULES" ]] && echo "[dry-run] Would install probe-rs udev rules"
    exit 0
fi

echo "Installing ${#packages[@]} system packages..."
sudo apt-get update -qq
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
    -o Dpkg::Options::=--force-confold \
    "${packages[@]}"

# probe-rs udev rules (embedded debug probes without root).
if [[ ! -f "$UDEV_RULES" ]]; then
    echo "Installing probe-rs udev rules..."
    curl -fsSL https://probe.rs/files/69-probe-rs.rules \
        | sudo tee "$UDEV_RULES" > /dev/null
    sudo udevadm control --reload-rules
    sudo udevadm trigger
fi

echo "Done."
