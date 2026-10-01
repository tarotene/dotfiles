# github-app-snapshot deployment (ADR-590 D3/D5).
# vega-only for now (imported from home/hosts/vega.nix only) — the only
# host this snapshot has actually been run from; add to another host's
# import list once that host also needs to run it.
{ pkgs, ... }:
{
  imports = [ ./bitwarden.nix ];

  home.packages = [ pkgs.openssl ];

  # Rust, from crates/github-app-snapshot via pkgs.dotfiles-tools (ADR-0024,
  # #414). It still shells out to openssl (RS256 signing), curl, bws and
  # secret-tool, so pkgs.openssl above and the bitwarden.nix import stay.
  home.file.".local/bin/github-app-snapshot".source =
    "${pkgs.dotfiles-tools}/bin/github-app-snapshot";

  # github-app-registry-check (ADR-436 Amendment 2026-09-30): account-level
  # Manifest ⇔ live-registration drift check, reads the snapshot
  # github-app-snapshot writes. No secrets, same as github-audit itself.
  # Rust, from crates/github-app-registry-check (#414).
  home.file.".local/bin/github-app-registry-check".source =
    "${pkgs.dotfiles-tools}/bin/github-app-registry-check";

  # D2: the registration source of truth (permissions/events), never the
  # PEM — see config/github-app-manifests/*.json's own header for what this
  # is and isn't.
  xdg.configFile."github-app-snapshot/manifests/releaser.json".source =
    ../../config/github-app-manifests/releaser.json;
}
