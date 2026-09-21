#!/usr/bin/env zsh
# 40-dev-rust.zsh - Rust development environment
#
# This module sets up the Rust/Cargo environment if available.
# It checks for the standard Rust installation locations.

# Deliberately NOT sourcing ~/.cargo/env here (ADR-0029).
#
# That script exists only to put ~/.cargo/bin on PATH, and it *prepends* —
# landing in front of ~/.nix-profile/bin and silently overriding whatever
# home-manager declares.  config/shell/common_env already adds the directory,
# appended, which is the order ADR-0029 requires.  The file itself is left on
# disk so anything else that sources it keeps working; it turns into a no-op
# once ~/.cargo/bin is already present.

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
