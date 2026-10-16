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
#
# com.bambulab.BambuStudio: 3D プリンタ(施設の Bambu Lab P1S/X1)向けスライサー。
# GL アプリで、zoom と同じく Flatpak が GL ランタイムを自前で持つので nixGL の
# ラップが要らない。nixpkgs の bambu-studio は 02.05.00.67 で、Flathub の
# 2.8.2.61(2026-08-21、`flatpak remote-info --user flathub com.bambulab.BambuStudio`、
# 取得 2026-10-16)より大きく遅れており、工房 PC の版に合わせにくい。
# Linux 専用(altair の darwin はこのモジュール自体が対象外)。
{ lib, pkgs, ... }:
{
  config = lib.mkIf pkgs.stdenv.isLinux {
    services.flatpak = {
      enable = true;
      packages = [
        "us.zoom.Zoom"
        "com.bambulab.BambuStudio"
      ];
    };
  };
}
