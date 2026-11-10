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
# A second timer (promotion-detect-digest-check) runs only the digest check hourly in the daytime (`--digest-check`).
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
    # The daily run also checks the cloud's daily digest (ADR-1382), but the toast is only shown when Herdr is up
    # at that moment; when it was not, no once-a-day mark is written and the next daily run is a day away. This
    # second timer runs only that check (`--digest-check`) hourly in the daytime, the same schedule as due-remind,
    # so the notice is retried the same day. The once-a-day mark is shared with the daily run: whichever shows the
    # notice first, the other stays quiet for the rest of that day. It does nothing when the digest is fresh or the
    # config file is absent.
    (timerPair {
      name = "promotion-detect-digest-check";
      description = "Retry the notice that the ledger's daily digest has stopped arriving (ADR-1382)";
      timerDescription = "Check the ledger's daily digest hourly, 09:00-19:00";
      exec = deployedPath;
      args = [ "--digest-check" ];
      launchdExec = storePath;
      path = servicePath;
      service.TimeoutStartSec = "1min";
      schedule = {
        calendar = "*-*-* 09..19:00:00";
        persistent = true;
      };
      launchd.StartCalendarInterval = map (h: {
        Hour = h;
        Minute = 0;
      }) (lib.range 9 19);
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
  # The same binary with `--digest-check`: one node, two timers (git-auto-prune-* is the same shape).
  dotfiles.pipeline.timerNodes.promotion-detect-digest-check = "promotion-detect";
}
