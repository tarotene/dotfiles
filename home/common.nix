# Shared across every host. Instance-specific values (username, hostname) and
# identity-specific values (git identity, …) live in the host / identity
# modules that import this file.
{ lib, ... }:
{
  imports = [
    ./modules/claude.nix
    ./modules/claude-mcp-servers.nix
    ./modules/herdr.nix
    ./modules/worktree.nix
    ./modules/drift.nix
    ./modules/wrapper-hub.nix
    ./modules/apt.nix
    ./modules/quarantine.nix
    ./modules/shell.nix
    ./modules/atuin.nix
    ./modules/git.nix
    ./modules/gpg.nix
    ./modules/due-remind.nix
    ./modules/promotion-detect.nix
    ./modules/packages.nix
    ./modules/tailscale.nix
    ./modules/desktop.nix
    ./modules/downloads.nix
    ./modules/flatpak.nix
    ./modules/runtimes.nix
    ./modules/hm-warnings.nix
  ];

  programs.home-manager.enable = true;

  # Release the config targets. Bumped deliberately, not automatically.
  home.stateVersion = lib.mkDefault "25.11";
}
