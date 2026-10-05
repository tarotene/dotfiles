# tailscale-prefs (ADR-471, #471 stage 2) — deploys the CLI that applies the
# per-identity Tailscale preferences, and owns the prefs file it reads
# (`~/.config/dotfiles/tailscale-prefs`). The `tailscaled` daemon itself is
# apt-layer (system service, ADR-471's Decision) — this module only owns the
# unprivileged convergence step `hms` runs after every switch.
#
# The prefs are a typed option, not free text: the identity modules
# (home/identities/{personal,company}.nix) set the values that follow the
# person, and the one value that must not live in this PUBLIC repository —
# the Mullvad exit node's real name — is supplied by the private wrapper
# flake's `extraModules` (ADR-0034), the same "expose only the mouth" shape as
# `dotfiles.privateHub.ref` (wrapper-hub.nix) and `dotfiles.apt.extraPackages`
# (apt.nix). A local edit of the generated file would not survive: `hms`
# applies through the wrapper on a registered host, and the next switch
# rewrites it.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.tailscale;
  boolText = b: if b then "true" else "false";
in
{
  options.dotfiles.tailscale = {
    exitNode = lib.mkOption {
      type = lib.types.str;
      default = "";
      description = ''
        The exit node `tailscale set --exit-node=` is converged to on every
        `hms`. Empty (the default) still emits a clearing `--exit-node=`, so a
        node a previous manual session left set is reverted. The real
        Mullvad node name is a private machine-state value (ADR-0034): the
        private wrapper flake's extraModules set it, this repository does not.
        Pin a node name from `tailscale exit-node list`, not `auto:any` — the
        latter can fall back to a Mullvad node that is not currently reachable
        (docs/operations.md, "Café Wi-Fi").
      '';
      example = "<node name from `tailscale exit-node list`>";
    };

    exitNodeAllowLanAccess = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        `--exit-node-allow-lan-access`: keep directly-connected subnets (the
        home LAN printer) reachable while an exit node is active.
      '';
    };

    shieldsUp = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        `--shields-up`: refuse incoming connections from other tailnet
        devices. The company identity sets this; personal devices accept them.
      '';
    };

    ssh = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        `--ssh`: run the Tailscale SSH server on this host, so other tailnet
        devices can reach it per the ACL's `ssh` section
        (config/tailscale/policy.hujson). Off by default; a host that cannot
        run the server must stay off. The macOS Standalone app cannot — only
        the open-source tailscaled can (Tailscale KB 1065) — which is why
        the personal identity enables it on Linux only.
      '';
    };
  };

  config = {
    # Rust、crates/tailscale-prefs(ADR-0024、#414)経由で pkgs.dotfiles-tools から配備。
    home.file.".local/bin/tailscale-prefs".source = "${pkgs.dotfiles-tools}/bin/tailscale-prefs";

    # The closed vocabulary crates/tailscale-prefs reads (an unknown key fails
    # its own tests), one `key=value` per line.
    xdg.configFile."dotfiles/tailscale-prefs".text = ''
      exit_node=${cfg.exitNode}
      exit_node_allow_lan_access=${boolText cfg.exitNodeAllowLanAccess}
      shields_up=${boolText cfg.shieldsUp}
      ssh=${boolText cfg.ssh}
    '';

    # Same `.backup` collision quarantine as gpg-subkey/detect-drift
    # (home/modules/quarantine.nix) — without this, a pre-existing file at
    # this path makes `hms` fail at checkLinkTargets (#244).
    dotfiles.quarantine.managedFiles = [ ".local/bin/tailscale-prefs" ];
  };
}
