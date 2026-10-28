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
#     firewall (ufw) — ADR-471
#   - system76-scheduler priority drop-in under /etc (Pop!_OS only; skipped
#     where system76-scheduler is absent) — ADR-0001 Amendment, #1056
#   - Waydroid (Android container: root, a system service, kernel binder) and
#     its apt repo, only on a host whose package list names `waydroid` — #323
#
# User-space CLIs are managed by home-manager (home/modules/packages.nix).
# Usage: ./install-packages.sh [--dry-run]

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Prefer the home-manager-deployed file (#3): it is the shared list plus
# this host's `dotfiles.apt.extraPackages` (home/modules/apt.nix), the same
# resolution order detect-drift's `apt_declared_path()` uses (deployed path
# first, checkout-relative as a bootstrap-time fallback before the first
# `hms` has ever run).
XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
DEPLOYED_APT_FILE="${XDG_CONFIG_HOME}/dotfiles/apt-packages.txt"
if [[ -f "$DEPLOYED_APT_FILE" ]]; then
    APT_FILE="$DEPLOYED_APT_FILE"
else
    APT_FILE="${SCRIPT_DIR}/../packages/declarative/apt-packages.txt"
fi
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

# system76-scheduler drop-in (ADR-0001 Amendment, #1056): the daemon reads
# /etc/system76-scheduler/process-scheduler/*.kdl after its packaged
# /usr/share/... profiles, so a file there corrects priorities without
# editing the package's own (upgrade-clobbered) files. Gated on the package's
# own config dir existing — hosts without system76-scheduler (non-Pop!_OS, CI)
# skip it — and on the deployed copy differing from the checkout's, so a
# re-run is a no-op.
SCHED_DIST_DIR="/usr/share/system76-scheduler"
SCHED_DROPIN_SRC="${SCRIPT_DIR}/../config/system76-scheduler/desktop-priority.kdl"
SCHED_DROPIN_DST="/etc/system76-scheduler/process-scheduler/desktop-priority.kdl"
sched_dropin_stale=false
if [[ -d "$SCHED_DIST_DIR" && -f "$SCHED_DROPIN_SRC" ]] \
    && ! cmp -s "$SCHED_DROPIN_SRC" "$SCHED_DROPIN_DST"; then
    sched_dropin_stale=true
fi

# Tailscale apt repo/keyring (ADR-471): `tailscale` in apt-packages.txt needs
# a third-party repo added before `apt-get update` can resolve it. Gated on
# the keyring file the same way probe-rs's udev rule is gated below — a
# fresh host installs it once, a re-run is a no-op.
TAILSCALE_KEYRING="/usr/share/keyrings/tailscale-archive-keyring.gpg"
TAILSCALE_LIST="/etc/apt/sources.list.d/tailscale.list"
tailscale_repo_new=false
[[ -f "$TAILSCALE_KEYRING" ]] || tailscale_repo_new=true

# Waydroid apt repo/keyring (#323): `waydroid` is not in Ubuntu/Pop!_OS' own
# archives, so a host that lists it (home/hosts/vega.nix) needs the project's
# repo first. Same shape as Tailscale above, but only for a host whose package
# list names `waydroid` — arcturus and CI never add it. The project's own
# installer (`curl -s https://repo.waydro.id | sudo bash`) does exactly these
# two writes (read 2026-10-28, without running it): the keyring from
# https://repo.waydro.id/waydroid.gpg and a `deb [signed-by=…] https://repo.waydro.id/
# <codename> main` line; `noble` is among the codenames it accepts.
WAYDROID_KEYRING="/usr/share/keyrings/waydroid.gpg"
WAYDROID_LIST="/etc/apt/sources.list.d/waydroid.list"
waydroid_repo_new=false
if printf '%s\n' "${packages[@]}" | grep -qx 'waydroid' && [[ ! -f "$WAYDROID_KEYRING" ]]; then
    waydroid_repo_new=true
fi

if [[ "$DRY_RUN" == "true" ]]; then
    echo "[dry-run] Would install ${#packages[@]} system packages:"
    printf '  %s\n' "${packages[@]}"
    [[ ! -f "$UDEV_RULES" ]] && echo "[dry-run] Would install probe-rs udev rules"
    if [[ "$sched_dropin_stale" == "true" ]]; then
        echo "[dry-run] Would install $SCHED_DROPIN_DST and restart com.system76.Scheduler"
    fi
    if [[ "$tailscale_repo_new" == "true" ]]; then
        echo "[dry-run] Would install Tailscale apt repo and run 'tailscale up --operator=\$USER'"
    fi
    if [[ "$waydroid_repo_new" == "true" ]]; then
        echo "[dry-run] Would install Waydroid apt repo ($WAYDROID_LIST)"
    fi
    exit 0
fi

if [[ "$waydroid_repo_new" == "true" ]]; then
    echo "Installing Waydroid apt repo..."
    # Same codename resolution as the project's installer: UBUNTU_CODENAME
    # (noble on Pop!_OS 24.04). The repo lists no Pop!_OS-specific suite.
    codename="$(. /etc/os-release && echo "${UBUNTU_CODENAME:-noble}")"
    curl -fsSL --proto '=https' --tlsv1.2 https://repo.waydro.id/waydroid.gpg \
        | sudo tee "$WAYDROID_KEYRING" > /dev/null
    echo "deb [signed-by=${WAYDROID_KEYRING}] https://repo.waydro.id/ ${codename} main" \
        | sudo tee "$WAYDROID_LIST" > /dev/null
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

# system76-scheduler priority drop-in. A restart is needed (not just `reload`):
# the daemon assigns each process once and only forgets assignments when it
# restarts, so already-running processes keep their old nice until then.
if [[ "$sched_dropin_stale" == "true" ]]; then
    echo "Installing system76-scheduler priority config..."
    sudo install -D -m 0644 "$SCHED_DROPIN_SRC" "$SCHED_DROPIN_DST"
    sudo systemctl try-restart com.system76.Scheduler.service
fi

# Bring Tailscale up once, right after its first install, so `--operator`
# is set before this host is ever used non-interactively (ADR-471). This
# blocks on a login URL the human must open in a browser — deliberately: it
# is a one-time interactive step, not something later re-runs should repeat.
if [[ "$tailscale_repo_new" == "true" ]]; then
    echo "Bringing Tailscale up — open the printed login URL in your browser to authorize this device."
    sudo tailscale up --operator="$USER"
fi

echo "Done."
