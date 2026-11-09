# due-remind — hourly reminder for deadlines recorded in a plaintext due-index
# owned by another (private) repository, per that repository's own
# docs/adr/333-due-index-contract.md. This module is a pure reader: it knows
# only the two-key row contract (`slug`/`due` under
# ${XDG_STATE_HOME}/claude/<domain>/<repo-slug>/due.jsonl`), never a domain's
# own vocabulary, so a new domain over there needs no change here.
#
# Notification is through Herdr, same channel as git-audit-worktrees and
# gpg-subkey-remind (docs/operations.md) — not notify-send/libnotify.
#
# Cadence and the once-a-day de-dup exist because a daily 00:00-style timer
# (like gpg-subkey-remind) would silently drop the reminder on any day Herdr
# happens to have no foreground client at that one moment — this domain's
# deadlines are not something the owner can afford to miss (grill-me session,
# 2026-09-28). The crate itself (crates/due-remind) owns the once-a-day state
# file and the retry/failure classification; this module only wires the timer.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  storePath = "${pkgs.dotfiles-tools}/bin/due-remind";
  deployedPath = "${config.home.homeDirectory}/.local/bin/due-remind";
  servicePath = lib.makeBinPath [
    pkgs.coreutils # `date +%F` — see crates/due-remind/src/main.rs::today()
    pkgs.herdr
  ];
  timerPair = import ../lib/timer-pair.nix { inherit lib pkgs; };
in
{
  home.file.".local/bin/due-remind" = {
    source = storePath;
    executable = true;
  };

  # Same `.backup` collision quarantine as gpg-subkey / herdr-issue-counts
  # above — see quarantine.nix for why this is a shared helper.
  dotfiles.quarantine.managedFiles = [ ".local/bin/due-remind" ];

  # OnCalendar= のみを使うので persistent は生きた宣言になる(#442 の教訓 — OnBootSec=/
  # OnUnitActiveSec= だけの herdr-issue-counts では書けない。timerPair の schedule の形が
  # 区別する)。launchd は StartInterval が「起動後 N 秒毎」なので使わず、
  # StartCalendarInterval を時間毎に 11 エントリ並べる(gpg-subkey-remind の 1 エントリ版と同型)。
  # EnvironmentVariables.PATH は launchd が PATH を置換するので、呼び出す全バイナリを列挙する。
  dotfiles.pipeline.timerNodes.due-remind = "due-remind";
  imports = [
    (timerPair {
      name = "due-remind";
      description = "Post a Herdr toast for deadlines due within 7 days (another repo's due-index)";
      timerDescription = "Check due-indexes hourly, 09:00-19:00";
      exec = deployedPath;
      launchdExec = storePath;
      path = servicePath;
      # oneshot の既定 TimeoutStartSec は無限。herdr が固まっても次の周期を
      # 塞がないよう上限を置く(herdr-issue-counts と同じ理由)。
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
}
