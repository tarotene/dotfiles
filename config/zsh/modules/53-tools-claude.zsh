#!/usr/bin/env zsh
# 53-tools-claude.zsh - keep the Opus Plan Mode model pin in sync with updates
#
# Claude Code's own binary is the native installer's self-updater symlink
# (~/.local/bin/claude), not a Nix package (ADR-457, a scoped exception to
# ADR-0001). Automatic background updates are disabled by declaration
# (DISABLE_AUTOUPDATER, home/modules/claude.nix) — the only update path is
# running `claude update` yourself. `scripts/claude-plan-model` resolves its
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
