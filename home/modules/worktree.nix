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

  registerCodexHooks = pkgs.writeShellScript "register-codex-worktree-hooks" (
    builtins.readFile ../../scripts/register-codex-worktree-hooks
  );
in
{
  home.packages = [ pkgs.util-linux ];

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
      ${lib.escapeShellArg guardCmd} \
      ${lib.escapeShellArg contextCmd}
  '';

  # Named after the command it runs (git-audit-worktrees), not the other way
  # around — the old "git-worktree-audit" name had the words reversed from
  # the command, which is how a `git worktree-audit` typo actually happened.
  systemd.user.services.git-audit-worktrees = {
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

  systemd.user.timers.git-audit-worktrees = {
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
}
