# tailscale-prefs (ADR-471, #471 stage 2) — deploys the CLI that applies the
# per-identity Tailscale preferences declared by
# home/identities/{personal,company}.nix's `xdg.configFile."dotfiles/
# tailscale-prefs"`. The `tailscaled` daemon itself is apt-layer (system
# service, ADR-471's Decision) — this module only owns the unprivileged
# convergence step `hms` runs after every switch.
{ ... }:
let
  repoRoot = ../..;
in
{
  home.file.".local/bin/tailscale-prefs" = {
    source = repoRoot + "/scripts/tailscale-prefs";
    executable = true;
  };

  # Same `.backup` collision quarantine as gpg-subkey/detect-drift
  # (home/modules/quarantine.nix) — without this, a pre-existing file at
  # this path makes `hms` fail at checkLinkTargets (#244).
  dotfiles.quarantine.managedFiles = [ ".local/bin/tailscale-prefs" ];
}
