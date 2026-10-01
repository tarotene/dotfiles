# promotion-detect — daily detector for Q2 (LLM/prose → deterministic)
# promotion and demotion candidates (ADR-543 "既存手段の前倒し接地と、
# 決定論への昇格導線" 段4). The crate (crates/promotion-detect) owns all
# detection logic (recurrence of feedback Issues targeting the same
# skill/hook/AGENTS.md section, verbatim-repeated SKILL.md code blocks,
# stale gate skip files, and skip-heavy gate-events.jsonl entries) and
# writes candidates into this repo's own wrap-up inbox via
# wrapup-stop-gate --check-dup/--add; this module only wires the timer.
#
# Daily cadence (not hourly like due-remind): unlike a missed deadline,
# missing one day's detection run has no real cost — the underlying
# signals (feedback Issue counts, cmd-hash counts, skip-file age) only
# grow monotonically between runs, so the next run catches up. This
# matches gpg-subkey-remind's daily 00:00-style cadence rather than
# due-remind's hourly retry.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  storePath = "${pkgs.dotfiles-tools}/bin/promotion-detect";
  deployedPath = "${config.home.homeDirectory}/.local/bin/promotion-detect";
  # wrapup-stop-gate (Rust since #413, crates/wrapup-stop-gate), which this
  # binary execs, takes its inbox lock with flock(2) itself and no longer
  # needs bash/jq/flock(1)/grep/awk; it still shells out to git and gh.
  servicePath = lib.makeBinPath [
    pkgs.coreutils
    pkgs.git
    pkgs.gh
  ];
in
{
  home.file.".local/bin/promotion-detect" = {
    source = storePath;
    executable = true;
  };

  dotfiles.quarantine.managedFiles = [ ".local/bin/promotion-detect" ];

  systemd.user.services.promotion-detect = lib.mkIf pkgs.stdenv.isLinux {
    Unit.Description = "Detect Q2 promotion/demotion candidates for tarotene/dotfiles (ADR-543)";
    Service = {
      Type = "oneshot";
      ExecStart = deployedPath;
      Environment = "PATH=${servicePath}";
      # gh calls hit the network; don't let a hung request block the next
      # cycle indefinitely (same reasoning as herdr-issue-counts/due-remind).
      TimeoutStartSec = "2min";
    };
  };

  systemd.user.timers.promotion-detect = lib.mkIf pkgs.stdenv.isLinux {
    Unit.Description = "Run promotion-detect once a day";
    Timer = {
      OnCalendar = "daily";
      Persistent = true;
      Unit = "promotion-detect.service";
    };
    Install.WantedBy = [ "timers.target" ];
  };

  # launchd twin (ADR-0018). EnvironmentVariables.PATH replaces (rather than
  # extends) launchd's PATH, so every binary promotion-detect or the
  # wrapup-stop-gate it shells out to needs must be listed explicitly
  # (same reasoning as git-audit-worktrees' launchd agent).
  launchd.agents.promotion-detect = lib.mkIf pkgs.stdenv.isDarwin {
    enable = true;
    config = {
      ProgramArguments = [ storePath ];
      StartCalendarInterval = [
        {
          Hour = 9;
          Minute = 30;
        }
      ];
      EnvironmentVariables.PATH = servicePath;
    };
  };
}
