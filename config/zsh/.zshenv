#!/usr/bin/env zsh
# Zsh environment variable settings (loaded in all Zsh sessions)
# Only essential environment variables - no computations or complex logic

# dotfiles specific settings
# NOTE: Using command substitution in .zshenv is generally bad practice as it can slow
# shell startup and cause issues if commands fail. However, this is necessary to derive
# DOTFILES_DIR from the actual file location when .zshenv is symlinked.
# Fallback to hardcoded path if dynamic detection fails
_this_file="${(%):-%N}"
_real_file="$(readlink -f "$_this_file" 2>/dev/null || echo "$_this_file")"
_dotfiles_dir="$(dirname "$(dirname "$(dirname "$_real_file")")" 2>/dev/null)"
if [[ -n "$_dotfiles_dir" && -d "$_dotfiles_dir" ]]; then
    export DOTFILES_DIR="$_dotfiles_dir"
else
    export DOTFILES_DIR="$HOME/dotfiles"
fi
unset _this_file _real_file _dotfiles_dir

# dotfiles management settings (custom path)
export ZDOTDIR="$DOTFILES_DIR/config/zsh"

# Load common environment variables
if [[ -f "$DOTFILES_DIR/config/shell/common_env" ]]; then
    source "$DOTFILES_DIR/config/shell/common_env"
fi

# System settings (XDG compliant - use XDG vars set by common_env)
export GIT_CONFIG_GLOBAL="${XDG_CONFIG_HOME}/git/config"

# GitHub scripts
export PATH="$HOME/dotfiles/scripts/github:$PATH"