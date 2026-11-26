# Antigravity CLI (`agy`) keeps its writable settings.json; reconcile only the declared
# permission keys, from the same sources Claude and Codex read (docs/agents/agent-permissions.md).
#
# Evaluated on every host, applied only where `dotfiles.antigravityPermissions.enable` is set (the
# personal identity: a cloud-connected AI tool does not go to the company hosts, #9). The declaration
# file under agent-native/ is written on every host, because `hms --follow` stops on a missing file
# (crates/hms/src/lib.rs, `policy_changed`).
{
  config,
  lib,
  pkgs,
  ...
}:
let
  settingsReconcile = "${pkgs.dotfiles-tools}/bin/agent-settings-reconcile";
  cfg = config.dotfiles.antigravityPermissions;
  home = config.home.homeDirectory;

  # agy's rule targets are absolute paths or paths relative to a workspace root; `~` is not
  # documented as expanded (Google, Antigravity docs "Permissions", retrieved 2026-11-26).
  absolute = p: if lib.hasPrefix "~/" p then home + lib.removePrefix "~" p else p;

  # Claude's `Bash(P *)` and `Bash(P)` are word-by-word prefix matches; agy's `command(P)` is the
  # same kind of match. A `*` inside the prefix has no counterpart (agy's `*` stands for the whole
  # action), so such a rule is dropped only when it is listed here; any other one fails evaluation
  # rather than being silently left out.
  noAgyCounterpart = [ "Bash(./scripts/check-*.sh *)" ];

  bashPrefix =
    rule:
    let
      m = builtins.match "Bash\\((.*)\\)" rule;
      inner = builtins.head m;
      prefix = lib.removeSuffix " *" inner;
    in
    assert lib.assertMsg (!(lib.hasInfix "*" prefix))
      "home/modules/antigravity.nix: `${rule}` has a `*` inside the prefix, which agy's command(...) cannot express. List it in noAgyCounterpart or declare it differently.";
    prefix;

  # Only `Bash(...)` rules convert. `mcp__…` rules name Claude's MCP servers, and agy has none of
  # them configured (~/.gemini/config/mcp_config.json is empty), so they are left out on purpose.
  toCommands =
    rules:
    lib.unique (
      map (r: "command(${bashPrefix r})") (
        lib.filter (r: lib.hasPrefix "Bash(" r && !(lib.elem r noAgyCounterpart)) rules
      )
    );

  readDeny = map (p: "read_file(${absolute p})") cfg.secretPaths;
  readUrl = map (d: "read_url(${d})") cfg.domains;

  allow = lib.unique (
    toCommands config.dotfiles.claudePermissions.allowRules ++ readUrl ++ cfg.extraAllow
  );
  ask = lib.unique (toCommands config.dotfiles.claudePermissions.askRules ++ cfg.extraAsk);
  deny = lib.unique (readDeny ++ cfg.extraDeny);

  # The CLI ignores a rule it cannot parse (the schema's `permissionRule`), so a malformed rule
  # would look deployed and do nothing. The reconciler rejects the same shapes at apply time.
  ruleShape = "(command|read_file|write_file|read_url|execute_url|mcp|unsandboxed)\\(.*\\)";
  checked =
    rules:
    assert lib.assertMsg (lib.all (r: builtins.match ruleShape r != null) rules)
      "home/modules/antigravity.nix: a rule is not `action(target)`: ${
        lib.concatStringsSep ", " (lib.filter (r: builtins.match ruleShape r == null) rules)
      }";
    rules;

  # Only values that differ from the CLI's defaults: the CLI drops default-valued keys when it
  # rewrites the file, so declaring a default would be undone on the next start.
  spec = {
    allow = checked allow;
    ask = checked ask;
    deny = checked deny;
    enable_terminal_sandbox = true;
  }
  // lib.optionalAttrs (config.dotfiles.agentPermissions.sandboxed_shell == "auto") {
    tool_permission = "proceed-in-sandbox";
  };
in
{
  options.dotfiles.antigravityPermissions = {
    enable = lib.mkEnableOption "declaring permissions in Antigravity CLI's settings.json";
    secretPaths = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = config.dotfiles.agentPermissions.secret_paths;
      description = "Credential paths denied to the agent's file reads (same source as Claude's and Codex's).";
    };
    domains = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = config.dotfiles.agentPermissions.domains ++ [
        # Read version-pinned upstream sources during investigation (same as Claude's).
        "raw.githubusercontent.com"
      ];
      description = ''
        Hosts allowed as `read_url(...)`. In agy these are also the sandbox's outbound allowlist
        (Antigravity docs "Terminal sandbox").
      '';
    };
    extraAllow = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "Additional rules in agy's own syntax, e.g. `command(git fetch)`.";
    };
    extraAsk = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "Additional ask rules in agy's own syntax.";
    };
    extraDeny = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "Additional deny rules in agy's own syntax.";
    };
  };

  config = {
    xdg.configFile."agent-native/antigravity-permissions.json".text = builtins.toJSON {
      inherit (cfg) enable;
      inherit spec;
    };

    home.activation.reconcileAntigravityPermissions = lib.mkIf cfg.enable (
      lib.hm.dag.entryAfter [ config.dotfiles.agentPermissions.retireStep ] ''
        # pipeline-node: agent-settings-reconcile
        run ${settingsReconcile} antigravity-permissions "$HOME/.gemini/antigravity-cli/settings.json" \
          ${lib.escapeShellArg (
            builtins.toJSON (
              spec
              // {
                # Rules written last time. A rule dropped from the declaration is removed from
                # settings.json by this record; rules the owner added through the CLI stay.
                state = "${home}/.local/state/dotfiles/managed-permissions-antigravity.json";
              }
            )
          )}
      ''
    );
  };
}
