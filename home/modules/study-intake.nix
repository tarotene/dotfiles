# study-intake — スキャンした手書きノートの取り込みを、timer で無人に回す。
#
# 取り込みの手順そのもの(Drive の取り込み用フォルダの PDF を読み、節ごとの清書ノートを vault に
# 書き、通知文を出す)は、別の非公開リポジトリのスキル `study-intake` が持つ。この module は、
# そのスキルを `claude -p` で定期に起動し、スキルが `--notify-out` に書いた通知文を Herdr の
# デスクトップ通知として送り出すだけ。非公開リポジトリの場所は `workdir` で受け取る
# (名前を公開リポジトリに書けない。ADR-0034 の秘密の名前と同じ扱い)。
#
# 通知の送り出しを、スキルの外のこの 1 箇所に置くのは、無人の `claude -p` の `PushNotification` が
# Remote Control なしでは iPhone に届かないため(2026-11-08 に実測)。iPhone への経路は、候補を
# 実測してから決める。決まったら、送り出し(下の `herdr notification show` の 1 行)だけを差し替える。
#
# 既定は無効。有効にするのは、スキルの検証(実物の手書きでの清書の質、Drive コネクタが無人で
# 使えること)が済んでから。Linux(systemd user)のみで、launchd の双子は持たない。
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.studyIntake;
  home = config.home.homeDirectory;

  stateDir = "${home}/.local/state/dotfiles/study-intake";
  # Claude Code は native installer が正本(packages.nix のコメント、ADR-457)。
  claudeBin = "${home}/.local/bin/claude";

  # 無人の実行で許す道具。ここに無いものは、権限の確認で止まり、実行が失敗する(許可を広げない)。
  allowedTools = [
    "Skill"
    "Read"
    "Write"
    "Edit"
    "Bash(pdftoppm *)"
    "Bash(pdfseparate *)"
    "Bash(pdfunite *)"
    "Bash(jq *)"
    "Bash(base64 *)"
    "Bash(sha256sum *)"
    "Bash(mkdir *)"
    "Bash(ls *)"
    "Bash(stat *)"
    "Bash(date *)"
    "mcp__claude_ai_Google_Drive__search_files"
    "mcp__claude_ai_Google_Drive__download_file_content"
  ];

  runner = pkgs.writeShellApplication {
    name = "study-intake-run";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.herdr
      pkgs.jq
      pkgs.poppler-utils
    ];
    text = ''
      state=${lib.escapeShellArg stateDir}
      workdir=${lib.escapeShellArg (toString cfg.workdir)}
      vault=${lib.escapeShellArg (toString config.dotfiles.obsidian.vaultPath)}
      claude=${lib.escapeShellArg claudeBin}
      notify_file="$state/notify.txt"
      log="$state/last-run.log"

      die() {
        printf 'study-intake-run: %s\n' "$*" >&2
        exit 1
      }

      mkdir -p "$state/tmp"
      rm -f "$notify_file"
      [ -d "$workdir" ] || die "workdir $workdir がありません"
      [ -d "$vault" ] || die "vault $vault がありません"
      [ -x "$claude" ] || die "$claude がありません(native installer で入れる)"

      # claude の作業領域(画像・復元した PDF)を、専用の場所に閉じる。
      export TMPDIR="$state/tmp"
      cd "$workdir"

      rc=0
      timeout ${cfg.timeout} "$claude" -p \
        "スキル study-intake を、引数 --vault $vault --notify-out $notify_file で実行してください。" \
        --add-dir "$vault" --add-dir "$state" \
        --allowedTools ${lib.concatMapStringsSep " " lib.escapeShellArg allowedTools} \
        >"$log" 2>&1 || rc=$?

      # 通知の送り出し。経路を差し替えるときは、この 1 行だけを直す。
      if [ -s "$notify_file" ]; then
        herdr notification show "手書きノート" --body "$(cat "$notify_file")" --sound request >/dev/null 2>&1 || true
      fi

      if [ "$rc" -ne 0 ]; then
        herdr notification show "手書きノートの取り込みに失敗" \
          --body "exit=$rc。ログ: $log" --sound request >/dev/null 2>&1 || true
        exit "$rc"
      fi
    '';
  };
in
{
  options.dotfiles.studyIntake = {
    enable = lib.mkEnableOption "the study-intake timer (runs a private repository's skill with claude -p)";

    workdir = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        スキル `study-intake` を持つリポジトリの作業ツリーの絶対パス。`claude -p` はここを cwd にして起動する。
        非公開リポジトリの場所なので、private wrapper から渡す。enable のとき null は評価で落ちる。
      '';
    };

    calendar = lib.mkOption {
      type = lib.types.str;
      default = "*-*-* 07..23:0/15:00";
      description = ''
        systemd の OnCalendar。1 回の起動は、新しい PDF が無くても `claude -p` を 1 回呼ぶ
        (Drive の一覧を取るため)ので、間隔は使用量とのかね合いで決める。
      '';
    };

    timeout = lib.mkOption {
      type = lib.types.str;
      default = "20min";
      description = "1 回の `claude -p` の上限(coreutils の timeout の書式)。超えると失敗として通知する。";
    };
  };

  config = lib.mkIf (cfg.enable && pkgs.stdenv.isLinux) {
    assertions = [
      {
        assertion = cfg.workdir != null;
        message = "dotfiles.studyIntake.workdir: enable のとき、スキルを持つリポジトリの作業ツリーの絶対パスが要る(private wrapper から渡す)";
      }
    ];

    systemd.user.services.study-intake = {
      Unit.Description = "Run the study-intake skill (scanned handwritten notes -> vault) with claude -p";
      Service = {
        Type = "oneshot";
        ExecStart = "${runner}/bin/study-intake-run";
        # oneshot は、前の実行が残っている間、同じ unit を重ねて起動しない。上限は coreutils の timeout が持つ。
        TimeoutStartSec = "30min";
      };
    };

    dotfiles.pipeline.timerNodes.study-intake = "study-intake";
    systemd.user.timers.study-intake = {
      Unit.Description = "Run the study-intake skill every 15 minutes, 07:00-23:59";
      Timer = {
        OnCalendar = cfg.calendar;
        # 眠っていた間の分を取り戻さない(次の周期で足りる)。
        Persistent = false;
        Unit = "study-intake.service";
      };
      Install.WantedBy = [ "timers.target" ];
    };
  };
}
