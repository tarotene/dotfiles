#!/usr/bin/env zsh
# 10-path.zsh - PATH environment variable management

# CRITICAL: Ensure path/PATH synchronization is properly set up
# This must be done BEFORE any PATH modifications
typeset -U path PATH

# Force re-sync in case it was broken
export PATH

# User local binaries
[[ -d "$HOME/.local/bin" ]] && path=("$HOME/.local/bin" $path)
[[ -d "$HOME/bin" ]] && path=("$HOME/bin" $path)

# Development tool paths are added by respective modules
# Export again to ensure sync
export PATH
