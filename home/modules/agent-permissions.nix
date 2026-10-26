# Shared permission boundaries. Keep private paths/domains in the private wrapper
# module; this public declaration contains rules and well-known credential paths.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.agentPermissions;
  inherit (lib) mkOption types;
  policy = {
    version = 1;
    inherit (cfg)
      writable_roots
      secret_paths
      domains
      tools
      sandboxed_shell
      projects
      project_rules
      ;
  }
  # Left out while empty, so the deployed file stays what an older reader accepts.
  // lib.optionalAttrs (cfg.mcp_allow != [ ]) { inherit (cfg) mcp_allow; };
in
{
  options.dotfiles.agentPermissions = {
    enable = lib.mkEnableOption "shared Claude and Codex permission boundaries";

    writable_roots = mkOption {
      type = types.listOf (
        types.enum [
          "workspace"
          "temp"
        ]
      );
      default = [
        "workspace"
        "temp"
      ];
      description = "Symbolic roots writable without a one-time boundary review.";
    };

    secret_paths = mkOption {
      type = types.listOf types.str;
      default = [
        "~/.ssh"
        "~/.gnupg"
        "~/.aws"
        "~/.config/gcloud"
        "~/.claude/.credentials.json"
        "~/.codex/auth.json"
      ];
      description = "Credential paths excluded from ordinary file reads; use reviewed operations.";
    };

    domains = mkOption {
      type = types.listOf types.str;
      default = [ "api.github.com" ];
      description = "Known network destinations; this grants connectivity, not permission to send.";
    };

    tools = mkOption {
      type = types.enum [ "prompt" ];
      default = "prompt";
      description = "External tool calls require individual review; permanent shell allows stay empty.";
    };

    sandboxed_shell = mkOption {
      type = types.enum [
        "auto"
        "prompt"
      ];
      default = "auto";
      description = ''
        auto: a shell command that runs inside the native sandbox needs no approval
        (Claude autoAllowBashIfSandboxed; Codex on-request already behaves so).
        Writes stay limited to the workspace and temp, network to the declared
        domains. A re-run outside the sandbox, ask and deny rules are still reviewed.
        prompt: every shell command goes through the regular permission flow.
      '';
    };

    mcp_allow = mkOption {
      type = types.listOf (types.strMatching "mcp__[A-Za-z0-9_-]+__[A-Za-z0-9_-]+");
      default = [ ];
      example = [ "mcp__<server>__<tool>" ];
      description = ''
        MCP tools that are always allowed, one `mcp__<server>__<tool>` per entry; a wildcard
        cannot be written. Claude gets the same string in permissions.allow, Codex gets
        approval_mode = "approve" on that tool of a server its config already defines, and the
        permission hook stops asking about it. This is the one exception to "an individual
        allow lasts for the session" (docs/agents/agent-permissions.md). Adding a tool widens
        the boundary, so it needs inspect and a reviewed migrate; removing one narrows it and
        is applied automatically. Bash, command prefixes and WebSearch have no declaration.
      '';
    };

    projects = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "Project paths to inventory in addition to the active project (private wrapper values).";
    };

    project_rules = mkOption {
      type = types.attrsOf (
        types.submodule {
          options.domains = mkOption {
            type = types.listOf types.str;
            default = [ ];
            description = "Additional known destinations for this project only.";
          };
        }
      );
      default = { };
      description = "Project-scoped declarations keyed by project path, without modifying shared repositories.";
    };

    policyFile = mkOption {
      type = types.path;
      readOnly = true;
      default = pkgs.writeText "agent-permissions-policy.json" (builtins.toJSON policy);
      description = "Validated CLI input, available before enabling deployment for inspection and migration.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = lib.unique cfg.mcp_allow == cfg.mcp_allow;
        message = "dotfiles.agentPermissions.mcp_allow lists a tool twice";
      }
    ];
    xdg.configFile."agent-permissions/policy.json".source = cfg.policyFile;
    home.file.".local/bin/agent-permissions".source = "${pkgs.dotfiles-tools}/bin/agent-permissions";
    # Native Claude sandbox dependencies. Bubblewrap is Linux-only; Darwin uses
    # Seatbelt. These are user-space executables, not system-layer services.
    home.packages = [ pkgs.socat ] ++ lib.optionals pkgs.stdenv.isLinux [ pkgs.bubblewrap ];
  };
}
