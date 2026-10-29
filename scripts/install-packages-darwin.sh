#!/usr/bin/env bash
set -euo pipefail

# macOS system-layer installer (ADR-0018) — the darwin counterpart of
# install-packages.sh. Installs ONLY what needs macOS's own app-bundle/GUI
# registration machinery (packages/declarative/Brewfile); user-space CLIs
# stay in home-manager (home/modules/packages.nix).
#
# Also sets how long macOS keeps files in /tmp (periodic's daily
# 110.clean-tmps, default 3 days) to 14 days — the same retention as the Linux
# hosts' /etc/tmpfiles.d/tmp.conf (config/tmpfiles/tmp.conf), so an agent's
# artifacts written straight to /tmp are not swept sooner on one machine than on
# another. periodic reads /etc/periodic.conf.local after its defaults; the
# switch is a single line there, written only when that file does not exist yet
# (a file someone else wrote is never overwritten).
#
# Usage: ./install-packages-darwin.sh [--dry-run]

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BREWFILE="${SCRIPT_DIR}/../packages/declarative/Brewfile"
PERIODIC_LOCAL="/etc/periodic.conf.local"
PERIODIC_LINE='daily_clean_tmps_days="14"'
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

# write: no file yet. present: our line is already there. conflict: the file
# exists with other content — warn and leave it alone.
if [[ ! -e "$PERIODIC_LOCAL" ]]; then
    periodic_action=write
elif grep -qxF "$PERIODIC_LINE" "$PERIODIC_LOCAL"; then
    periodic_action=present
else
    periodic_action=conflict
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
    case "$periodic_action" in
        write) echo "[dry-run] Would write $PERIODIC_LINE to $PERIODIC_LOCAL (/tmp kept 14 days)" ;;
        conflict) echo "[dry-run] $PERIODIC_LOCAL exists without $PERIODIC_LINE — would leave it alone and warn" ;;
    esac
    exit 0
fi

echo "Installing packages from $BREWFILE..."
# No --cleanup: this stays additive, never removing a cask/formula a human
# installed by hand outside the declarative list (same posture as
# install-packages.sh, which never uninstalls either). Drift detection is a
# separate, explicit step — `brew bundle check --file=$BREWFILE`.
brew bundle --file="$BREWFILE"

case "$periodic_action" in
    write)
        echo "Setting /tmp retention to 14 days ($PERIODIC_LOCAL)..."
        printf '%s\n' "$PERIODIC_LINE" | sudo tee "$PERIODIC_LOCAL" > /dev/null
        ;;
    conflict)
        echo "Warning: $PERIODIC_LOCAL exists without $PERIODIC_LINE; left alone." >&2
        echo "         Add that line by hand to keep /tmp for 14 days (default is 3)." >&2
        ;;
esac

echo "Done."
