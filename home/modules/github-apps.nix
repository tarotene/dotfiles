# github-app-snapshot deployment (ADR-590 D3/D5).
# vega-only for now (imported from home/hosts/vega.nix only) — the only
# host this snapshot has actually been run from; add to another host's
# import list once that host also needs to run it.
{ pkgs, ... }:
{
  imports = [ ./bitwarden.nix ];

  home.packages = [ pkgs.openssl ];

  home.file.".local/bin/github-app-snapshot" = {
    source = ../../scripts/github-app-snapshot;
    executable = true;
  };

  # github-app-registry-check (ADR-436 Amendment 2026-09-30): account-level
  # Manifest ⇔ live-registration drift check, reads the snapshot
  # github-app-snapshot writes. No secrets, same as github-audit itself.
  home.file.".local/bin/github-app-registry-check" = {
    source = ../../scripts/github-app-registry-check;
    executable = true;
  };

  # D2: the registration source of truth (permissions/events), never the
  # PEM — see config/github-app-manifests/*.json's own header for what this
  # is and isn't.
  xdg.configFile."github-app-snapshot/manifests/releaser.json".source =
    ../../config/github-app-manifests/releaser.json;
}
