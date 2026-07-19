#!/usr/bin/env zsh
# 45-dev-deno.zsh - Deno runtime environment
#
# Load Deno shell completion when available.

if command -v deno &>/dev/null; then
    eval "$(deno completions zsh)"
fi
