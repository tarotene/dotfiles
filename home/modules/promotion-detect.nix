# promotion-detect — daily detector for Q2 (LLM/prose → deterministic)
# promotion and demotion candidates (ADR-543 "既存手段の前倒し接地と、
# 決定論への昇格導線" 段4). The crate (crates/promotion-detect) owns all
# detection logic (recurrence of feedback Issues targeting the same
# skill/hook/AGENTS.md section, verbatim-repeated SKILL.md code blocks,
# stale gate skip files, and skip-heavy gate-events.jsonl entries) and
# writes candidates into this repo's own wrap-up inbox via
# issue-inbox --check-dup/--add; this module only wires the timer.
# It also calibrates the retro's as-code-candidate vocabulary (ADR-1276): it
# reads the vault's inbox/ notes (the vault is found through the same
# OBSIDIAN_VAULT / marker-file declaration the other tools use) and counts
# adopted vs dropped candidates per signal.
#
# At the end of each run it also writes a liveness record for the owner's
# Issue ledger and checks that the cloud's daily digest is still arriving
# (ADR-1382, ledger_heartbeat). The ledger repository is private, so its
# name is not here: the crate reads it from the machine-local file
# ~/.config/ledger-heartbeat/target and does nothing when that is absent.
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
  # issue-inbox (Rust, crates/issue-inbox), which this
  # binary execs, takes its inbox lock with flock(2) itself and no longer
  # needs bash/jq/flock(1)/grep/awk; it still shells out to git and gh.
  # herdr: 本人の Issue 台帳の日次の通知が途絶えたときの通知(ADR-1382)。due-remind と同じ
  # `herdr notification show`。
  servicePath = lib.makeBinPath [
    pkgs.coreutils
    pkgs.git
    pkgs.gh
    pkgs.herdr
  ];
  timerPair = import ../lib/timer-pair.nix { inherit lib pkgs; };
in
{
  imports = [
    (timerPair {
      name = "promotion-detect";
      description = "Detect Q2 promotion/demotion candidates for tarotene/dotfiles (ADR-543)";
      timerDescription = "Run promotion-detect once a day";
      exec = deployedPath;
      launchdExec = storePath;
      path = servicePath;
      # gh calls hit the network; don't let a hung request block the next
      # cycle indefinitely (same reasoning as herdr-issue-counts/due-remind).
      service.TimeoutStartSec = "2min";
      schedule = {
        calendar = "daily";
        persistent = true;
      };
      launchd.StartCalendarInterval = [
        {
          Hour = 9;
          Minute = 30;
        }
      ];
    })
  ];

  home.file.".local/bin/promotion-detect" = {
    source = storePath;
    executable = true;
  };

  dotfiles.quarantine.managedFiles = [ ".local/bin/promotion-detect" ];

  # launchd 版(ADR-0018)の EnvironmentVariables.PATH は、launchd が PATH を延ばさず置換するので、
  # promotion-detect と、これが exec する issue-inbox が呼ぶ全バイナリを列挙する
  # (git-audit-worktrees の agent と同じ理由)。
  dotfiles.pipeline.timerNodes.promotion-detect = "promotion-detect";
}
