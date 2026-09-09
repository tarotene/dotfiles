#!/usr/bin/env zsh
# 51-tools-starship.zsh - Starship prompt

# Starship initialization
if command -v starship &>/dev/null; then
    eval "$(starship init zsh)"
    # Post-load assertion: init defines the precmd prompt hook. starship >= 1.24
    # renamed it starship_precmd -> prompt_starship_precmd; accept both.
    if ! (( ${+functions[starship_precmd]} || ${+functions[prompt_starship_precmd]} )); then
        print -u2 "warn: starship prompt failed to load"
    fi
    # Debug output (uncomment if needed)
    # echo "Starship prompt loaded (46-tools-starship.zsh)"
fi
