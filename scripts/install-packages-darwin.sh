#!/usr/bin/env bash
set -euo pipefail

# macOS system-layer installer (ADR-0018) — the darwin counterpart of
# install-packages.sh. Installs ONLY what needs macOS's own app-bundle/GUI
# registration machinery (packages/declarative/Brewfile); user-space CLIs
# stay in home-manager (home/modules/packages.nix).
#
# Usage: ./install-packages-darwin.sh [--dry-run]

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BREWFILE="${SCRIPT_DIR}/../packages/declarative/Brewfile"
DRY_RUN=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --dry-run) DRY_RUN=true; shift ;;
        --help|-h)
            echo "Usage: $0 [--dry-run]"
            echo ""
            echo "Install system-layer Homebrew packages from Brewfile."
            echo "User-space CLIs are managed by home-manager — see home/modules/packages.nix."
            exit 0
            ;;
        *) echo "Error: Unknown option: $1"; exit 1 ;;
    esac
done

if [[ ! -f "$BREWFILE" ]]; then
    echo "Error: $BREWFILE not found"
    exit 1
fi

if command -v brew &>/dev/null; then
    echo "Homebrew already installed ($(brew --version | head -1))."
else
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "[dry-run] Would install Homebrew via the official installer."
    else
        echo "Installing Homebrew..."
        NONINTERACTIVE=1 /bin/bash -c \
            "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
        # Apple Silicon prefix — load brew into this shell.
        if [[ -x /opt/homebrew/bin/brew ]]; then
            eval "$(/opt/homebrew/bin/brew shellenv)"
        fi
    fi
fi

if [[ "$DRY_RUN" == "true" ]]; then
    echo "[dry-run] Would run: brew bundle --file=$BREWFILE"
    exit 0
fi

echo "Installing packages from $BREWFILE..."
# No --cleanup: this stays additive, never removing a cask/formula a human
# installed by hand outside the declarative list (same posture as
# install-packages.sh, which never uninstalls either). Drift detection is a
# separate, explicit step — `brew bundle check --file=$BREWFILE`.
brew bundle --file="$BREWFILE"

echo "Done."
