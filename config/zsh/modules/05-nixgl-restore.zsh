#!/usr/bin/env zsh
# 05-nixgl-restore.zsh - give an interactive shell the GL environment it had *before*
# a nixGL-wrapped terminal started it (#851)
#
# Why: the nixGL wrapper (home/modules/nixgl.nix) sets LD_LIBRARY_PATH,
# LIBGL_DRIVERS_PATH and friends so the wrapped GUI app (alacritty, warp-terminal)
# can load nix's own mesa. Every descendant inherits them -- including the shell
# the terminal starts and everything typed into it -- so an apt-installed GL app
# (KiCad) loads nix's mesa into a system-glibc process and falls back to software
# rendering ("Could not use OpenGL, falling back to software rendering"). Only the
# wrapped app itself needs the variables.
#
# How: the wrapper saves each variable's value from before nixGL touched it into
# __NIXGL_SAVED_<VAR> (or __NIXGL_UNSET__ if it was unset) and sets NIXGL_WRAPPED=1.
# This module puts the saved values back and removes the marks, so neither the shell
# nor its children carry the nixGL environment. Nothing is done without the mark: a
# user who types `nixGLIntel <cmd>` on purpose keeps exactly what they asked for
# (nix-community/nixGL#116's unconditional `unset` would break that).
#
# Gated on the mark only (ADR-0005): no binary or credential is involved.

if [[ -n ${NIXGL_WRAPPED-} ]]; then
    local __nixgl_var __nixgl_saved
    for __nixgl_var in GBM_BACKENDS_PATH LIBGL_DRIVERS_PATH LIBVA_DRIVERS_PATH \
        __EGL_VENDOR_LIBRARY_FILENAMES LD_LIBRARY_PATH; do
        __nixgl_saved=__NIXGL_SAVED_${__nixgl_var}
        # A variable whose original value was not saved is left alone.
        (( ${+parameters[$__nixgl_saved]} )) || continue
        if [[ ${(P)__nixgl_saved} == __NIXGL_UNSET__ ]]; then
            unset "$__nixgl_var"
        else
            export "$__nixgl_var=${(P)__nixgl_saved}"
        fi
        unset "$__nixgl_saved"
    done
    unset NIXGL_WRAPPED
    unset __nixgl_var __nixgl_saved
fi
