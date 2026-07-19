#!/usr/bin/env zsh
# 30-environment.zsh - Environment variables and history management
# Consolidated environment configuration

# Load POSIX-compliant common environment variables
if [[ -f "${XDG_CONFIG_HOME:-$HOME/.config}/shell/common_env" ]]; then
    source "${XDG_CONFIG_HOME:-$HOME/.config}/shell/common_env"
fi

# History file settings
HISTFILE="${ZDOTDIR:-$HOME}/.zsh_history"

# Create history file directory if it doesn't exist
# (fallback to /tmp if read-only)
if [[ ! -w "${HISTFILE:h}" ]]; then
    HISTFILE="/tmp/.zsh_history"
fi

HISTSIZE=10000
SAVEHIST=10000

# History-related options
setopt EXTENDED_HISTORY       # Record execution time in history
setopt HIST_EXPIRE_DUPS_FIRST # Delete duplicates first when history is full
setopt HIST_IGNORE_DUPS       # Don't record duplicate of previous command
setopt HIST_IGNORE_ALL_DUPS   # Remove older duplicates of commands
setopt HIST_IGNORE_SPACE      # Don't record commands starting with space
setopt HIST_FIND_NO_DUPS      # Don't show duplicates in history search
setopt HIST_SAVE_NO_DUPS      # Don't save duplicates to history file
setopt HIST_REDUCE_BLANKS     # Remove extra whitespace from history
setopt SHARE_HISTORY          # Share history between multiple Zsh sessions

# History search keybindings (for vi mode)
bindkey '^R' history-incremental-search-backward
bindkey '^S' history-incremental-search-forward
