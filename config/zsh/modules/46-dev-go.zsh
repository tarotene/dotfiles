#!/usr/bin/env zsh
# 46-dev-go.zsh - Go development environment
#
# Go is managed by mise (#215 / ADR-0002); this module only adds shell
# completion for the Go commands when `go` is on PATH.

if command -v go &>/dev/null; then
    # Enable Go completion (if go is installed)
    # Note: Go's built-in completion is available via `go completion zsh`
    if [[ ! -f "$HOME/.cache/zsh/completions/_go" ]]; then
        mkdir -p "$HOME/.cache/zsh/completions"
        go completion zsh > "$HOME/.cache/zsh/completions/_go" 2>/dev/null
    fi
fi
