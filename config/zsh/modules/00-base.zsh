#!/usr/bin/env zsh
# 00-base.zsh - Zsh basic configuration
#
# This module provides the most basic Zsh configuration
# Must be loaded before all other modules

# Basic Zsh options
setopt AUTO_CD              # Auto cd when typing directory name
setopt AUTO_PUSHD          # Auto push directory to stack on cd
setopt PUSHD_IGNORE_DUPS   # Don't add duplicate directories to stack
# Disable spelling correction to avoid annoying suggestions
unsetopt CORRECT          # Don't correct command spelling mistakes
unsetopt CORRECT_ALL      # Don't correct entire command line spelling
setopt NO_BEEP           # Disable beep sounds
setopt INTERACTIVE_COMMENTS # Enable comments in interactive mode

# Character encoding settings moved to config/shell/common_env

# Editor settings
export EDITOR=${EDITOR:-vim}
export VISUAL=${VISUAL:-$EDITOR}

# Pager settings
export PAGER=less
export LESS='-R -i -M -X'

# Debug output (uncomment if needed)
# echo "Base configuration loaded (00-base.zsh)"
