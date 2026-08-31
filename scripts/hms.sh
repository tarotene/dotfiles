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
#   4. verify the running fcitx5 binary matches the unit's new ExecStart
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

# Verify: the unit's ExecStart store path must be what is actually running.
unit_bin="$(systemctl --user show -p ExecStart --value "$FCITX5_UNIT" \
    | sed -n 's/.*path=\([^ ;]*\).*/\1/p' | head -n1)"

running_bin=""
for _ in 1 2 3 4 5; do
    pid="$(pgrep -x fcitx5 | head -n1 || true)"
    if [[ -n "$pid" ]]; then
        running_bin="$(readlink "/proc/${pid}/exe" || true)"
        [[ -n "$running_bin" ]] && break
    fi
    sleep 1
done

if [[ -z "$running_bin" ]]; then
    echo "Error: fcitx5 is not running after the restart." >&2
    echo "  unit ExecStart: ${unit_bin:-<unknown>}" >&2
    exit 1
fi

if [[ "$running_bin" != "$unit_bin" ]]; then
    echo "Error: running fcitx5 does not match the unit's ExecStart." >&2
    echo "  unit ExecStart: ${unit_bin:-<unknown>}" >&2
    echo "  running:        ${running_bin}" >&2
    exit 1
fi

echo "==> fcitx5 running from ${running_bin}"
echo "Done."
