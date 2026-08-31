# Herdr — terminal workspace manager for AI coding agents.
#
# バイナリは nixpkgs(unstable overlay、flake.nix 参照)から入れる。以前の
# self-installed ~/.local/bin/herdr は PATH で ~/.nix-profile/bin より先に来る
# (packages.nix の claude-code と同じ罠)ので、残っていると旧版が nix の herdr を
# shadow し続ける。activation で自動的に退避する(下記)。
#
# nix 管理下では self-update は herdr 自身が無効化する — 実行バイナリの静的解析で
# 確認済みの文字列: "self-update is disabled for Nix installs; update with
# 'nix profile upgrade' or update the flake input that provides Herdr"。
# バージョンは `nix flake update nixpkgs-unstable` で上げる。
#
# nixGL wrapper は不要: ADR-0006 の wrapper は nix プロセスが mesa/EGL を dlopen
# する GUI アプリのためのもので、herdr は端末エスケープシーケンスで描画する TUI。
# GL を持つのはホスト側の alacritty である。
#
# config.toml は literal を verbatim 配備(ADR-0002、alacritty.toml と同型)。
# read-only symlink になるので、herdr の実行時書き込み(in-TUI の theme / sound /
# toast / status indicators / agent border labels トグル、onboarding、
# channel set)は失敗する。ただしこれは「見える失敗」ではない — herdr の
# logging::config_write_failed がログに記録して飲み込み、
# apply_config_from_disk() が読み直すので、UI のトグルが黙って元に戻るだけである。
# 設定変更はこのリポジトリを編集して `home-manager switch`、反映は
# `herdr server reload-config`(alacritty / starship / git と同じ運用)。
{
  lib,
  pkgs,
  ...
}:
{
  home.packages = [ pkgs.herdr ];

  xdg.configFile."herdr/config.toml".source = ../../config/herdr/config.toml;

  # 自前インストールの ~/.local/bin/herdr は PATH で ~/.nix-profile/bin に先行する
  # ので、残っていると旧版が nix の herdr を shadow し続ける。desktop.nix の
  # quarantineStrayFcitx5Autostart と同型に、消さずに改名して退避する
  # (「手で消すこと」という手順書は、ゼロ手作業を憲章にした repo には置けない)。
  # symlink(store 由来のもの)は対象外 — 二重管理を避けるため、real file だけを
  # 見る。
  home.activation.quarantineSelfInstalledHerdr = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    stray="$HOME/.local/bin/herdr"
    if [ -f "$stray" ] && [ ! -L "$stray" ]; then
      run mv -f "$stray" "$stray.pre-nix"
    fi
  '';
}
