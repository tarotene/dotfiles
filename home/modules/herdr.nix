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

  # herdr's config.toml is a real file with switch history behind it (a prior
  # -b backup can leave config.toml.backup sitting next to it), so it hits the
  # generic `.backup` collision quarantine (#64) — see home/modules/quarantine.nix
  # for why this is a shared helper rather than a copy of the logic here.
  dotfiles.quarantine.managedFiles = [ ".config/herdr/config.toml" ];

  # 自前インストールの ~/.local/bin/herdr は PATH で ~/.nix-profile/bin に先行する
  # ので、残っていると旧版が nix の herdr を shadow し続ける。desktop.nix の
  # quarantineStrayFcitx5Autostart と同型に、消さずに改名して退避する
  # (「手で消すこと」という手順書は、ゼロ手作業を憲章にした repo には置けない)。
  # symlink(store 由来のもの)は対象外 — 二重管理を避けるため、real file だけを
  # 見る。この経路は herdr 固有(quarantine.nix の対象は xdg.configFile /
  # home.file の管理下ファイルのみ)なのでここに残す。
  #
  # DAG 位置は entryBefore [ "checkLinkTargets" ] — entryAfter [ "writeBoundary" ]
  # では手遅れになる。checkLinkTargets は writeBoundary より前に走るため。
  home.activation.quarantineSelfInstalledHerdr = lib.hm.dag.entryBefore [ "checkLinkTargets" ] ''
    stray="$HOME/.local/bin/herdr"
    if [ -f "$stray" ] && [ ! -L "$stray" ]; then
      run mv -f "$stray" "$stray.pre-nix"
    fi
  '';

  # herdr のネイティブ agent セッション復元(既定 on の
  # `[session] resume_agents_on_restore`)は、herdr 公式 integration hook
  # (~/.claude/hooks/herdr-agent-state.sh)がセッション参照を報告している
  # ペインでしか働かない。この hook は onboarding フロー経由でしか入らず、
  # このリポジトリの config.toml は `onboarding = false` を配備するため
  # onboarding が走らないマシンでは integration が入らないまま — 復元自体は
  # (layout だけの)"success" として記録されるので気づきにくい
  # (docs/claude/herdr-sidebar-metadata.md の共存ノート参照)。
  #
  # ゲートはファイル存在(`herdr integration status` のテキスト出力を
  # パースしない)。導入済みなら no-op なので冪等。`|| true` は herdr サーバ
  # 未起動・オフライン等での install 失敗が switch 自体を止めないための保険
  # (herdr 未インストール環境や headless CI でも无害)。
  #
  # settings.json への書き込みは registerClaudeHooks(claude.nix)と同じ
  # ファイルを対象にするため、jq merge の lost-update 窓(#61 と同種)を
  # 避けて entryAfter で明示的にその後ろに置く。
  home.activation.installHerdrClaudeIntegration =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerClaudeHooks" ]
      ''
        if command -v herdr >/dev/null 2>&1 \
           && [ ! -e "$HOME/.claude/hooks/herdr-agent-state.sh" ]; then
          run herdr integration install claude || true
        fi
      '';
}
