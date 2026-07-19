#!/bin/bash
set -euo pipefail

# Bootstrap a fresh Pop!_OS / Ubuntu host from bare metal to a fully managed
# home-manager environment.  One command (plus a manual YubiKey step).
#
# Usage:
#   curl -fsSL <raw-url>/bootstrap.sh | bash          # greenfield
#   ./bootstrap.sh                                     # from a clone
#   ./bootstrap.sh --dry-run                           # preview only
#
# What it does (in order):
#   1. Install Nix via the Determinate Systems installer (multi-user default).
#   2. Clone this repo (skipped when already inside a checkout).
#   3. Install system-layer apt packages (scripts/install-packages.sh).
#   4. Run `home-manager switch --flake .#<hostname>`.
#   5. Register the Nix-provided zsh in /etc/shells (idempotent). `chsh` itself
#      stays manual — see "Next steps" — since it authenticates interactively
#      and can fail when stdin is consumed by curl|bash.
#
# After bootstrap completes you still need to:
#   - Insert the YubiKey and run `gpg --card-status` to bind the signing subkey.
#   - Update programs.git.signing.key in the host module if needed.
#
# See docs/cutover-runbook.md for the full cutover procedure on existing hosts.

REPO_URL="https://github.com/tarotene/dotfiles.git"
BRANCH="main"
CLONE_DIR="$HOME/dotfiles"
DRY_RUN=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --dry-run) DRY_RUN=true; shift ;;
        --help|-h)
            echo "Usage: $0 [--dry-run]"
            echo ""
            echo "Bootstrap a fresh host with Nix + home-manager."
            echo "Run from inside a clone or standalone (will clone for you)."
            exit 0
            ;;
        *) echo "Error: Unknown option: $1"; exit 1 ;;
    esac
done

info() { echo "==> $*"; }

# --- 1. Install Nix ---
if command -v nix &>/dev/null; then
    info "Nix already installed ($(nix --version))."
else
    info "Installing Nix via the Determinate Systems installer..."
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "[dry-run] Would run: curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install --no-confirm"
    else
        curl --proto '=https' --tlsv1.2 -sSf -L \
            https://install.determinate.systems/nix | sh -s -- install --no-confirm
        # Source nix-daemon.sh so `nix` is available in this shell.
        if [[ -e /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh ]]; then
            # shellcheck disable=SC1091
            . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh
        fi
    fi
fi

# --- 2. Ensure we are inside a repo checkout ---
if [[ -n "${BASH_SOURCE[0]:-}" ]]; then
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
else
    SCRIPT_DIR=""   # curl|bash: no script file on disk → force clone path
fi

if [[ -f "$SCRIPT_DIR/flake.nix" ]]; then
    REPO_DIR="$SCRIPT_DIR"
    info "Running from existing checkout: $REPO_DIR"
else
    REPO_DIR="$CLONE_DIR"
    if [[ -d "$REPO_DIR/.git" ]]; then
        info "Repo already cloned at $REPO_DIR; pulling latest..."
        if [[ "$DRY_RUN" == "true" ]]; then
            echo "[dry-run] Would pull $BRANCH in $REPO_DIR"
        else
            git -C "$REPO_DIR" fetch origin
            git -C "$REPO_DIR" checkout "$BRANCH"
            git -C "$REPO_DIR" pull --ff-only
        fi
    else
        info "Cloning $REPO_URL → $REPO_DIR ..."
        if [[ "$DRY_RUN" == "true" ]]; then
            echo "[dry-run] Would clone $REPO_URL into $REPO_DIR (branch $BRANCH)"
        else
            git clone -b "$BRANCH" "$REPO_URL" "$REPO_DIR"
        fi
    fi
fi

# --- 3. System-layer packages (apt) ---
info "Installing system-layer apt packages..."
if [[ "$DRY_RUN" == "true" ]]; then
    "$REPO_DIR/scripts/install-packages.sh" --dry-run
else
    "$REPO_DIR/scripts/install-packages.sh"
fi

# --- 4. home-manager switch ---
HOSTNAME="$(hostname)"
info "Activating home-manager configuration for host '$HOSTNAME'..."

if [[ "$DRY_RUN" == "true" ]]; then
    echo "[dry-run] Would run: nix run home-manager -- switch --flake $REPO_DIR#$HOSTNAME"
else
    nix run home-manager -- switch --flake "$REPO_DIR#$HOSTNAME"
fi

# --- 5. Register the Nix zsh as a valid login shell ---
NIX_ZSH="$HOME/.nix-profile/bin/zsh"
if [[ "$DRY_RUN" == "true" ]]; then
    echo "[dry-run] Would register $NIX_ZSH in /etc/shells (if not already present)"
elif [[ -x "$NIX_ZSH" ]] && ! grep -qxF "$NIX_ZSH" /etc/shells; then
    info "Registering $NIX_ZSH in /etc/shells..."
    echo "$NIX_ZSH" | sudo tee -a /etc/shells >/dev/null
fi

info "Bootstrap complete."
echo ""
echo "Next steps:"
echo "  1. Insert your YubiKey and run: gpg --card-status"
echo "  2. Verify signing key: git log --show-signature -1"
echo "  3. Set your login shell to the Nix-managed zsh, then log out and back in:"
echo "       chsh -s \"$NIX_ZSH\""
echo "  4. Or just restart your current shell: exec zsh"
