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
  config,
  lib,
  pkgs,
  ...
}:
let
  registerCodexHooks = pkgs.writeShellScript "register-codex-hooks" (
    builtins.readFile ../../scripts/register-codex-hooks
  );
  registerCopilotHooks = pkgs.writeShellScript "register-copilot-hooks" (
    builtins.readFile ../../scripts/register-copilot-hooks
  );
  codexMetadataCmd = "sh '${config.home.homeDirectory}/.codex/herdr-codex-metadata.sh'";
  copilotMetadataCmd = "sh '${config.home.homeDirectory}/.copilot/hooks/herdr-copilot-metadata.sh'";

  issueCountsPath = "${config.home.homeDirectory}/.local/bin/herdr-issue-counts";
  # herdr(workspace list / report-metadata)、git(remote -v)、gh(auth status /
  # api graphql)だけを呼ぶ。launchd の EnvironmentVariables.PATH は置換なので
  # 同じ一覧を両方に渡す(worktree.nix の git-audit-worktrees と同じ理由)。
  issueCountsServicePath = lib.makeBinPath [
    pkgs.gh
    pkgs.git
    pkgs.herdr
  ];
in
{
  home.packages = [ pkgs.herdr ];

  # サイドバー行の Codex/Copilot 版レポーター(docs/claude/herdr-sidebar-metadata.md)。
  # herdr 自身の integration ファイル(~/.codex/herdr-agent-state.sh、
  # ~/.copilot/hooks/herdr-agent-state.sh、herdr 管理・編集禁止)の隣に置く —
  # herdr 側のヘッダコメントが「custom hooks はこのファイルの隣に置け」と
  # 指示している配置に倣う。
  home.file.".codex/herdr-codex-metadata.sh" = {
    source = ../../config/codex/hooks/herdr-codex-metadata.sh;
    executable = true;
  };
  home.file.".copilot/hooks/herdr-copilot-metadata.sh" = {
    source = ../../config/copilot/hooks/herdr-copilot-metadata.sh;
    executable = true;
  };

  # Codex は worktree.nix の registerCodexWorktreeHooks も同じ
  # ~/.codex/hooks.json を jq で書き換える — lost-update 窓(#61 と同種)を
  # 避けるため明示的にその後ろに順序付ける
  # (installHerdrClaudeIntegration が registerClaudeHooks の後ろに並ぶのと同じ手法)。
  # PreToolUse には登録しない(herdr-codex-metadata.sh のコメント参照)。
  home.activation.registerCodexHerdrMetadataHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCodexWorktreeHooks" ]
      ''
        run ${registerCodexHooks} "$HOME/.codex/hooks.json" \
          SessionStart "" ${lib.escapeShellArg codexMetadataCmd} 10 \
          UserPromptSubmit "" ${lib.escapeShellArg codexMetadataCmd} 10 \
          Stop "" ${lib.escapeShellArg codexMetadataCmd} 10 \
          SessionEnd "" ${lib.escapeShellArg codexMetadataCmd} 10
      '';

  # ~/.copilot/settings.json を activation で書くものは他に無いので
  # writeBoundary だけで足りる。イベント名は Copilot CLI の native camelCase
  # (herdr 自身の PascalCase エントリと共存する — herdr-sidebar-metadata.md 参照)。
  home.activation.registerCopilotHerdrMetadataHooks = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${registerCopilotHooks} "$HOME/.copilot/settings.json" \
      sessionStart ${lib.escapeShellArg "${copilotMetadataCmd} report"} 10 \
      userPromptSubmitted ${lib.escapeShellArg "${copilotMetadataCmd} report"} 10 \
      agentStop ${lib.escapeShellArg "${copilotMetadataCmd} report"} 10 \
      sessionEnd ${lib.escapeShellArg "${copilotMetadataCmd} clear"} 10
  '';

  xdg.configFile."herdr/config.toml".source = ../../config/herdr/config.toml;

  # サイドバーの $oshi トークン(推しマーク絵文字)が引く name→mark 表。
  # 3 エージェントの metadata hook 共通で参照する。docs/claude/herdr-sidebar-metadata.md 参照。
  xdg.configFile."herdr/oshi-marks.tsv".source = ../../config/herdr/oshi-marks.tsv;

  # サイドバー workspace 行の $issues(リポジトリの open Issue 数、PR 除く)。
  # Rust、crates/herdr-issue-counts(ADR-0024)を pkgs.dotfiles-tools から配備し、
  # 5 分毎のタイマーで workspace metadata として報告する(ttl 15 分なので、
  # タイマーが止まれば herdr 自身が値を消す)。drift.nix と同じ「配備 + timer
  # 同居」型。docs/claude/herdr-sidebar-metadata.md「$issues」節参照。
  home.file.".local/bin/herdr-issue-counts".source = "${pkgs.dotfiles-tools}/bin/herdr-issue-counts";

  # exit code は「配信の成否」だけを表す(crates/herdr-issue-counts の
  # exit_code、detect-drift と同じ考え方、#442): herdr 未起動・gh 未認証・
  # GitHub リポの workspace 無しは恒久状態になり得るので 0(stderr に 1 行)、
  # GraphQL・報告の失敗は 1。後者は一過性で、次の成功で failed が消える。
  systemd.user.services.herdr-issue-counts = lib.mkIf pkgs.stdenv.isLinux {
    Unit.Description = "Report each Herdr workspace's open GitHub issue count";
    Service = {
      Type = "oneshot";
      ExecStart = issueCountsPath;
      Environment = "PATH=${issueCountsServicePath}";
      # oneshot の既定 TimeoutStartSec は無限。gh がネットワークで固まっても
      # 次の周期を塞がないよう上限を置く。
      TimeoutStartSec = "2min";
    };
  };

  # Persistent= は書かない — OnCalendar= にしか効かない死んだ宣言になる
  # (drift.nix の #442 の教訓、systemd.timer(5))。
  systemd.user.timers.herdr-issue-counts = lib.mkIf pkgs.stdenv.isLinux {
    Unit.Description = "Refresh Herdr sidebar issue counts every 5 minutes";
    Timer = {
      OnBootSec = "1min";
      OnUnitActiveSec = "5min";
      AccuracySec = "30s";
      Unit = "herdr-issue-counts.service";
    };
    Install.WantedBy = [ "timers.target" ];
  };

  # launchd 版(ADR-0018)。RunAtLoad が systemd の OnBootSec に相当する。
  launchd.agents.herdr-issue-counts = lib.mkIf pkgs.stdenv.isDarwin {
    enable = true;
    config = {
      ProgramArguments = [ issueCountsPath ];
      StartInterval = 300;
      RunAtLoad = true;
      EnvironmentVariables.PATH = issueCountsServicePath;
    };
  };

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
  #
  # herdr バイナリの解決は `${pkgs.herdr}/bin/herdr` を直書きする — `command -v
  # herdr` は使わない。home-manager activation スクリプトの PATH は nix store
  # の coreutils/jq 等に限定され、`~/.nix-profile/bin`(nix-env/home-manager が
  # パッケージをリンクする場所)を含まないため、`command -v herdr` は対話シェル
  # では成功しても activation 内では常に失敗する。このモジュール自身が
  # `home.packages` に `pkgs.herdr` を入れているのでバイナリの存在は自明であり、
  # ゲートとして `command -v` を挟む意味がそもそも無い(#94 で入れたこのゲートが
  # 原因で、当該 activation はどのマシンでも一度も発火していなかった)。
  home.activation.installHerdrClaudeIntegration =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerClaudeHooks" ]
      ''
        if [ ! -e "$HOME/.claude/hooks/herdr-agent-state.sh" ]; then
          run ${pkgs.herdr}/bin/herdr integration install claude || true
        fi
      '';

  # codex/copilot 版。上の installHerdrClaudeIntegration と同じ理由・同じ
  # ゲート方式(ファイル存在、`herdr integration status` のテキストはパース
  # しない)。entryAfter の対象は、対象 hooks.json/settings.json を jq merge
  # する registrar(このモジュール内 registerCodex/CopilotHerdrMetadataHooks、
  # codex はさらに worktree.nix の registerCodexWorktreeHooks)— claude 版と
  # 同じ lost-update 回避(#61 と同種)。
  home.activation.installHerdrCodexIntegration =
    lib.hm.dag.entryAfter
      [
        "writeBoundary"
        "registerCodexWorktreeHooks"
        "registerCodexHerdrMetadataHooks"
      ]
      ''
        if [ ! -e "$HOME/.codex/herdr-agent-state.sh" ]; then
          run ${pkgs.herdr}/bin/herdr integration install codex || true
        fi
      '';

  home.activation.installHerdrCopilotIntegration =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCopilotHerdrMetadataHooks" ]
      ''
        if [ ! -e "$HOME/.copilot/hooks/herdr-agent-state.sh" ]; then
          run ${pkgs.herdr}/bin/herdr integration install copilot || true
        fi
      '';
}
