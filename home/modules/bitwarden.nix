# Shared Bitwarden Secrets Manager (bws) wiring (ADR-0000-github-app-as-code
# D10). Split out of obsidian.nix, which was the sole consumer until
# github-apps.nix needed the exact same bws + libsecret + `bws/config`
# triple for a second, deliberately separate machine account (least
# privilege per Secrets Manager project — obsidian-backup's token never
# needs to read the github-apps project, and vice versa). A single
# xdg.configFile."bws/config" definition keeps that from ever becoming two
# home-manager modules racing to declare the same path.
{ pkgs, ... }:
{
  home.packages = [
    pkgs.bws
    pkgs.libsecret
  ];

  # A revoked machine token must stop working immediately. bws otherwise
  # persists an encrypted session which can outlive revocation for up to an
  # hour.
  #
  # bws 2.0.0 has no implicit US-cloud default: an absent server_base fails
  # with "Profile has no server_base or server_identity" before ever touching
  # Bitwarden, so it must be set explicitly. https://vault.bitwarden.com is
  # the confirmed value for the US cloud (Bitwarden staff,
  # https://community.bitwarden.com/t/what-is-the-correct-url-for-server-base/57673,
  # 取得 2026-09-23).
  #
  # state_opt_out is a TOML *string*, not a boolean — bws 2.0.0's own `bws
  # config state-opt-out true` writes `state_opt_out = "true"`; a bare `true`
  # fails config parsing outright (crates/bws/src/config.rs) and every `bws
  # run` invocation errors before touching Bitwarden at all.
  xdg.configFile."bws/config".text = ''
    [profiles.default]
    server_base = "https://vault.bitwarden.com"
    state_opt_out = "true"
  '';
}
