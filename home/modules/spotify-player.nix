# spotify-player (#861): a Spotify TUI that doubles as a scriptable CLI
# (`spotify_player search|playback|get|playlist ...`, JSON on stdout).
#
# Imported from home/identities/personal.nix, not common.nix: a personal
# streaming account, kept off company hosts like esa.nix and line.nix. Needs a
# Spotify Premium account.
#
# Package: the overlay in flake.nix hands over the nixpkgs-unstable build
# (0.25.1). The pinned stable 0.23.0 cannot parse the Web API responses of a
# newly created developer app (`missing field followers` / `available_markets`;
# fixed by aome510/spotify-player#973, first released in v0.25.0).
#
# Audio backend, Linux only: the default `rodio` (ALSA straight through nix's
# alsa-lib) panics in the player thread on this host's PipeWire setup — seen on
# both 0.23.0 and 0.25.1, at the same `unwrap` in librespot-playback 0.8.0
# (src/audio_backend/rodio.rs:222). The error text is lost in the panic and the
# root cause is not pinned down. `pulseaudio` goes through PipeWire's
# pulse-compatible socket instead; it did not panic and played audio (checked
# 2026-10-20 on vega). The override is not in the binary cache, so the first
# build compiles from source (about 8 minutes). darwin keeps the default
# backend; CoreAudio goes through rodio and was not tried.
#
# Client ID: spotify-player's built-in one is ncspot's shared ID, whose request
# quota is shared by every user of it and ran into 429 on the first call. The
# README recommends registering one's own, and that is what this module takes
# as an option. The value is a machine-state value, so it is NOT written in this
# public repository (ADR-0034): the private wrapper flake sets
# `dotfiles.spotifyPlayer.clientId`. Registration steps: docs/operations.md,
# "spotify-player: your own client ID". Left at null, `app.toml` is empty and
# spotify-player falls back to the shared ID.
#
# `app.toml` is deployed read-only, so spotify-player never writes its default
# one there. Its cache (`~/.cache/spotify-player`: credentials, tokens, logs) is
# not managed.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.spotifyPlayer;

  package =
    if pkgs.stdenv.isLinux then
      pkgs.spotify-player.override { withAudioBackend = "pulseaudio"; }
    else
      pkgs.spotify-player;

  appConfig = (pkgs.formats.toml { }).generate "spotify-player-app.toml" (
    lib.optionalAttrs (cfg.clientId != null) { client_id = cfg.clientId; }
  );
in
{
  options.dotfiles.spotifyPlayer.clientId = lib.mkOption {
    type = lib.types.nullOr lib.types.str;
    default = null;
    description = ''
      The Spotify application client ID spotify-player authenticates with.
      Null keeps spotify-player's built-in (shared) ID. The real value comes
      from the private wrapper flake, never from this repository (ADR-0034).
    '';
  };

  config = {
    home.packages = [ package ];

    xdg.configFile."spotify-player/app.toml".source = appConfig;
  };
}
