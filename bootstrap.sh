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
#   4. Build and run this host's home-manager activation package.
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

# --- 4. home-manager activation ---
HOSTNAME="$(hostname)"
info "Activating home-manager configuration for host '$HOSTNAME'..."

# Build the activation package out of this flake and run it, rather than
# `nix run home-manager -- switch` (#11). The CLI form resolves `home-manager`
# through the global flake registry — i.e. the *master* branch — while flake.nix
# pins the configuration to a release branch, so a greenfield bootstrap would
# drive a pinned config with whatever CLI master happened to be that day.
#
# The obvious fix, spelling the release branch into the `nix run` URL, just moves
# the problem: the branch name would then live in two places and one of them
# would go stale at the next channel bump. (#11's own body proposed
# `release-25.11`, which was already wrong by the time it was read — PR #17 moved
# the flake to 26.05.) Building the activation package has no version string at
# all: it uses whatever flake.lock pins, so the skew is zero by construction and
# no external fetch or registry lookup is involved.
#
# Only this first activation needs it. `programs.home-manager.enable = true` in
# home/common.nix installs the CLI from the same pinned input, so every later
# apply goes through `hms` (docs/operations.md) with no unpinned invocation left
# anywhere in the repo.
ACTIVATION_ATTR="$REPO_DIR#homeConfigurations.\"$HOSTNAME\".activationPackage"

if [[ "$DRY_RUN" == "true" ]]; then
    echo "[dry-run] Would run: nix build --no-link --print-out-paths $ACTIVATION_ATTR"
    echo "[dry-run] Would run: <result>/activate"
else
    activation_path="$(nix build --no-link --print-out-paths "$ACTIVATION_ATTR")"
    "$activation_path/activate"
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
