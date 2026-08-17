# Claude Code plan-review workflow — an automatic Codex CLI review gate on
# ExitPlanMode (deny→revise loop, max 2 rounds/session, fail-open), plus a
# manual /codex-plan-review advisory command.
#
# Hybrid translation (ADR-0002): the hook script and the slash command stay
# literal under config/claude/ and are deployed via home.file. The hook
# no-ops silently on hosts without a codex binary (ADR-0005's
# binary-existence gating), so it is deployed unconditionally to every host —
# enabling/disabling per machine is a matter of whether codex is installed,
# not of home-manager configuration.
#
# ~/.claude/settings.json is Claude-Code-owned (the CLI rewrites it at
# runtime), so the hook *registration* cannot be a store symlink — the same
# constraint as the fcitx5 profile in desktop.nix. Instead it is merged
# idempotently at activation time: the PreToolUse entry is injected only if
# an identical one is missing, and nothing else in the file is touched.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  repoConfig = ../../config;
  hookCmd = "bash '${config.home.homeDirectory}/.claude/hooks/codex-plan-review.sh'";

  registerHook = pkgs.writeShellScript "register-claude-plan-review-hook" ''
    set -eu
    settings="$1"
    hook_cmd="$2"
    jq=${pkgs.jq}/bin/jq

    if [ ! -f "$settings" ]; then
      mkdir -p "$(dirname "$settings")"
      printf '{}\n' > "$settings"
    fi

    if "$jq" -e --arg cmd "$hook_cmd" \
        '[.hooks.PreToolUse[]? | select(.matcher == "ExitPlanMode")
          | .hooks[]? | select(.command == $cmd)] | length > 0' \
        "$settings" >/dev/null; then
      exit 0
    fi

    tmp="$(mktemp)"
    "$jq" --arg cmd "$hook_cmd" \
      '.hooks.PreToolUse = ((.hooks.PreToolUse // []) + [{
        matcher: "ExitPlanMode",
        hooks: [{ type: "command", command: $cmd, timeout: 300 }]
      }])' \
      "$settings" > "$tmp"
    mv "$tmp" "$settings"
  '';
in
{
  home.file.".claude/hooks/codex-plan-review.sh" = {
    source = repoConfig + "/claude/hooks/codex-plan-review.sh";
    executable = true;
  };

  # The command file hardcodes /home/tarotene — fine while every identity
  # pins home.username = "tarotene" (identities/*.nix). Revisit if a host
  # ever overrides the username.
  home.file.".claude/commands/codex-plan-review.md".source =
    repoConfig + "/claude/commands/codex-plan-review.md";

  home.activation.registerClaudePlanReviewHook = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${registerHook} "$HOME/.claude/settings.json" ${lib.escapeShellArg hookCmd}
  '';
}
