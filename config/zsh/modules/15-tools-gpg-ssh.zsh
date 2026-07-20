#!/usr/bin/env zsh
# 15-tools-gpg-ssh.zsh - GPG agent tty wiring (pinentry + SSH support)
#
# gpg-agent renders curses pinentry on the terminal named by GPG_TTY, and with
# enableSshSupport (home/modules/gpg.nix) the agent also serves SSH, so a stale
# tty yields "Screen or window too small" on curses hosts and broken prompts
# for SSH-triggered pinentry. Refresh both on every interactive shell.

# $TTY is set by zsh itself whenever the shell has a controlling terminal;
# ttyless shells (editors, CI, tool-spawned) skip the block instead of
# exporting the "not a tty" garbage an unguarded $(tty) would produce.
if command -v gpg-connect-agent &>/dev/null && [[ -n $TTY ]]; then
    export GPG_TTY=$TTY
    gpg-connect-agent updatestartuptty /bye >/dev/null 2>&1
fi
