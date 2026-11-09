# Shared across every host. Instance-specific values (username, hostname) and
# identity-specific values (git identity, …) live in the host / identity
# modules that import this file.
{ lib, ... }:
let
  importDir = import ./lib/import-dir.nix { inherit lib; };
in
{
  # home/modules/ is the set of modules every host evaluates: the directory's
  # contents are the import list, so a new module placed there is evaluated
  # (no second list to update). A module only one identity or one host needs
  # goes in home/identities/<identity>/ or home/hosts/<host>/; a plain function
  # that is not a module (nixgl.nix) goes in home/lib/.
  imports = importDir ./modules;

  programs.home-manager.enable = true;

  # Release the config targets. Bumped deliberately, not automatically.
  home.stateVersion = lib.mkDefault "25.11";
}
