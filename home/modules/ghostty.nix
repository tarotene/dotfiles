# Ghostty, the outer terminal that draws images (ADR-1694, docs/agents/show-image.md).
#
# herdr renders pane images only through the Kitty graphics protocol, which Alacritty does not
# implement (measured 2026-11-24: `chafa -f kitty` in an Alacritty herdr pane shows nothing).
# Ghostty does. Linux gets the pinned nixpkgs build behind the shared nixGL wrapper (it needs
# EGL like alacritty; measured 2026-11-24: nixpkgs 1.3.1 starts, draws the images and takes
# Japanese input on vega); darwin gets `ghostty-bin`, since the source build is not offered there.
# The wrapped package ships its own `com.mitchellh.ghostty.desktop`, with Exec= rewritten to the
# wrapped store path, so no launcher entry is written here.
#
# Alacritty stays declared (home/modules/desktop.nix) until daily use of Ghostty is confirmed.
# Declaring `kittyProtocol` here is what lets `show-image` draw: a herdr attached from Alacritty
# on the same host shows a blank where the image goes (the path is printed as text beside it).
{
  lib,
  pkgs,
  repoConfig,
  ...
}:
let
  nixGLWrap = import ../lib/nixgl.nix { inherit pkgs; };
in
{
  config = lib.mkMerge [
    (lib.mkIf pkgs.stdenv.isLinux {
      home.packages = [ (nixGLWrap pkgs.ghostty) ];
    })
    (lib.mkIf pkgs.stdenv.isDarwin {
      home.packages = [ pkgs.ghostty-bin ];
    })
    {
      xdg.configFile."ghostty/config".source = repoConfig + "/ghostty/config";
      dotfiles.terminal.graphics.kittyProtocol = true;
    }
  ];
}
