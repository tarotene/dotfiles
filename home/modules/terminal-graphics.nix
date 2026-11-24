# 図・画像を端末のペインに出す入口(ADR-1694、docs/agents/show-image.md)。
#
# `show-image <画像のパス>` は、herdr の隣のペインに chafa で画像を描く(Kitty graphics protocol)。
# 外側の端末が画像に対応しているかは、herdr のペインの中からは検出できない(API に項目が無く、同じ
# サーバーに別の端末がつながりうる)ので、対応する端末を使うホストが `kittyProtocol = true` と
# 宣言する。宣言は `~/.config/dotfiles/terminal-graphics` に `kitty` と書かれ、`show-image` はそれが
# 無いホストでは理由を出して 0 以外で終わる(呼び出し側は従来の経路に落ちる)。
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.terminal.graphics;
in
{
  options.dotfiles.terminal.graphics.kittyProtocol = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = ''
      Whether the terminal this host's herdr is used from implements the Kitty graphics protocol
      (Ghostty and kitty do; Alacritty does not). `true` writes ~/.config/dotfiles/terminal-graphics
      with `kitty`, the marker `show-image` reads.
    '';
  };

  config = {
    # chafa: the one tool that draws the image, in the pane (-f kitty); `-f symbols` is the
    # character approximation that was too coarse to read a diagram on Alacritty (ADR).
    home.packages = [ pkgs.chafa ];

    dotfiles.binaries.".local/bin" = [ "show-image" ];

    xdg.configFile."dotfiles/terminal-graphics" = lib.mkIf cfg.kittyProtocol {
      text = "kitty\n";
    };
  };
}
