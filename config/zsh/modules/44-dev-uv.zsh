#!/usr/bin/env zsh
# 44-dev-uv.zsh - uv (Python package manager) integration
#
# Load uv and uvx shell completion when available.

if command -v uv &>/dev/null; then
    eval "$(uv generate-shell-completion zsh)"
    # Post-load assertion: the completion defines a `_uv` function.
    if ! (( ${+functions[_uv]} )); then
        print -u2 "warn: uv completion failed to load"
    fi
    command -v uvx &>/dev/null && eval "$(uvx --generate-shell-completion zsh)"
fi
