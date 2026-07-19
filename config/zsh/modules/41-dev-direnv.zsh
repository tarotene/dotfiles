#!/usr/bin/env zsh
# 51-dev-direnv.zsh - direnv environment manager integration
#
# This module integrates direnv for automatic environment variable management
# based on directory context (.envrc files).

# Load direnv hook if direnv is available
if command -v direnv &>/dev/null; then
    eval "$(direnv hook zsh)"
    # Post-load assertion: the hook registers a `_direnv_hook` precmd function.
    if ! (( ${+functions[_direnv_hook]} )); then
        print -u2 "warn: direnv hook failed to load"
    fi
fi

# Debug output (comment out in production)
# echo "direnv integration loaded (51-dev-direnv.zsh)"
