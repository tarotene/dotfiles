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
#   - mesh VPN daemon (tailscaled, root systemd system service) + host
#     firewall (ufw) — ADR-0000
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

# Tailscale apt repo/keyring (ADR-0000): `tailscale` in apt-packages.txt needs
# a third-party repo added before `apt-get update` can resolve it. Gated on
# the keyring file the same way probe-rs's udev rule is gated below — a
# fresh host installs it once, a re-run is a no-op.
TAILSCALE_KEYRING="/usr/share/keyrings/tailscale-archive-keyring.gpg"
TAILSCALE_LIST="/etc/apt/sources.list.d/tailscale.list"
tailscale_repo_new=false
[[ -f "$TAILSCALE_KEYRING" ]] || tailscale_repo_new=true

if [[ "$DRY_RUN" == "true" ]]; then
    echo "[dry-run] Would install ${#packages[@]} system packages:"
    printf '  %s\n' "${packages[@]}"
    [[ ! -f "$UDEV_RULES" ]] && echo "[dry-run] Would install probe-rs udev rules"
    if [[ "$tailscale_repo_new" == "true" ]]; then
        echo "[dry-run] Would install Tailscale apt repo and run 'tailscale up --operator=\$USER'"
    fi
    exit 0
fi

if [[ "$tailscale_repo_new" == "true" ]]; then
    echo "Installing Tailscale apt repo..."
    # Pop!_OS is Ubuntu-based and sets UBUNTU_CODENAME in /etc/os-release
    # (e.g. jammy on 22.04); Tailscale does not publish a Pop!_OS-specific
    # repo, but the Ubuntu one works (confirmed: pkgs.tailscale.com only
    # lists distro/release pairs, no Pop!_OS entry).
    codename="$(. /etc/os-release && echo "${UBUNTU_CODENAME:-jammy}")"
    curl -fsSL "https://pkgs.tailscale.com/stable/ubuntu/${codename}.noarmor.gpg" \
        | sudo tee "$TAILSCALE_KEYRING" > /dev/null
    curl -fsSL "https://pkgs.tailscale.com/stable/ubuntu/${codename}.tailscale-keyring.list" \
        | sudo tee "$TAILSCALE_LIST" > /dev/null
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

# Bring Tailscale up once, right after its first install, so `--operator`
# is set before this host is ever used non-interactively (ADR-0000). This
# blocks on a login URL the human must open in a browser — deliberately: it
# is a one-time interactive step, not something later re-runs should repeat.
if [[ "$tailscale_repo_new" == "true" ]]; then
    echo "Bringing Tailscale up — open the printed login URL in your browser to authorize this device."
    sudo tailscale up --operator="$USER"
fi

echo "Done."
