# detect-drift(Issue #4): apt/cargo/npm -g/pipx の ad-hoc install を検出
# 専用で報告する。git-audit-worktrees(worktree.nix)と同じ「配備 +
# systemd/launchd timer を同じモジュールに同居させる」型。
{
  config,
  lib,
  pkgs,
  ...
}:
let
  driftPath = "${config.home.homeDirectory}/.local/bin/detect-drift";

  # apt-mark(/usr/bin)・rustup 管理の cargo(~/.cargo/bin)・mise shim の
  # npm(~/.local/share/mise/shims、ADR-0002)・pipx(apt/pip 経由なら
  # /usr/bin または ~/.local/bin)を横断して見つける必要がある —
  # git-audit-worktrees の PATH(nix store のみ)とは違い、これらは
  # いずれも nix の管轄外(rustup/mise は ADR-0002 の per-project runtime
  # escape hatch、apt-mark/pipx はシステム/pip 由来)。見つからない層は
  # detect-drift 自身が ADR-0005 に倣って黙って skip するので、ここでの
  # PATH 漏れは即エラーにはならず「その層だけ検出できない」に留まる。
  driftServicePath =
    lib.makeBinPath [ pkgs.nix-index ]
    + ":${config.home.homeDirectory}/.local/bin:${config.home.homeDirectory}/.local/share/mise/shims:${config.home.homeDirectory}/.cargo/bin:/usr/bin:/bin";
in
{
  # Rust、crates/detect-drift(ADR-0024)経由で pkgs.dotfiles-tools から配備。
  # 宣言ファイル(packages/declarative/apt-packages.txt)も配備する —
  # 実行ファイルは `~/.local/bin` に置かれるため checkout 相対の解決が
  # できず、`apply-rulesets.sh`(#417)と同じ理由で配備先を必要とする。
  home.file.".local/bin/detect-drift".source = "${pkgs.dotfiles-tools}/bin/detect-drift";
  xdg.configFile."dotfiles/apt-packages.txt".source = ../../packages/declarative/apt-packages.txt;

  # 週次実行(#4 の設計どおり)。毎分実行の git-audit-worktrees と違い、
  # apt-mark/cargo/npm/pipx の照会はネットワーク I/O や比較的重い
  # プロセス起動を伴いうるため、頻度を抑える。
  #
  # `--file-issue` の exit code は「drift の有無」ではなく「配信の成否」
  # (crates/detect-drift の `exit_code`)を表す: 0 = 起票/コメント成功、
  # または ADR-0025 フィルタ後に報告対象ゼロ、3 = 起票そのものに失敗。
  # cargo/npm/pipx は宣言ファイルを持たず在庫全件が drift 扱いになる
  # (#4 の設計)ため、旧来の「drift あり=exit 1」を unit の失敗条件にすると
  # 恒久的に failed になっていた(2026-09-24、hms の degraded 報告で発覚)。
  systemd.user.services.detect-drift = lib.mkIf pkgs.stdenv.isLinux {
    Unit.Description = "Detect apt/cargo/npm/pipx installs outside their declaration";
    Service = {
      Type = "oneshot";
      ExecStart = "${driftPath} --file-issue tarotene/dotfiles";
      Environment = "PATH=${driftServicePath}";
    };
  };

  # `Persistent=` は OnCalendar= timer にのみ効く(systemd.timer(5))。旧来の
  # OnBootSec=/OnUnitActiveSec= の組み合わせでは死んだ宣言になり、ログイン
  # (boot)毎に再発火していた(2026-09-24 実測、1日に3回発火し #440 に
  # 同じ内容のコメントが3回積まれた)。OnCalendar=weekly に切り替え、
  # RandomizedDelaySec でログイン直後のネットワーク未復帰との衝突を薄める。
  systemd.user.timers.detect-drift = lib.mkIf pkgs.stdenv.isLinux {
    Unit.Description = "Check for ad-hoc install drift weekly";
    Timer = {
      OnCalendar = "weekly";
      Persistent = true;
      RandomizedDelaySec = "30min";
      Unit = "detect-drift.service";
    };
    Install.WantedBy = [ "timers.target" ];
  };

  # launchd 版(ADR-0018)。EnvironmentVariables.PATH は systemd の
  # Environment= と違い置換なので、同じ一覧をそのまま渡す。
  launchd.agents.detect-drift = lib.mkIf pkgs.stdenv.isDarwin {
    enable = true;
    config = {
      ProgramArguments = [
        driftPath
        "--file-issue"
        "tarotene/dotfiles"
      ];
      StartInterval = 604800; # 7d
      RunAtLoad = false; # #4 は週次前提。ログイン毎の実行は意図しない
      EnvironmentVariables.PATH = driftServicePath;
    };
  };
}
