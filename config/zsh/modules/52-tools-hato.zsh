#!/usr/bin/env zsh
# 52-tools-hato.zsh - hato notification triage prompt hook
#
# Loads the badge (🔔N in RPROMPT) and Ctrl-G TUI launch keybinding.
# Gated only on binary existence, never on auth (ADR-0005) — `hato init zsh`
# just prints a static hook script and needs no credentials to do so.

if command -v hato &>/dev/null; then
    eval "$(hato init zsh)"
    # Post-load assertion: the hook defines the Ctrl-G TUI-launch widget.
    if ! (( ${+functions[_hato_tui_widget]} )); then
        print -u2 "warn: hato prompt hook failed to load"
    fi
    # Debug output (uncomment if needed)
    # echo "hato prompt hook loaded (52-tools-hato.zsh)"
fi
