#!/usr/bin/env zsh
# 43-tools-ghr.zsh - ghr repository management tool
#
# This module enables ghr shell extensions for repository management.
# ghr provides cd command integration and shell completions.
# Note: Using Bash shell integration as ghr doesn't have native Zsh support

# Load ghr shell extension if available.
# Gate ONLY on binary existence, never on GITHUB_TOKEN: `ghr shell bash` exits 0
# and emits the `ghr()` wrapper even with an empty token, and its "token not
# found" warning is already suppressed by the 2>/dev/null redirect below.
# See docs/adr/0005-shell-extension-init-no-auth-gate.md.
if command -v ghr &>/dev/null; then
    # Load Bash shell integration (compatible with Zsh)
    # This enables `ghr cd` and `ghr clone --cd` functionality
    source <(ghr shell bash 2>/dev/null)

    # Defensive check: if the wrapper failed to register, surface it now instead
    # of letting `ghr cd` fall through to the real binary later.
    if ! (( ${+functions[ghr]} )); then
        print -u2 "warn: ghr shell extension failed to load"
    fi

    # Load Bash completions (may work in Zsh with bashcompinit)
    # Enable bash completion compatibility if not already loaded
    autoload -Uz bashcompinit 2>/dev/null
    if (( ${+functions[bashcompinit]} )) && ! (( ${+_comps} )); then
        bashcompinit
    fi

    # Load ghr completions
    if (( ${+functions[complete]} )); then
        source <(ghr shell bash --completion 2>/dev/null)
    fi

    # Debug output (comment out in production)
    # echo "ghr integration loaded (43-tools-ghr.zsh)"
fi
