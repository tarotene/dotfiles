# Off-site local-state backup; private values arrive through BWS at runtime.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.localStateBackup;
  executable = "${pkgs.dotfiles-tools}/bin/local-state-backup";
  servicePath = lib.makeBinPath [
    pkgs.bws
    pkgs.coreutils
    pkgs.curl
    pkgs.git
    pkgs.libsecret
    pkgs.networkmanager
    pkgs.procps
    pkgs.restic
    pkgs.sqlite
    pkgs.systemd
  ];
  service = operation: {
    Unit = {
      Description = "Local state backup: ${operation}";
      ConditionHost = "vega";
    };
    Service = {
      Type = "oneshot";
      ExecStart = "${executable} ${operation}";
      Environment = [ "PATH=${servicePath}" ];
      UMask = "0077";
      NoNewPrivileges = true;
      PrivateTmp = true;
      # A concurrent backup operation holds a non-blocking shared lock. Retry
      # every service, including long-running monthly operations, after failure.
      Restart = "on-failure";
      RestartSec = "1h";
    };
  };
  timer = operation: schedule: {
    Unit.Description = "Run local state backup ${operation}";
    Timer = {
      OnCalendar = schedule;
      Persistent = true;
      RandomizedDelaySec = "30min";
      Unit = "local-state-backup-${operation}.service";
    };
    Install.WantedBy = [ "timers.target" ];
  };
in
{
  imports = [ ./bitwarden.nix ];
  options.dotfiles.localStateBackup.enable = lib.mkEnableOption "off-site local state backup";
  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = pkgs.stdenv.isLinux;
        message = "local state backup requires Linux user services";
      }
    ];
    home.packages = [ pkgs.restic ];
    systemd.user.services.local-state-backup-backup = service "backup";
    systemd.user.services.local-state-backup-check = service "check";
    systemd.user.services.local-state-backup-maintenance = service "maintenance";
    systemd.user.services.local-state-backup-restore-test = service "restore-test";
    systemd.user.services.local-state-backup-audit = service "audit";
    dotfiles.pipeline.timerNodes.local-state-backup-backup = "local-state-backup-backup";
    dotfiles.pipeline.timerNodes.local-state-backup-check = "local-state-backup-check";
    dotfiles.pipeline.timerNodes.local-state-backup-maintenance = "local-state-backup-maintenance";
    dotfiles.pipeline.timerNodes.local-state-backup-restore-test = "local-state-backup-restore-test";
    dotfiles.pipeline.timerNodes.local-state-backup-audit = "local-state-backup-audit";
    systemd.user.timers.local-state-backup-backup = timer "backup" "daily";
    systemd.user.timers.local-state-backup-check = timer "check" "*-*-01 01:00:00";
    systemd.user.timers.local-state-backup-maintenance = timer "maintenance" "*-*-01 03:00:00";
    systemd.user.timers.local-state-backup-restore-test = timer "restore-test" "*-01,04,07,10-01 07:00:00";
    systemd.user.timers.local-state-backup-audit = timer "audit" "*-*-01 05:00:00";
  };
}
