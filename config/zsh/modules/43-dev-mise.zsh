#!/usr/bin/env zsh
# 43-dev-mise.zsh - mise runtime manager integration
#
# Load mise shell hooks when available so runtime versions and shims
# stay in sync as directories change.
# Shell completion is included in mise activate.

if command -v mise &>/dev/null; then
    eval "$(mise activate zsh)"
    # Post-load assertion: activation registers a `_mise_hook` chpwd/precmd hook.
    if ! (( ${+functions[_mise_hook]} )); then
        print -u2 "warn: mise activation failed to load"
    fi
fi

