#!/bin/bash
set -euo pipefail

# hms — home-manager switch, the canonical apply (docs/operations.md).
#
# Usage: hms [flake-ref]
#   hms          apply pushed main (github:tarotene/dotfiles) — the default
#   hms .        apply the current checkout/worktree (pre-push verification)
#   hms <path>   apply an arbitrary local checkout
#
# One command = the whole apply runbook:
#   1. home-manager switch --flake <ref>#$(hostname) -b backup
#   2. systemctl --user daemon-reload
#   3. restart the generated fcitx5 autostart unit — a switch moves its
#      ExecStart store path, but daemon-reload alone never restarts a
#      generated unit, so the old binary would keep running
#   4. verify the unit is active with a live MainPID after the restart
#
# The default ref is the remote main so the apply never depends on which
# branch (or how dirty) any local checkout happens to be.  Applying a
# worktree is possible but only ever explicit: `hms .`.

DEFAULT_REF="github:tarotene/dotfiles"
FCITX5_UNIT="app-fcitx5@autostart.service"

ref="$DEFAULT_REF"

while [[ $# -gt 0 ]]; do
    case $1 in
        --help|-h)
            echo "Usage: hms [flake-ref]"
            echo ""
            echo "Apply the home-manager configuration for this host."
            echo "  hms          apply pushed main (${DEFAULT_REF})"
            echo "  hms .        apply the current checkout/worktree (pre-push verification)"
            echo "  hms <path>   apply an arbitrary local checkout"
            exit 0
            ;;
        -*) echo "Error: Unknown option: $1" >&2; exit 1 ;;
        *)
            ref="$1"
            shift
            ;;
    esac
done

host="$(hostname)"

echo "==> home-manager switch --flake ${ref}#${host} -b backup"
home-manager switch --flake "${ref}#${host}" -b backup

echo "==> systemctl --user daemon-reload"
systemctl --user daemon-reload

# fcitx5 unit follow-up — skipped cleanly on a host without the unit.
if ! systemctl --user cat "$FCITX5_UNIT" > /dev/null 2>&1; then
    echo "==> ${FCITX5_UNIT} not present; skipping fcitx5 restart."
    echo "Done."
    exit 0
fi

echo "==> systemctl --user restart ${FCITX5_UNIT}"
systemctl --user restart "$FCITX5_UNIT"

# Verify: the restart is synchronous and runs after daemon-reload, so an
# active unit with a live MainPID is by construction running the new
# generation's ExecStart. MainPID is authoritative — matching by process
# name (pgrep) or by exe path is not possible here: nixpkgs wraps fcitx5
# (bin/fcitx5 -> .fcitx5-wrapped -> the real binary), so the comm is
# ".fcitx5-wrapped" and /proc/<pid>/exe resolves past the wrapper the unit's
# ExecStart points at.
if ! systemctl --user is-active --quiet "$FCITX5_UNIT"; then
    echo "Error: ${FCITX5_UNIT} is not active after the restart." >&2
    systemctl --user status --no-pager "$FCITX5_UNIT" >&2 || true
    exit 1
fi

pid="$(systemctl --user show -p MainPID --value "$FCITX5_UNIT")"
if [[ -z "$pid" || "$pid" == "0" ]]; then
    echo "Error: ${FCITX5_UNIT} is active but has no MainPID." >&2
    exit 1
fi

running_bin="$(readlink -f "/proc/${pid}/exe" 2>/dev/null || true)"
echo "==> fcitx5 running (pid ${pid}) from ${running_bin:-<unknown>}"
echo "Done."
