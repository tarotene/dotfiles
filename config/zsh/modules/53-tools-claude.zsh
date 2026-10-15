#!/usr/bin/env zsh
# 53-tools-claude.zsh - keep the Opus Plan Mode model pin in sync with updates
#
# Claude Code's own binary is the native installer's self-updater symlink
# (~/.local/bin/claude), not a Nix package (ADR-457, a scoped exception to
# ADR-0001). Automatic background updates are disabled by declaration
# (DISABLE_AUTOUPDATER, home/modules/claude.nix) — the only update path is
# running `claude update` yourself. `claude-plan-model` (crates/claude-plan-model) resolves its
# concrete model IDs from the *installed* binary's baked-in catalog, so a
# manual update can silently leave the pin one generation behind until
# `claude-plan-model` is run by hand. This wrapper closes that gap: running
# `claude update`/`upgrade` re-syncs the pin right after, in the same
# command, the same way home-manager activation already does on every `hms`.
#
# Gated only on binary existence (ADR-0005) — `command claude` needs no
# credentials to exist.

if command -v claude &>/dev/null; then
    claude() {
        command claude "$@"
        local rc=$?
        case "${1-}" in
        update | upgrade)
            command -v claude-plan-model &>/dev/null && claude-plan-model sync
            ;;
        esac
        return $rc
    }
fi

# Claude Code's Bash tool runs commands in this zsh (via a shell snapshot that
# restores the options set here). zsh does not field-split unquoted parameter
# expansions, so `set -- $var` and `for x in $var` that work in bash silently
# yield one word, and a batch loop fails once. Give *only* Claude Code's shell
# bash-like splitting: CLAUDECODE=1 is set inside the Bash tool, so an
# interactive terminal keeps zsh's default (SH_WORD_SPLIT off).
# zsh manual, "Options", SH_WORD_SPLIT:
# https://zsh.sourceforge.io/Doc/Release/Options.html (retrieved 2026-10-15).
# Whether CLAUDECODE is already set while the snapshot is generated is
# unverified — read it back in the Bash tool: `[[ -o shwordsplit ]] && echo on`.
[[ -n ${CLAUDECODE-} ]] && setopt SH_WORD_SPLIT
