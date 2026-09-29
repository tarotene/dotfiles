# Flatpak declarations (#24、ADR-0001 4th escape hatch)。zoom-us が
# nixGL wrap 越しの GLX 結線に失敗する(bwrap+FHS 越しに nix mesa と
# glvnd を橋渡しできない、upstream NixOS/nixpkgs#267663 と同一症状、
# 2年以上 stale)ため、GL ランタイムを自己完結で持つ Flatpak に置き換える。
#
# flatpak 自体(D-Bus system service・xdg-desktop-portal 登録)は root-owned
# システム統合なので home-manager では提供できず、apt 側(packages/
# declarative/apt-packages.txt)で宣言する。このモジュールが宣言するのは
# 「どの Flatpak app をインストールするか」だけ(nix-flatpak が内部で
# システムの flatpak バイナリを呼ぶ)。
{ lib, pkgs, ... }:
{
  config = lib.mkIf pkgs.stdenv.isLinux {
    services.flatpak = {
      enable = true;
      packages = [ "us.zoom.Zoom" ];
    };
  };
}
