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
    pkgs.findutils
    pkgs.herdr
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
        "OBSIDIAN_BACKUP_RESTIC_BIN=${pkgs.restic}/bin/restic"
        "OBSIDIAN_BACKUP_SECRET_TOOL_BIN=${pkgs.libsecret}/bin/secret-tool"
        "OBSIDIAN_BACKUP_VAULT=${vaultPath}"
        "OBSIDIAN_BACKUP_HOST=personal-pop"
      ];
      NoNewPrivileges = true;
      PrivateTmp = true;
      UMask = "0077";
    };
  };
in
{
  home.packages = [
    obsidianPackage
    pkgs.bws
    pkgs.libsecret
    pkgs.restic
  ];

  home.file.".local/bin/obsidian-backup" = {
    source = ../../scripts/obsidian-backup;
    executable = true;
  };
  dotfiles.quarantine.managedFiles = [ ".local/bin/obsidian-backup" ];

  # A revoked machine token must stop working immediately. bws otherwise
  # persists an encrypted session which can outlive revocation for up to an
  # hour. Server endpoints stay at bws's US defaults.
  xdg.configFile."bws/config".text = ''
    [profiles.default]
    state_opt_out = true
  '';

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
}
