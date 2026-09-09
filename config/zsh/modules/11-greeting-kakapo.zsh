#!/usr/bin/env zsh
# 11-greeting-kakapo.zsh - Party parrot login greeting
#
# ASCII art: frame 4 of animations/parrot.animation from
#   https://github.com/jmhobbs/terminal-parrot  (MIT)
#   Copyright (c) 2016 John Hobbs
#   Full licence text: licenses/terminal-parrot.MIT
# Reproduced verbatim.  The party parrot *is* a kakapo: it derives from
# Sirocco, the kakapo in Last Chance to See.  Note there are two works here —
# the GIF on cultofthepartyparrot.com is CC BY-SA 4.0, but the ASCII frames
# are released by the same copyright holder under MIT, so copying the ASCII
# (and never re-rendering from the GIF) keeps this file free of share-alike.
# That is also why the art is not scaled down: shrinking it would mean
# re-running jp2a against the CC BY-SA GIF.
#
# Gated on `[[ -o login ]]`.  modules/ is sourced from ~/.zshrc (via
# programs.zsh.initContent in home/modules/shell.nix), so this file only ever
# runs in an *interactive* shell — a non-interactive login shell (scp,
# `ssh host cmd`) can never reach it, which is why the greeting lives here
# rather than in programs.zsh.loginExtra (~/.zlogin).  The login test then
# narrows it to the shell a terminal window starts with: Alacritty spawns
# `-zsh`, so one window == one parrot, while zellij panes, `$(zsh)` and
# command substitutions stay quiet.
#
# Loaded at 11 (not last) on purpose: later modules' ADR-0005 post-load
# warnings must land *below* the 18-line art, next to the prompt where they
# are actually read.
#
# The upstream program animates by cycling one colour per frame.  A login
# banner must not block the shell, so one frame is printed in one colour
# picked at random from upstream's palette: the party survives, the shell
# keeps starting.
#
# `cat` (coreutils, home/modules/packages.nix) is used rather than `print -r`
# because the art contains `'`, which a single-quoted zsh string cannot hold.
# It carries no `command -v` gate on purpose: ADR-0005 governs shell
# *extension init* (`eval "$(tool init …)"`), not every external binary.

if [[ -o login ]]; then
    typeset _parrot_on='' _parrot_off=''

    # Only ever emit escape sequences to a real terminal.
    if [[ -t 1 ]]; then
        # xterm-256 palette, verbatim from terminal-parrot's colors.go:
        # peach, orange, green, cyan, blue, purple, pink, fuschia, magenta, red.
        typeset -a _parrot_palette
        _parrot_palette=(210 222 120 123 111 134 177 207 206 204)
        _parrot_on=$'\e[38;5;'"${_parrot_palette[$((RANDOM % 10 + 1))]}m"
        _parrot_off=$'\e[0m'
    fi

    print -rn -- "$_parrot_on"
    cat <<'PARROT'
           .ccccccc.
      .,,,;cooolccol;;,,.
     .dOx;..;lllll;..;xOd.
   .cdo,',loOXXXXXkll;';odc.
  ,oo:;c,':oko:cccccc,...ckl.
  ;c.;kXo..::..;c::'.......oc
,dc..oXX0kk0o.':lll;..cxxc.,ld,
kNo.'oXXXXXXo'':lll;..oXXOd;cOd.
KOc;oOXXXXXXo.':lol,..dXXXXl';xc
Ol,:k0XXXXXX0c.,clc'.:0XXXXx,.oc
KOc;dOXXXXXXXl..';'..lXXXXXd..oc
dNo..oXXXXXXXOx:..'lxOXXXXXk,.:; ..
cNo..lXXXXXXXXXOolkXXXXXXXXXkl;..;:.;.
.,;'.,dkkkkk0XXXXXXXXXXXXXXXXXOxxl;,;,;l:.
  ;c.;:''''':doOXXXXXXXXXXXXXXXXXXOdo;';clc.
  ;c.lOdood:'''oXXXXXXXXXXXXXXXXXXXXXk,..;ol.
  ';.:xxxxxocccoxxxxxxxxxxxxxxxxxxxxxxl::'.';;.
  ';........................................;l'
PARROT
    print -rn -- "$_parrot_off"

    unset _parrot_on _parrot_off _parrot_palette
fi
