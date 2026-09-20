#!/usr/bin/env zsh
# 20-environment.zsh - Environment variables and history management
# Consolidated environment configuration

# Load POSIX-compliant common environment variables
if [[ -f "${XDG_CONFIG_HOME:-$HOME/.config}/shell/common_env" ]]; then
    source "${XDG_CONFIG_HOME:-$HOME/.config}/shell/common_env"
fi

# History file settings
HISTFILE="${ZDOTDIR:-$HOME}/.zsh_history"

# If $HOME (or $ZDOTDIR) isn't writable, don't persist history at all.
# A previous fallback wrote to the shared, world-readable /tmp/.zsh_history
# instead — on a multi-user host that leaks command history (which can
# contain secrets) to every other user, and could interleave history from
# unrelated sessions/users that hit the same fallback path (#263).
if [[ ! -w "${HISTFILE:h}" ]]; then
    unset HISTFILE
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

# Don't persist commands referencing ephemeral agent-session paths
# (Claude scratchpad, herdr worktrees) to HISTFILE; they remain in the
# session's internal history only. SHARE_HISTORY stays on.
HISTORY_IGNORE='(*/tmp/claude-*|*.herdr/worktrees/*)'

# History search keybindings (for vi mode)
# Ctrl-R is intentionally left to atuin (home/modules/atuin.nix's
# `atuin init zsh`, evaluated in the generated .zshrc after this module's
# loop — confirmed via the generated ~/.zshrc — so atuin's own ^R binding
# always wins regardless). atuin owns the interactive search UI by design,
# while ↑-arrow keeps zsh's native behavior (--disable-up-arrow in that
# module). A native bindkey here was redundant and made the intent
# ambiguous — this module never actually controls ^R (#264).
bindkey '^S' history-incremental-search-forward
