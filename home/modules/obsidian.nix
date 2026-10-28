# Personal Obsidian client and off-site backup.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  nixGLWrap = import ./nixgl.nix { inherit pkgs; };
  obsidianPackage = nixGLWrap pkgs.obsidian;
  backupPath = "${config.home.homeDirectory}/.local/bin/obsidian-backup";
  vaultPath = "${config.home.homeDirectory}/Documents/obsidian/personal";
  servicePath = lib.makeBinPath [
    pkgs.bash
    pkgs.bws
    pkgs.coreutils
    pkgs.diffutils
    pkgs.findutils
    pkgs.herdr
    pkgs.jq
    pkgs.libsecret
    pkgs.restic
  ];
  # 成功の印（ファイルの mtime）。バックアップ本体の unit が成功したときだけ、unit の ExecStartPost が
  # 更新する。本体の実行ファイルが無い（203/EXEC）など、ExecStart が失敗すると ExecStartPost は走らず、
  # 印は古いままになる。印を読む側は本体に依存しない（下の staleCheck、#1170 の完了条件 3）。
  backupStamp = "${config.home.homeDirectory}/.local/state/dotfiles/obsidian-backup-last-success";
  staleDays = 3;
  staleCheck = pkgs.writeShellApplication {
    name = "obsidian-backup-stale-check";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.findutils
      pkgs.herdr
    ];
    text = ''
      # 印が無い間（この検査を入れた直後など）は、最初の検査の時刻から猶予を数える。
      stamp=${lib.escapeShellArg backupStamp}
      mkdir -p "$(dirname "$stamp")"
      [ -e "$stamp" ] || touch "$stamp"
      if [ -n "$(find "$stamp" -mtime -${toString staleDays})" ]; then
        exit 0
      fi
      msg="Obsidian backup has not succeeded for ${toString staleDays}+ days (last success: $(date -r "$stamp" '+%F %T')); inspect systemctl --user status obsidian-backup*"
      printf '%s\n' "$msg" >&2
      herdr notification show "Obsidian backup stale" --body "$msg" --sound request >/dev/null 2>&1 || true
      exit 1
    '';
  };
  serviceBase = {
    Unit = {
      Description = "Back up the personal Obsidian vault to Backblaze B2";
      After = [ "graphical-session.target" ];
    };
    Service = {
      Type = "oneshot";
      Environment = [
        "PATH=${servicePath}"
        "OBSIDIAN_BACKUP_BWS_BIN=${pkgs.bws}/bin/bws"
        "OBSIDIAN_BACKUP_ENV_BIN=${pkgs.coreutils}/bin/env"
        "OBSIDIAN_BACKUP_FIND_BIN=${pkgs.findutils}/bin/find"
        "OBSIDIAN_BACKUP_HERDR_BIN=${pkgs.herdr}/bin/herdr"
        "OBSIDIAN_BACKUP_JQ_BIN=${pkgs.jq}/bin/jq"
        "OBSIDIAN_BACKUP_RESTIC_BIN=${pkgs.restic}/bin/restic"
        "OBSIDIAN_BACKUP_SECRET_TOOL_BIN=${pkgs.libsecret}/bin/secret-tool"
        "OBSIDIAN_BACKUP_VAULT=${vaultPath}"
        "OBSIDIAN_BACKUP_HOST=vega"
      ];
      NoNewPrivileges = true;
      PrivateTmp = true;
      UMask = "0077";
    };
  };
in
{
  imports = [ ./bitwarden.nix ];

  home.packages = [
    obsidianPackage
    pkgs.restic
  ];

  # bash 版(scripts/obsidian-backup、`--selftest` あり)。#683 が Rust 版(crates/obsidian-backup)
  # への参照だけを入れ、Rust 版は追加されなかったため、2026-10-03 から B2 へのバックアップが
  # 取れていなかった(#1170)。Rust 版が実在し、参照の実在が評価時に検査されるまで、この形を保つ。
  home.file.".local/bin/obsidian-backup" = {
    source = ../../scripts/obsidian-backup;
    executable = true;
  };
  dotfiles.quarantine.managedFiles = [ ".local/bin/obsidian-backup" ];

  # vault の場所の宣言(~/.config/dotfiles/obsidian-vault の 1 行目)。worklog(crates/worklog、
  # docs/agents/worklog.md)と vault-note(wrap-up の気づき・retro・裁定待ちの写し、
  # docs/agents/wrapup-inbox.md)が、hub-resolve の OBSIDIAN_VAULT として同じマーカーを読む
  # 単一の正本。SessionStart hook は agents.nix が全ホストに配備するが、このマーカーが無い機体では
  # 何も注入せず、気づきは機体の jsonl に溜まるだけ — vault を宣言した機体(このモジュールを読む
  # 機体)だけで vault への書き込みが効く。
  home.file.".local/bin/worklog".source = "${pkgs.dotfiles-tools}/bin/worklog";
  xdg.configFile."dotfiles/obsidian-vault".text = "${vaultPath}\n";

  # worklog / vault-note(エージェントが Bash で呼ぶ)が書く場所。sandbox の中では読み取り専用で
  # `Read-only file system` になる(2026-10-28 に vega で実測、#1164)。vault は vega のこの
  # モジュールだけが宣言するので、書き込みの許可も同じ場所で足す。
  dotfiles.claudePermissions.allowWrite = [ vaultPath ];

  systemd.user.services.obsidian-backup = lib.recursiveUpdate serviceBase {
    Service.ExecStart = "${backupPath} backup --notify";
    # 成功したときだけ印を更新する（ExecStart が失敗すると走らない）。
    Service.ExecStartPost = "${pkgs.coreutils}/bin/install -D -m 0600 /dev/null ${backupStamp}";
  };

  # 本体の unit が「失敗する」ことには `--notify` が気づけるが、unit が起動すらできない
  # （実行ファイルが無い、timer が発火しない）と、失敗の通知も出ない。2026-10-03 から約 25 日、
  # バックアップが取れていなかったのに、`hms` の degraded まで気づかなかった（#1170）。
  # 成功の印が ${toString staleDays} 日より古ければ、本体に依存せず（coreutils・findutils・herdr だけ）知らせる。
  # 失敗した unit は、`hms` の degraded と `systemctl --user --failed` にも残る。
  systemd.user.services.obsidian-backup-stale-check = {
    Unit.Description = "Check that the Obsidian vault backup succeeded within the last ${toString staleDays} days";
    Service = {
      Type = "oneshot";
      ExecStart = "${staleCheck}/bin/obsidian-backup-stale-check";
      NoNewPrivileges = true;
      PrivateTmp = true;
    };
  };

  systemd.user.services.obsidian-backup-maintenance = lib.recursiveUpdate serviceBase {
    Unit.Description = "Prune and check the personal Obsidian restic repository";
    Service.ExecStart = "${backupPath} maintenance --notify";
  };

  systemd.user.services.obsidian-backup-restore-test = lib.recursiveUpdate serviceBase {
    Unit.Description = "Restore and compare Obsidian acceptance fixtures";
    Service.ExecStart = lib.concatStringsSep " " [
      backupPath
      "restore-test"
      "--notify"
      "acceptance/roundtrip.md"
      "acceptance/attachment.png"
      "acceptance/attachment.pdf"
    ];
  };

  dotfiles.pipeline.timerNodes.obsidian-backup = "obsidian-backup";
  systemd.user.timers.obsidian-backup = {
    Unit.Description = "Back up the personal Obsidian vault daily";
    Timer = {
      OnCalendar = "daily";
      Persistent = true;
      RandomizedDelaySec = "30min";
      Unit = "obsidian-backup.service";
    };
    Install.WantedBy = [ "timers.target" ];
  };

  dotfiles.pipeline.timerNodes.obsidian-backup-stale-check = "obsidian-backup-stale-check";
  systemd.user.timers.obsidian-backup-stale-check = {
    Unit.Description = "Check the Obsidian backup is not stale, daily";
    Timer = {
      OnCalendar = "daily";
      Persistent = true;
      RandomizedDelaySec = "1h";
      Unit = "obsidian-backup-stale-check.service";
    };
    Install.WantedBy = [ "timers.target" ];
  };

  dotfiles.pipeline.timerNodes.obsidian-backup-maintenance = "obsidian-backup-maintenance";
  systemd.user.timers.obsidian-backup-maintenance = {
    Unit.Description = "Prune and check the Obsidian backup weekly";
    Timer = {
      OnCalendar = "weekly";
      Persistent = true;
      RandomizedDelaySec = "2h";
      Unit = "obsidian-backup-maintenance.service";
    };
    Install.WantedBy = [ "timers.target" ];
  };

  # weekly maintenance's `restic check` only validates repository metadata, it
  # never reads the actual snapshot payload back. This timer exercises the
  # real restore path against the acceptance fixtures monthly, so a broken
  # restore is caught before it is needed for real. Interval is monthly, not
  # weekly like maintenance, because the shortest retention tier is
  # `--keep-daily 7` — a month between checks still leaves ample time to
  # notice and fix a restore failure before older snapshots age out.
  dotfiles.pipeline.timerNodes.obsidian-backup-restore-test = "obsidian-backup-restore-test";
  systemd.user.timers.obsidian-backup-restore-test = {
    Unit.Description = "Restore and compare the Obsidian acceptance fixtures monthly";
    Timer = {
      OnCalendar = "monthly";
      Persistent = true;
      RandomizedDelaySec = "6h";
      Unit = "obsidian-backup-restore-test.service";
    };
    Install.WantedBy = [ "timers.target" ];
  };
}
