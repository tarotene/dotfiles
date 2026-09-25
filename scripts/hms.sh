#!/usr/bin/env bash
set -euo pipefail

# hms — home-manager switch, the canonical apply (docs/operations.md).
#
# Usage: hms [flake-ref]
#   hms          apply pushed main (github:tarotene/dotfiles), or a private
#                wrapper flake instead if one is registered (see
#                resolve_default_ref below, ADR-0034)
#   hms .        apply the current checkout/worktree (pre-push verification)
#   hms <path>   apply an arbitrary local checkout
#   hms --selftest   run the offline, network-free unit tests below and exit
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
#
# `hms .` degrades on a host with a registered private wrapper flake
# (ADR-0034): it applies this PUBLIC worktree alone, so every private value
# module the wrapper flake adds is dropped for that one apply. hms itself
# takes only a single flake-ref argument (no flag passthrough), so verifying
# a worktree together with the private values needs a direct call instead:
#   home-manager switch --flake <private-hub-ref>#$(hostname) \
#     --override-input dotfiles path:$PWD -b backup
#
# Applying a wrapper flake has the same #48-shaped staleness problem one
# layer down: the wrapper's own flake.lock pins `dotfiles` to whatever rev it
# was last bumped to, and #48's refresh only re-resolves the wrapper's own
# ref, never the `dotfiles` input inside its lock. A dotfiles-side merge
# (e.g. a retired permission rule) can land, `hms` can run right after and
# still print "Done.", yet the wrapper's stale lock silently re-applies the
# pre-merge dotfiles. So whenever the flake being applied has a `dotfiles`
# input in its lock (a wrapper, whether remote or local), hms overrides it
# with pushed main's resolved revision instead of trusting the lock — see
# ADR-0034 Amendment (2026-09-25 — hms overrides the wrapper's dotfiles
# input). The wrapper's own `flake.lock` entry for `dotfiles` is then only a
# hygiene pin for `nix flake check`, not what gets applied.

DEFAULT_REF="github:tarotene/dotfiles"
FCITX5_UNIT="app-fcitx5@autostart.service"

# Reads a `nix flake metadata --json` document on stdin and, if its root
# flake's `dotfiles` input is a plain (non-follows) input — i.e. the flake
# being applied is a private wrapper flake per ADR-0034 — prints the locked
# rev that input is currently pinned to. Prints nothing (rc 0) for a flake
# with no `dotfiles` input, or one expressed as a `follows` chain (a JSON
# array rather than a node-name string): neither case is a wrapper flake's
# own direct pin, so there is nothing to override.
wrapper_locked_dotfiles_rev() {
    jq -r '
        (.locks.nodes.root.inputs.dotfiles // empty) as $node
        | if ($node | type) != "string" then empty
          else (.locks.nodes[$node].locked.rev // empty)
          end
    '
}

# Prints, one per line, the `home-manager switch` arguments that pin the
# `dotfiles` input to pushed main's resolved revision ($1) instead of
# whatever the wrapper's own lock says. Prints nothing (rc 0) when $1 is
# empty — the caller only reaches for this once it has already resolved a
# revision to pin, so an empty argument means "nothing to override".
dotfiles_override_opts() { # $1=dotfiles revision to pin (pushed main's HEAD)
    local main_rev="${1:-}"
    [[ -n "$main_rev" ]] || return 0
    printf '%s\n' \
        --override-input dotfiles "github:tarotene/dotfiles/${main_rev}" \
        --no-write-lock-file
}

selftest() {
    local fails=0

    check_rev() { # $1=名前 $2=metadata JSON $3=期待する出力
        local name="$1" json="$2" want="$3" got
        got="$(printf '%s' "$json" | wrapper_locked_dotfiles_rev)"
        if [[ "$got" == "$want" ]]; then
            echo "ok   $name"
        else
            echo "FAIL $name (want '$want' got '$got')" >&2
            fails=$((fails + 1))
        fi
    }

    check_opts() { # $1=名前 $2=main_rev $3=期待する行数
        local name="$1" main_rev="$2" want_lines="$3" got_lines
        got_lines="$(dotfiles_override_opts "$main_rev" | wc -l | tr -d ' ')"
        if [[ "$got_lines" == "$want_lines" ]]; then
            echo "ok   $name"
        else
            echo "FAIL $name (want $want_lines lines got $got_lines)" >&2
            fails=$((fails + 1))
        fi
    }

    echo "wrapper_locked_dotfiles_rev:"
    check_rev "1 dotfiles input present (a wrapper flake)" \
        '{"locks":{"nodes":{
            "root":{"inputs":{"dotfiles":"dotfiles","nixpkgs":"nixpkgs"}},
            "dotfiles":{"locked":{"rev":"abc123def456","type":"github"}}
        }}}' \
        "abc123def456"
    check_rev "2 no dotfiles input (dotfiles applied directly)" \
        '{"locks":{"nodes":{
            "root":{"inputs":{"nixpkgs":"nixpkgs"}}
        }}}' \
        ""
    check_rev "3 dotfiles input is a follows array" \
        '{"locks":{"nodes":{
            "root":{"inputs":{"dotfiles":["nixpkgs","dotfiles"]}}
        }}}' \
        ""

    echo "dotfiles_override_opts:"
    check_opts "4 non-empty revision -> 4 args" \
        "90db04baaa54c598a2b5ba847adbb9451d2bc798" 4
    check_opts "5 empty revision -> no output" "" 0

    if [[ $fails -ne 0 ]]; then
        return 1
    fi
    echo "hms.sh: OK"
    return 0
}

if [[ "${1:-}" == "--selftest" ]]; then
    selftest
    exit $?
fi

# Resolve the default flake ref: a marker file first, DEFAULT_REF as fallback
# (ADR-0034, same indirection type as resolve_host below and docs/claude/
# writing-style.md's style-hub marker). The marker lets one host point `hms`
# (with no explicit ref) at a private wrapper flake that layers private
# value modules on top of this repo's host modules, without this repo's
# source ever naming that flake — declaring the path in home-manager would
# put the same absolute path back into a managed (store-symlinked) file,
# defeating the indirection, so this marker is hand-placed and never
# home-manager-managed.
resolve_default_ref() {
    local marker="${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/private-hub" r
    if [[ -r "$marker" ]]; then
        r="$(head -n1 "$marker" | tr -d '[:space:]')"
        if [[ -n "$r" ]]; then
            echo "$r"
            return
        fi
    fi
    echo "$DEFAULT_REF"
}

ref="$(resolve_default_ref)"

while [[ $# -gt 0 ]]; do
    case $1 in
        --help|-h)
            echo "Usage: hms [flake-ref]"
            echo ""
            echo "Apply the home-manager configuration for this host."
            echo "  hms          apply pushed main (${ref})"
            echo "  hms .        apply the current checkout/worktree (pre-push verification;"
            echo "               drops private value modules if ${ref} is a private wrapper flake)"
            echo "  hms <path>   apply an arbitrary local checkout"
            echo "  hms --selftest   run offline unit tests and exit"
            exit 0
            ;;
        -*) echo "Error: Unknown option: $1" >&2; exit 1 ;;
        *)
            ref="$1"
            shift
            ;;
    esac
done

# Resolve the logical host name: a marker file first, `hostname` as fallback
# (ADR-0019). The marker lets a host carry a star-codename (e.g. "altair",
# "vega"); resolution never depends on the OS hostname. On Linux the rename
# runbook (docs/cutover-runbook.md, "Renaming an existing host to a star
# codename") sets the OS hostname to the same star-codename, so the
# `hostname` fallback resolves correctly even with no marker placed yet; each
# star-codename host module then declares its own marker via `xdg.configFile`
# on that same switch, and the marker is home-manager-managed from then on.
resolve_host() {
    local marker="${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/host" h
    if [[ -r "$marker" ]]; then
        h="$(head -n1 "$marker" | tr -d '[:space:]')"
        if [[ -n "$h" ]]; then
            echo "$h"
            return
        fi
    fi
    hostname
}

host="$(resolve_host)"

# home-manager runs `nix-env --profile --set` (which advances the current
# generation) before the activation script's real work executes. If
# activation then fails partway (e.g. checkLinkTargets, #62/#63), the current
# generation points at the new store path while the actual home-files symlink
# under gcroots/current-home is still the old, successful generation — a
# "generation advanced but the world is stale" state that hms does not
# otherwise detect (#65). Warn (never fail) when the two disagree.
# herdr server staleness check (warn-only, #200): a switch replaces the
# `herdr` store path but never restarts the running `herdr server` — it keeps
# the old binary loaded in memory until someone kills and relaunches it
# (docs/operations.md, "Restarting herdr after a switch..."). This has no
# in-band way to fail loudly (hms usually runs *inside* a herdr pane, so hms
# cannot restart its own host process), so it only ever warns, matching
# check_generation_consistency's warn-but-never-fail shape.
check_herdr_staleness() {
    command -v herdr > /dev/null 2>&1 || return 0
    local current_bin running_pid running_bin
    current_bin="$(readlink -f "$(command -v herdr)" 2>/dev/null || true)"
    running_pid="$(pgrep -x herdr | head -n1 || true)"
    [[ -n "$current_bin" && -n "$running_pid" ]] || return 0
    running_bin="$(readlink -f "/proc/${running_pid}/exe" 2>/dev/null || true)"
    [[ -n "$running_bin" ]] || return 0

    # Only compare when both sides resolve into the nix store — anything else
    # (a wrapper script, a non-nix install) is not a staleness signal here.
    case "$current_bin:$running_bin" in
        /nix/store/*:/nix/store/*) : ;;
        *) return 0 ;;
    esac

    if [[ "$current_bin" != "$running_bin" ]]; then
        echo "Warning: the running herdr server is still on the old binary." >&2
        echo "  current generation: ${current_bin}" >&2
        echo "  running (pid ${running_pid}): ${running_bin}" >&2
        echo "  Any change to herdr's server-side behavior will not take effect" >&2
        echo "  until you kill and relaunch it from outside herdr (see" >&2
        echo "  docs/operations.md, 'Restarting herdr after a switch...')." >&2
    fi
}

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

extra_opts=()

# Remote flake refs (github:, git+ssh:, ...) are the ones nix caches; a local
# path (`.` or a checkout directory) always reads the current tree, so there is
# nothing to refresh.
ref_meta_json=""
if [[ ! -e "$ref" ]]; then
    echo "==> nix flake metadata --refresh ${ref}"
    if ref_meta_json="$(nix flake metadata --refresh --json "$ref" 2>/dev/null)" \
        && ref_revision="$(printf '%s' "$ref_meta_json" | jq -r '.revision // empty')" \
        && [[ -n "$ref_revision" ]]; then
        echo "==> applying revision ${ref_revision}"
    else
        echo "==> could not resolve a revision for ${ref} (offline?); continuing with whatever switch resolves" >&2
        ref_meta_json=""
    fi
else
    # `hms .` applies a local checkout/worktree whose git tree is routinely
    # dirty mid-session — that is the entire point of pre-push verification.
    # nix would otherwise repeat "warning: Git tree '<path>' has uncommitted
    # changes" on every switch. Suppress it only on this local-path branch,
    # not machine-wide via nix.conf (#149) — a non-local ref never triggers it.
    extra_opts=(--option warn-dirty false)
    ref_meta_json="$(nix flake metadata --json "$ref" 2>/dev/null || true)"
fi

# Wrapper-lock override (ADR-0034 Amendment, 2026-09-25): if the flake being
# applied has a `dotfiles` input (a wrapper, remote or local), pin it to
# pushed main's resolved revision instead of trusting the wrapper's lock.
if [[ -n "$ref_meta_json" ]]; then
    locked_dotfiles_rev="$(printf '%s' "$ref_meta_json" | wrapper_locked_dotfiles_rev)"
    if [[ -n "$locked_dotfiles_rev" ]]; then
        if dotfiles_meta_json="$(nix flake metadata --refresh --json "$DEFAULT_REF" 2>/dev/null)" \
            && dotfiles_main_rev="$(printf '%s' "$dotfiles_meta_json" | jq -r '.revision // empty')" \
            && [[ -n "$dotfiles_main_rev" ]]; then
            if [[ "$dotfiles_main_rev" == "$locked_dotfiles_rev" ]]; then
                echo "==> dotfiles revision ${dotfiles_main_rev} (wrapper's lock already at this revision)"
            else
                echo "==> dotfiles revision ${dotfiles_main_rev} (overriding the wrapper's lock, which pins ${locked_dotfiles_rev})"
            fi
            while IFS= read -r opt; do
                extra_opts+=("$opt")
            done < <(dotfiles_override_opts "$dotfiles_main_rev")
        else
            echo "==> could not resolve ${DEFAULT_REF} (offline?); applying the wrapper's lock as-is (dotfiles ${locked_dotfiles_rev})" >&2
        fi
    fi
fi

echo "==> home-manager switch --flake ${ref}#${host} -b backup"
rc=0
home-manager switch --flake "${ref}#${host}" -b backup "${extra_opts[@]}" || rc=$?
if [[ $rc -ne 0 ]]; then
    check_generation_consistency
    exit "$rc"
fi

# systemd --user and the fcitx5 unit are Linux-only (ADR-0018); darwin hosts
# (e.g. altair) have neither, so the whole follow-up is a no-op there.
if [[ "$(uname -s)" != "Linux" ]]; then
    echo "==> non-Linux host; skipping systemctl daemon-reload and fcitx5 restart."
    echo "Done."
    exit 0
fi

echo "==> systemctl --user daemon-reload"
systemctl --user daemon-reload

# herdr server staleness check (warn-only, #200)
check_herdr_staleness

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
