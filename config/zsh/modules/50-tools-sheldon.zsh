#!/usr/bin/env zsh
# 60-tools-sheldon.zsh - Sheldon plugin manager

# Sheldon initialization
# Execute only if sheldon command exists and config file is present
if command -v sheldon &>/dev/null; then
    # CRITICAL: Save current PATH state before Sheldon
    # Verify path array is valid before saving
    if [[ -n "${path+x}" ]] && (( ${#path[@]} > 0 )); then
        local _saved_path=("${path[@]}")
    else
        # If path is unset or empty, initialize from PATH
        local _saved_path=(${(s/:/)PATH})
    fi
    
    # Check cache file existence and update
    # sheldon downloads plugins on first run,
    # so create cache if it doesn't exist
    cache_dir="${XDG_CACHE_HOME:-$HOME/.cache}/sheldon"
    if [[ ! -d "$cache_dir" ]]; then
        mkdir -p "$cache_dir"
    fi
    
    # Execute sheldon evaluation
    # This loads plugins defined in plugins.toml.
    # sheldon sources arbitrary plugin artifacts, so there is no single wrapper
    # to assert on; instead check the generated source is non-empty before eval.
    local _sheldon_src="$(sheldon source)"
    if [[ -z "$_sheldon_src" ]]; then
        print -u2 "warn: sheldon produced no source output"
    fi
    eval "$_sheldon_src"
    
    # CRITICAL: Restore and merge PATH if needed
    # Check if ~/.local/bin was lost during Sheldon init
    if [[ -d "$HOME/.local/bin" ]] && [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
        # Restore from saved path
        path=("${_saved_path[@]}")
        # Re-ensure uniqueness
        typeset -U path PATH
        export PATH
    fi
    
    # Debug output (uncomment if needed)
    # echo "Sheldon plugin manager loaded (60-tools-sheldon.zsh)"
fi
