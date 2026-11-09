# github-app-snapshot deployment (ADR-590 D3/D5).
# vega-only for now (it lives in home/hosts/vega/, which only vega imports) —
# the only host this snapshot has actually been run from; move it to
# home/modules/ once every host needs to run it.
{ pkgs, ... }:
{
  home.packages = [ pkgs.openssl ];

  dotfiles.binaries.".local/bin" = [
    # Rust, from crates/github-app-snapshot via pkgs.dotfiles-tools (ADR-0024,
    # #414). It still shells out to openssl (RS256 signing), curl, bws and
    # secret-tool, so pkgs.openssl above and the bitwarden.nix import stay.
    "github-app-snapshot"
    # github-app-registry-check (ADR-436 Amendment 2026-09-30): account-level
    # Manifest ⇔ live-registration drift check, reads the snapshot
    # github-app-snapshot writes. No secrets, same as github-audit itself.
    # Rust, from crates/github-app-registry-check (#414).
    "github-app-registry-check"
  ];

  # D2: the registration source of truth (permissions/events), never the
  # PEM — see config/github-app-manifests/*.json's own header for what this
  # is and isn't.
  xdg.configFile."github-app-snapshot/manifests/releaser.json".source =
    ../../../config/github-app-manifests/releaser.json;
}
