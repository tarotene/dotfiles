#!/bin/sh
# Detaching opener for files and URLs — deployed as `open`, `xdg-open`,
# and referenced by $BROWSER.
#
# The system xdg-open (xdg-utils 1.1.3) doesn't recognize COSMIC, falls to
# generic mode, and execs the MIME handler (browser, eog, ...) in the
# caller's foreground process group — so the caller blocks and Ctrl+C kills
# the viewer. Detach into a new session instead. gio open resolves the same
# xdg defaults but returns immediately.
#
# The fallback must be the absolute /usr/bin/xdg-open: this script shadows
# `xdg-open` on PATH, so a bare name would recurse into itself.
if command -v gio >/dev/null 2>&1; then
    setsid -f gio open "$@" >/dev/null 2>&1
else
    setsid -f /usr/bin/xdg-open "$@" >/dev/null 2>&1
fi
