# github-app-snapshot deployment (ADR-0000-github-app-as-code D3/D5/D10).
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

  # D2: the registration source of truth (permissions/events), never the
  # PEM — see config/github-app-manifests/*.json's own header for what this
  # is and isn't.
  xdg.configFile."github-app-snapshot/manifests/releaser.json".source =
    ../../config/github-app-manifests/releaser.json;

  # D4 PAT probe declarations (PUBLIC-only; a .local.tsv sibling for
  # PRIVATE-repo-only probes is never committed, same split as
  # github-audit's own closed-vocabulary .tsv files).
  xdg.configFile."github-app-snapshot/pat-probes.tsv".source =
    ../../config/github-app-snapshot/pat-probes.tsv;
}
