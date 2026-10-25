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
        "OBSIDIAN_BACKUP_HERDR_BIN=${pkgs.herdr}/bin/herdr"
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

  # Rust 実装(crates/obsidian-backup、#414)。jq / find / cmp は内部化したので
  # OBSIDIAN_BACKUP_{JQ,FIND}_BIN は廃止(servicePath の findutils/jq/diffutils/bash も
  # 不要だが、PATH は restic など外部コマンドの解決にそのまま渡るので触らない)。
  home.file.".local/bin/obsidian-backup".source = "${pkgs.dotfiles-tools}/bin/obsidian-backup";
  dotfiles.quarantine.managedFiles = [ ".local/bin/obsidian-backup" ];

  # worklog: 当日ノートへ作業ログを 1 行ずつ追記し、読む CLI(crates/worklog、
  # docs/claude/worklog.md)。vault の場所はこのモジュールの vaultPath を、hub-resolve と
  # 同じマーカー(~/.config/dotfiles/worklog-vault の 1 行目)で渡す。SessionStart hook は
  # claude.nix が全ホストに配備するが、このマーカーが無い機体では何も注入しない —
  # vault を宣言した機体(このモジュールを読む機体)だけで規範が効く。
  home.file.".local/bin/worklog".source = "${pkgs.dotfiles-tools}/bin/worklog";
  xdg.configFile."dotfiles/worklog-vault".text = "${vaultPath}\n";

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

  # pipeline-node: obsidian-backup
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

  # pipeline-node: obsidian-backup-maintenance
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
  # pipeline-node: obsidian-backup-restore-test
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
