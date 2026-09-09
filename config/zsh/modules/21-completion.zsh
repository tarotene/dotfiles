#!/usr/bin/env zsh
# 40-completion.zsh - Completion system and keymap management
# Consolidated completion and keymap configuration

# Keymap corruption detection and repair
keymap_broken=false

# Pattern 1: KEYMAP is .safe
[[ "$KEYMAP" == ".safe" ]] && keymap_broken=true

# Pattern 2: Basic keybindings are abnormal
[[ "$(bindkey '^A' 2>/dev/null)" == *"self-insert"* ]] && keymap_broken=true

# Pattern 3: Alt+F/B are undefined
[[ "$(bindkey '^[f' 2>/dev/null)" == *"undefined-key"* ]] && keymap_broken=true

# Processing when corruption is detected
if [[ "$keymap_broken" == "true" ]]; then
    # Log only on first occurrence (date-based)
    local today=$(date +%Y-%m-%d)
    local logfile="/tmp/dotfiles-keymap-debug.log"
    local logged_today=false
    
    if [[ -f "$logfile" ]] && grep -q "=== $today ===" "$logfile" 2>/dev/null; then
        logged_today=true
    fi
    
    # Detailed log only on first occurrence
    if [[ "$logged_today" == "false" ]]; then
        {
            echo "=== $today ==="
            echo "Keymap broken detected at: $(date '+%H:%M:%S')"
            echo "KEYMAP: '$KEYMAP'"
            echo "EDITOR: '$EDITOR' | VISUAL: '$VISUAL'"
            echo "bindkey -lL main: $(bindkey -lL main 2>/dev/null)"
            echo "bindkey '^A': $(bindkey '^A' 2>/dev/null)"
            echo "Fixed with: bindkey -e"
            echo "---"
        } >> "$logfile"
        
        echo "⚠️  Keymap fixed (logged to $logfile)" >&2
    fi
fi

# Always force emacs mode (silently)
bindkey -e

# Add custom completion directory to fpath
if [[ -d "$HOME/.cache/zsh/completions" ]]; then
    fpath=("$HOME/.cache/zsh/completions" $fpath)
fi

# Initialize completion system
autoload -Uz compinit
compinit

# Basic completion settings
zstyle ':completion:*' menu select
zstyle ':completion:*' matcher-list 'm:{a-z}={A-Z}'