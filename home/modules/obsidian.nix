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
  # hour.
  #
  # bws 2.0.0 has no implicit US-cloud default: an absent server_base fails
  # with "Profile has no server_base or server_identity" before ever touching
  # Bitwarden, so it must be set explicitly. https://vault.bitwarden.com is
  # the confirmed value for the US cloud (Bitwarden staff,
  # https://community.bitwarden.com/t/what-is-the-correct-url-for-server-base/57673,
  # 取得 2026-09-23).
  #
  # state_opt_out is a TOML *string*, not a boolean — bws 2.0.0's own `bws
  # config state-opt-out true` writes `state_opt_out = "true"`; a bare `true`
  # fails config parsing outright (crates/bws/src/config.rs) and every `bws
  # run` invocation errors before touching Bitwarden at all.
  xdg.configFile."bws/config".text = ''
    [profiles.default]
    server_base = "https://vault.bitwarden.com"
    state_opt_out = "true"
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

  # weekly maintenance's `restic check` only validates repository metadata, it
  # never reads the actual snapshot payload back. This timer exercises the
  # real restore path against the acceptance fixtures monthly, so a broken
  # restore is caught before it is needed for real. Interval is monthly, not
  # weekly like maintenance, because the shortest retention tier is
  # `--keep-daily 7` — a month between checks still leaves ample time to
  # notice and fix a restore failure before older snapshots age out.
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
