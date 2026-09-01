#!/bin/sh
# $BROWSER target — open URLs without blocking the caller.
#
# gh browse (and anything honoring $BROWSER) waits for this command to exit.
# The system xdg-open (xdg-utils 1.1.3) doesn't recognize COSMIC, falls to
# generic mode, and execs the browser in the caller's foreground process
# group — so the caller blocks and Ctrl+C kills the browser. Detach into a
# new session instead. gio open resolves the same xdg default
# (identity-scoped mimeapps.list) but returns immediately.
if command -v gio >/dev/null 2>&1; then
    setsid -f gio open "$@" >/dev/null 2>&1
else
    setsid -f xdg-open "$@" >/dev/null 2>&1
fi
