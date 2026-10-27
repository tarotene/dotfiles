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
