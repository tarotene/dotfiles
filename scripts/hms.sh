#!/usr/bin/env bash
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
#
# nix caches a github:-style flake ref's resolution for tarball-ttl (1h by
# default). Right after a merge, that means `hms` can silently apply an hour-old
# main and still print "Done." (#48) — so for a non-local ref (anything that
# isn't a path on disk) we force a refresh before switching, and print the
# revision actually applied so a stale apply leaves a trace instead of none.

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

# home-manager runs `nix-env --profile --set` (which advances the current
# generation) before the activation script's real work executes. If
# activation then fails partway (e.g. checkLinkTargets, #62/#63), the current
# generation points at the new store path while the actual home-files symlink
# under gcroots/current-home is still the old, successful generation — a
# "generation advanced but the world is stale" state that hms does not
# otherwise detect (#65). Warn (never fail) when the two disagree.
check_generation_consistency() {
    local profile_link="${1:-$HOME/.local/state/nix/profiles/home-manager}"
    local current_home_link="${2:-$HOME/.local/state/home-manager/gcroots/current-home}"

    [[ -e "$profile_link" && -e "$current_home_link" ]] || return 0

    local profile_target current_home_target
    profile_target="$(readlink -f "$profile_link")"
    current_home_target="$(readlink -f "$current_home_link")"

    if [[ "$profile_target" != "$current_home_target" ]]; then
        echo "Warning: home-manager generation/reality mismatch detected." >&2
        echo "  profile (${profile_link}): ${profile_target}" >&2
        echo "  current-home (${current_home_link}): ${current_home_target}" >&2
        echo "  A previous activation likely failed partway through, leaving the" >&2
        echo "  generation pointer ahead of what is actually applied. A successful" >&2
        echo "  switch (this one) will resolve it." >&2
    fi
}

check_generation_consistency

# Remote flake refs (github:, git+ssh:, ...) are the ones nix caches; a local
# path (`.` or a checkout directory) always reads the current tree, so there is
# nothing to refresh.
if [[ ! -e "$ref" ]]; then
    echo "==> nix flake metadata --refresh ${ref}"
    if revision="$(nix flake metadata --refresh --json "$ref" 2>/dev/null | jq -r '.revision // empty')" && [[ -n "$revision" ]]; then
        echo "==> applying revision ${revision}"
    else
        echo "==> could not resolve a revision for ${ref} (offline?); continuing with whatever switch resolves" >&2
    fi
fi

echo "==> home-manager switch --flake ${ref}#${host} -b backup"
rc=0
home-manager switch --flake "${ref}#${host}" -b backup || rc=$?
if [[ $rc -ne 0 ]]; then
    check_generation_consistency
    exit "$rc"
fi

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
