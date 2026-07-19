# Shared across every host. Instance-specific values (username, hostname) and
# identity-specific values (git identity, …) live in the host / identity
# modules that import this file.
{ lib, ... }:
{
  imports = [
    ./modules/shell.nix
    ./modules/git.nix
    ./modules/gpg.nix
    ./modules/secrets.nix
    ./modules/packages.nix
    ./modules/desktop.nix
    ./modules/runtimes.nix
  ];

  programs.home-manager.enable = true;

  # Release the config targets. Bumped deliberately, not automatically.
  home.stateVersion = lib.mkDefault "25.11";
}
