# Literal Brewfile remains the single package declaration on altair.
{
  nixpkgs.hostPlatform = "aarch64-darwin";
  system.stateVersion = 7;
  system.primaryUser = "tarotene";
  users.users.tarotene.home = "/Users/tarotene";
  nix.enable = false;
  homebrew = {
    enable = true;
    extraConfig = builtins.readFile ../packages/declarative/Brewfile;
    onActivation = {
      cleanup = "none";
      autoUpdate = false;
      upgrade = false;
    };
  };
}
