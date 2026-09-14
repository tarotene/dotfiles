# Atuin — offline-only shell history capture (docs/adr/0011).
#
# Why: a separate personal work-time evidence pipeline (outside this repo)
# needs local-only evidence of terminal activity — command, cwd, duration,
# exit status, session — that Slack/GitHub timelines cannot reconstruct.
# Atuin's SQLite history.db (~/.local/share/atuin/history.db by default) is
# that evidence source; this repo's only job is to turn it on without ever
# registering an account or syncing anywhere.
#
# This module is intentionally offline-first and does NOT run `atuin
# register`/`atuin login` anywhere, nor enable any sync setting. Key names
# and defaults below are confirmed against the upstream config reference —
# Atuin, "Config" (docs.atuin.sh/latest/configuration/config/), retrieved
# 2026-09-14: `update_check` (default true, hits https://api.atuin.sh at
# most once/hour), `auto_sync` (default true, only syncs "when logged in"),
# `secrets_filter` (default true, credential-shaped commands are never
# stored), `store_failed` (default true), `history_filter` /
# `cwd_filter` (default empty, regex exclude-lists). The same page states
# plainly: "With the update check turned off and sync not set up, Atuin
# makes no network requests of its own" — so `update_check = false` +
# `auto_sync = false` + never authenticating together are what make this
# fully offline, not `auto_sync` alone.
#
# Before the first `hms`/`home-manager switch` on a new machine, confirm
# these option names still match this pin by running:
#   home-manager option programs.atuin
# (module lives at nixpkgs' home-manager modules/programs/atuin.nix; the
# `settings` key is a freeform attrs mirroring atuin's own config.toml).
{ ... }:
{
  programs.atuin = {
    enable = true;
    enableZshIntegration = true;
    # Keep the default ↑-arrow history behavior unchanged — only ctrl-r's
    # interactive search becomes atuin's.
    flags = [ "--disable-up-arrow" ];
    settings = {
      update_check = false; # zero network calls by default
      auto_sync = false; # never sync; no account, no registration
      secrets_filter = true; # built-in filters for AWS/GitHub/Slack/... token-shaped commands
      store_failed = true;
      history_filter = [ ];
      cwd_filter = [ ];
    };
  };
}
