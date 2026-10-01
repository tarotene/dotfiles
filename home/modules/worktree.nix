# Machine-wide stale-worktree audit and agent guardrails.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  auditPath = "${config.home.homeDirectory}/.local/bin/git-audit-worktrees";
  prunePath = "${config.home.homeDirectory}/.local/bin/git-prune-worktrees";
  # git-prune-branches itself is deployed by home/modules/packages.nix (it
  # predates this module and needs no interpolation of its own), but the
  # auto-prune timer below has to reference its path too.
  pruneBranchesPath = "${config.home.homeDirectory}/.local/bin/git-prune-branches";
  guardPath = "${config.home.homeDirectory}/.local/libexec/git-worktree-create-guard";
  # The Rust ports (#416, crates/git-audit-worktrees and
  # crates/git-worktree-create-guard) are native executables, so these hook
  # commands run them directly rather than through `bash`.
  guardCmd = "'${guardPath}'";
  contextCmd = "'${auditPath}' --context";
  # 旧(bash 経由)の command 文字列。register は command の完全一致で存在判定
  # するので、先に --retire で消さないと旧エントリが残る。
  legacyGuardCmd = "bash '${guardPath}'";
  legacyContextCmd = "bash '${auditPath}' --context";

  registerCodexHooks = pkgs.writeShellScript "register-codex-hooks" (
    builtins.readFile ../../scripts/register-codex-hooks
  );

  # #78: parent checkouts that herdr's Workspace Fork reads HEAD from
  # without fetching first. Explicit allowlist rather than a directory
  # scan (ghr's ~/.ghr tree can contain many repos this host has no
  # opinion about keeping fresh) — this dotfiles checkout is the only
  # entry today; add more paths here as the need arises.
  checkoutFreshnessPaths = [
    "${config.home.homeDirectory}/.ghr/github.com/tarotene/dotfiles"
  ];
  checkoutFreshnessPath = "${config.home.homeDirectory}/.local/bin/git-checkout-freshness";
  # flock(1): Linux ships it via util-linux (already pulled in below). darwin
  # has no native flock, so pkgs.flock (discoteq/flock, a portable C
  # reimplementation, meta.platforms = platforms.all) is added there instead —
  # crates/git-audit-worktrees needs no changes either way, `flock` just
  # resolves to whichever provider is on PATH per platform.
  flockPkg = if pkgs.stdenv.isDarwin then pkgs.flock else pkgs.util-linux;

  # launchd has no "list value becomes duplicate keys" equivalent — a
  # LaunchAgent's ProgramArguments is one flat argv, not a systemd-style
  # sequence of independent ExecStart= commands. Rather than add a wrapper
  # script just to run `git-prune-worktrees --auto` and `git-prune-branches
  # --auto` in a row (a new inline `writeShellScript ''...''` body, which
  # rust-migration.toml's scan tracks as debt on sight, for two lines that
  # don't need to be a script), these are two independent LaunchAgents
  # instead (below). They can race on the very first RunAtLoad tick, but
  # recover within the hour either way — deleting a worktree before its
  # branch is a (usually free) efficiency, not a correctness requirement:
  # `git branch -D` on a branch some other run hasn't yet freed just no-ops
  # into next hour's candidate list, same as any other skipped row.
  autoPruneAgent = path: {
    enable = true;
    config = {
      ProgramArguments = [
        path
        "--auto"
      ];
      StartInterval = 3600;
      RunAtLoad = true;
      # Same env-override injection as the systemd unit above (this
      # function's EnvironmentVariables.PATH is the darwin analogue of that
      # unit's closed nix-store PATH, with the identical gap): both scripts
      # read only the one var matching their own name, so setting both here
      # unconditionally is harmless — it lets one `autoPruneAgent` body serve
      # both LaunchAgents without branching on which `path` it was called
      # with.
      EnvironmentVariables = {
        GIT_PRUNE_WORKTREES_AUDIT_BIN = auditPath;
        GIT_PRUNE_BRANCHES_AUDIT_BIN = auditPath;
        PATH = lib.makeBinPath [
          pkgs.bash
          pkgs.coreutils
          pkgs.findutils
          pkgs.gnugrep
          pkgs.gnused
          pkgs.gawk
          pkgs.git
          pkgs.jq
          pkgs.flock
          pkgs.herdr
          pkgs.gh
        ];
      };
    };
  };
in
{
  home.packages = [ flockPkg ];

  # The three CLIs below and the guard are Rust (ADR-0024, #416): the stable
  # ~/.local/bin / ~/.local/libexec paths the systemd units, launchd agents
  # and git's `git-<subcommand>` lookup rely on stay, but now point at the
  # crane-built binary in pkgs.dotfiles-tools instead of a bash script.
  home.file.".local/bin/git-audit-worktrees".source =
    "${pkgs.dotfiles-tools}/bin/git-audit-worktrees";
  # git-prune-worktrees: the checkout-deleting half of the pair (docs/worktree-lifecycle.md).
  # Deployed as a plain ~/.local/bin executable — same "no alias needed"
  # placement as git-shelve/git-prune-branches (home/modules/packages.nix) —
  # rather than here as one more xdg.configFile, since its default
  # (confirmation-prompting) mode is user-invoked. Its --auto mode is what
  # the git-auto-prune timer below calls; that mode has no prompt.
  home.file.".local/bin/git-prune-worktrees".source =
    "${pkgs.dotfiles-tools}/bin/git-prune-worktrees";
  home.file.".local/libexec/git-worktree-create-guard".source =
    "${pkgs.dotfiles-tools}/bin/git-worktree-create-guard";
  # git-checkout-freshness: fetch + `merge --ff-only` a parent checkout onto
  # origin/<base> when clean and on the default branch (docs/claude/
  # git-checkout-freshness.md) — the one-level-up companion to
  # worktree-fresh-base (crates/worktree-fresh-base), which does the same for a
  # pristine *worktree*.
  home.file.".local/bin/git-checkout-freshness".source =
    "${pkgs.dotfiles-tools}/bin/git-checkout-freshness";

  # Codex owns hooks.json at runtime, as Herdr's integration does. Merge only
  # our two commands and preserve all unrelated entries.
  home.activation.registerCodexWorktreeHooks = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${registerCodexHooks} "$HOME/.codex/hooks.json" \
      --retire PreToolUse ${lib.escapeShellArg legacyGuardCmd} \
      --retire SessionStart ${lib.escapeShellArg legacyContextCmd} \
      --register \
      PreToolUse Bash ${lib.escapeShellArg guardCmd} 10 \
      SessionStart ${lib.escapeShellArg "startup|resume"} ${lib.escapeShellArg contextCmd} 30
  '';

  # Named after the command it runs (git-audit-worktrees), not the other way
  # around — the old "git-worktree-audit" name had the words reversed from
  # the command, which is how a `git worktree-audit` typo actually happened.
  #
  # systemd --user is Linux-only. darwin's equivalent is the launchd.agents
  # block below (ADR-0018) — home-manager's own launchd module asserts
  # `agentPlists != {} -> isDarwin`, so declaring it unconditionally would
  # fail eval on Linux; the mkIf here is required, not just documentation.
  systemd.user.services.git-audit-worktrees = lib.mkIf pkgs.stdenv.isLinux {
    Unit.Description = "Detect stale Git worktree registrations and orphaned checkouts";
    Service = {
      Type = "oneshot";
      ExecStart = "${auditPath} --notify";
      Environment = "PATH=${
        lib.makeBinPath [
          pkgs.bash
          pkgs.coreutils
          pkgs.findutils
          pkgs.git
          pkgs.gnused # crates/git-audit-worktrees の `sed -n 's#^worktree ##p'`
          pkgs.jq
          pkgs.util-linux
          pkgs.herdr
        ]
      }";
    };
  };

  systemd.user.timers.git-audit-worktrees = lib.mkIf pkgs.stdenv.isLinux {
    Unit.Description = "Check for stale Git worktrees every minute";
    Timer = {
      OnBootSec = "1min";
      OnUnitActiveSec = "1min";
      AccuracySec = "1s";
      Persistent = true;
      Unit = "git-audit-worktrees.service";
    };
    Install.WantedBy = [ "timers.target" ];
  };

  # launchd equivalent of the systemd timer+service pair above. StartInterval
  # (seconds) is launchd's OnUnitActiveSec; RunAtLoad covers OnBootSec (fires
  # once when the agent is first loaded, then every StartInterval thereafter).
  launchd.agents.git-audit-worktrees = lib.mkIf pkgs.stdenv.isDarwin {
    enable = true;
    config = {
      ProgramArguments = [
        auditPath
        "--notify"
      ];
      StartInterval = 60;
      RunAtLoad = true;
      # launchd's EnvironmentVariables *replaces* the agent's PATH rather than
      # extending it (unlike systemd's Environment=), so every binary
      # git-audit-worktrees actually shells out to must be listed explicitly.
      # coreutils/findutils do not provide grep/sed/awk — those are separate
      # nixpkgs packages — and the script uses all three (is_shelved,
      # scan_orphaned, prunable_rows/orphaned_rows). Omitting them silently
      # breaks the --notify scan on darwin instead of erroring loudly.
      EnvironmentVariables.PATH = lib.makeBinPath [
        pkgs.bash
        pkgs.coreutils
        pkgs.findutils
        pkgs.gnugrep
        pkgs.gnused
        pkgs.gawk
        pkgs.git
        pkgs.jq
        pkgs.flock
        pkgs.herdr
      ];
    };
  };

  # git-checkout-freshness timer (#78). 10-minute interval: frequent enough
  # that a parent checkout rarely sits more than 10 minutes stale before the
  # next Workspace Fork, but far less chatty against origin than
  # git-audit-worktrees' 1-minute read-only scan (this one does a real
  # `git fetch` per path per run).
  systemd.user.services.git-checkout-freshness = lib.mkIf pkgs.stdenv.isLinux {
    Unit.Description = "Fast-forward parent Git checkouts to origin/<base>";
    Service = {
      Type = "oneshot";
      ExecStart = "${checkoutFreshnessPath} ${
        lib.concatMapStringsSep " " lib.escapeShellArg checkoutFreshnessPaths
      }";
      Environment = "PATH=${
        lib.makeBinPath [
          pkgs.bash
          pkgs.coreutils
          pkgs.git
          pkgs.util-linux # timeout(1)
        ]
      }";
    };
  };

  systemd.user.timers.git-checkout-freshness = lib.mkIf pkgs.stdenv.isLinux {
    Unit.Description = "Keep parent Git checkouts fresh every 10 minutes";
    Timer = {
      OnBootSec = "2min";
      OnUnitActiveSec = "10min";
      AccuracySec = "30s";
      Persistent = true;
      Unit = "git-checkout-freshness.service";
    };
    Install.WantedBy = [ "timers.target" ];
  };

  launchd.agents.git-checkout-freshness = lib.mkIf pkgs.stdenv.isDarwin {
    enable = true;
    config = {
      ProgramArguments = [ checkoutFreshnessPath ] ++ checkoutFreshnessPaths;
      StartInterval = 600;
      RunAtLoad = true;
      EnvironmentVariables.PATH = lib.makeBinPath [
        pkgs.bash
        pkgs.coreutils
        pkgs.git
      ];
    };
  };

  # git-auto-prune: unattended deletion of content-preservation-evidence-
  # backed worktrees/branches (C1/C2/C3, docs/worktree-lifecycle.md) — a
  # separate, hourly timer from git-audit-worktrees' 1-minute read-only scan
  # above, deliberately: this one shells out to `gh` (per repo with a
  # candidate) and actually deletes, neither of which the 1-minute detection
  # loop may do. Worktrees first, then branches — a worktree's branch can't
  # be `-D`'d while checked out, so pruning the worktree first is what lets
  # that branch become a prune-branches candidate in the same run
  # (docs/worktree-lifecycle.md's "worktree → branch の順で畳む").
  #
  # Two ExecStart= lines, not a wrapper script: home-manager's systemd
  # module renders a list value as duplicate keys (`listsAsDuplicateKeys`),
  # which is exactly systemd's own native "run these in order" — no new
  # script to add to rust-migration.toml's scan, no inline shell either.
  systemd.user.services.git-auto-prune = lib.mkIf pkgs.stdenv.isLinux {
    Unit.Description = "Delete worktrees/branches backed by content-preservation evidence (C1/C2/C3)";
    Service = {
      Type = "oneshot";
      # #586: a hard ceiling so a hung `gh` can never pile an hourly tick
      # on top of a still-running previous one. --evidence asks GitHub once
      # per candidate sha (not once per PR), so a healthy run takes seconds;
      # 15 min is generous headroom, not an expected duration.
      TimeoutStartSec = "15min";
      ExecStart = [
        "${prunePath} --auto"
        "${pruneBranchesPath} --auto"
      ];
      # GIT_PRUNE_{WORKTREES,BRANCHES}_AUDIT_BIN=${auditPath}: both scripts
      # otherwise resolve `git-audit-worktrees` off PATH (their AUDIT_BIN
      # default), but this unit's PATH= below is a closed nix-store list that
      # deliberately excludes ~/.local/bin — every `git-*` binary that
      # dotfiles deploys reaches PATH via `git-<subcommand>` (git's own
      # dispatch), not by adding the user's bin dir here. Passing the
      # absolute path through the env override both scripts already support
      # (used by their own selftests to inject a stub) is the actual
      # injection point, not a wider PATH. Its absence here made both
      # `--auto` invocations exit 127 on every hourly tick since #544 landed
      # (2026-09-29, arcturus).
      Environment = [
        "GIT_PRUNE_WORKTREES_AUDIT_BIN=${auditPath}"
        "GIT_PRUNE_BRANCHES_AUDIT_BIN=${auditPath}"
        "PATH=${
          lib.makeBinPath [
            pkgs.bash
            pkgs.coreutils
            pkgs.findutils
            pkgs.gnugrep
            pkgs.gnused
            pkgs.gawk
            pkgs.git
            pkgs.jq
            pkgs.util-linux
            pkgs.herdr
            pkgs.gh
          ]
        }"
      ];
    };
  };

  systemd.user.timers.git-auto-prune = lib.mkIf pkgs.stdenv.isLinux {
    Unit.Description = "Run git-auto-prune every hour";
    Timer = {
      # OnCalendar=, not OnBootSec=/OnUnitActiveSec= alone: Persistent= only
      # does anything for a calendar timer (home/modules/herdr.nix's #442
      # comment has the same lesson) — this one should still catch up after
      # the machine was asleep/off, unlike the 1-minute detection timer
      # above (a missed detection cycle is harmless; catching up here means
      # evidence that has been sitting deletable for a while actually gets
      # deleted promptly on resume, instead of waiting for the next natural
      # hourly tick).
      OnCalendar = "hourly";
      RandomizedDelaySec = "5min";
      Persistent = true;
      Unit = "git-auto-prune.service";
    };
    Install.WantedBy = [ "timers.target" ];
  };

  launchd.agents.git-auto-prune-worktrees = lib.mkIf pkgs.stdenv.isDarwin (autoPruneAgent prunePath);
  launchd.agents.git-auto-prune-branches = lib.mkIf pkgs.stdenv.isDarwin (
    autoPruneAgent pruneBranchesPath
  );
}
