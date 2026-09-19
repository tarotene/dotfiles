# Machine-wide stale-worktree audit and agent guardrails.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  auditPath = "${config.home.homeDirectory}/.local/bin/git-audit-worktrees";
  guardPath = "${config.home.homeDirectory}/.local/libexec/git-worktree-create-guard";
  guardCmd = "bash '${guardPath}'";
  contextCmd = "bash '${auditPath}' --context";

  registerCodexHooks = pkgs.writeShellScript "register-codex-hooks" (
    builtins.readFile ../../scripts/register-codex-hooks
  );
  # flock(1): Linux ships it via util-linux (already pulled in below). darwin
  # has no native flock, so pkgs.flock (discoteq/flock, a portable C
  # reimplementation, meta.platforms = platforms.all) is added there instead —
  # scripts/git-audit-worktrees needs no changes either way, `flock` just
  # resolves to whichever provider is on PATH per platform.
  flockPkg = if pkgs.stdenv.isDarwin then pkgs.flock else pkgs.util-linux;
in
{
  home.packages = [ flockPkg ];

  home.file.".local/bin/git-audit-worktrees" = {
    source = ../../scripts/git-audit-worktrees;
    executable = true;
  };
  # git-prune-worktrees: the checkout-deleting half of the pair (docs/worktree-lifecycle.md).
  # Deployed as a plain ~/.local/bin executable — same "no alias needed"
  # placement as git-shelve/git-prune-branches (home/modules/packages.nix) —
  # rather than here as one more xdg.configFile, since it's a user-invoked
  # command, not something the audit timer or a Claude/Codex hook calls.
  home.file.".local/bin/git-prune-worktrees" = {
    source = ../../scripts/git-prune-worktrees;
    executable = true;
  };
  home.file.".local/libexec/git-worktree-create-guard" = {
    source = ../../scripts/git-worktree-create-guard;
    executable = true;
  };

  # Codex owns hooks.json at runtime, as Herdr's integration does. Merge only
  # our two commands and preserve all unrelated entries.
  home.activation.registerCodexWorktreeHooks = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${registerCodexHooks} "$HOME/.codex/hooks.json" \
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
}
