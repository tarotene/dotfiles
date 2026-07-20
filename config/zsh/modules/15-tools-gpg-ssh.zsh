#!/usr/bin/env zsh
# 15-tools-gpg-ssh.zsh - GPG agent tty wiring (pinentry + SSH support)
#
# gpg-agent renders curses pinentry on the terminal named by GPG_TTY, and with
# enableSshSupport (home/modules/gpg.nix) the agent also serves SSH, so a stale
# tty yields "Screen or window too small" on curses hosts and broken prompts
# for SSH-triggered pinentry. Refresh both on every interactive shell.

if command -v gpg-connect-agent &>/dev/null; then
    export GPG_TTY=${TTY:-$(tty)}
    gpg-connect-agent updatestartuptty /bye >/dev/null 2>&1
fi
