#!/usr/bin/env zsh
# 40-dev-rust.zsh - Rust development environment
#
# This module sets up the Rust/Cargo environment if available.
# It checks for the standard Rust installation locations.

# Load Cargo environment using standard locations
if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
fi

# Enable shell completion for rustup and cargo
if command -v rustup &>/dev/null; then
    # rustup completion
    if [[ ! -f "$HOME/.cache/zsh/completions/_rustup" ]]; then
        mkdir -p "$HOME/.cache/zsh/completions"
        rustup completions zsh > "$HOME/.cache/zsh/completions/_rustup" 2>/dev/null
    fi

    # cargo completion
    if [[ ! -f "$HOME/.cache/zsh/completions/_cargo" ]]; then
        mkdir -p "$HOME/.cache/zsh/completions"
        rustup completions zsh cargo > "$HOME/.cache/zsh/completions/_cargo" 2>/dev/null
    fi
fi

# Debug output (comment out in production)
# echo "Rust development environment loaded (40-dev-rust.zsh)"
