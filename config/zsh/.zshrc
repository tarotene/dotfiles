#!/usr/bin/env zsh
# Modular Zsh Configuration
# This file is the main file that loads each functional module

# Debug mode (enable if needed)
# set -x

# Module directory configuration
ZSHRC_MODULE_DIR="${ZDOTDIR:-$HOME/.config/zsh}/modules"

# Module loading function
load_module() {
    local module_file="$1"
    if [[ -r "$module_file" ]]; then
        # Debug output (enable if needed)
        # echo "Loading: $module_file"
        source "$module_file"
    else
        echo "Warning: Module not found: $module_file" >&2
    fi
}

# Load all modules in numerical order
if [[ -d "$ZSHRC_MODULE_DIR" ]]; then
    for module in "$ZSHRC_MODULE_DIR"/*.zsh(N); do
        load_module "$module"
    done
else
    echo "Warning: Module directory not found: $ZSHRC_MODULE_DIR" >&2
fi

# Load local settings (for personal settings not managed by git)
if [[ -r "${ZDOTDIR:-$HOME/.config/zsh}/.zshrc.local" ]]; then
    source "${ZDOTDIR:-$HOME/.config/zsh}/.zshrc.local"
fi
