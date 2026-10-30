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
        # Default credential stores of tools this repository installs or documents
        # (#1160): rclone's remotes (packages.nix, docs/gdrive-upload.md), clasp's
        # token (packages.nix), and the GNOME login keyring files that
        # `secret-tool` (bitwarden.nix, obsidian.nix) reads.
        "~/.config/rclone"
        "~/.clasprc.json"
        "~/.config/clasp"
        "~/.local/share/keyrings"
      ];
      description = ''
        Credential paths excluded from ordinary file reads; use reviewed operations.
        Add a path here when it is the upstream default credential store of a tool this
        repository installs or documents, and it exists on a host. Host- or
        organization-specific paths do not belong here (this repository is public):
        put them in the private wrapper flake (ADR-0034).
      '';
    };

    domains = mkOption {
      type = types.listOf types.str;
      default = [
        # GitHub's API (gh, and Codex's GitHub reads).
        "api.github.com"
        # git fetch / push over https. A push still needs a credential, which the
        # sandbox deliberately does not show (gh's token), so it runs outside it.
        "github.com"
        # The default Nix binary cache. Nix's own writes and daemon socket are not
        # declared here, so a build still runs outside the sandbox (#1050).
        "cache.nixos.org"
        # PyPI: uv resolves from pypi.org and downloads the wheels from files.pythonhosted.org.
        # Without them `uv sync` inside the sandbox fails on the download (measured 2026-10-29).
        "pypi.org"
        "files.pythonhosted.org"
        # The Python standard library's Sphinx inventory. The docs-python action's generated
        # conf.py reads it through intersphinx (docs/adr/640-stack-standard-api-docs.md,
        # Amendment 2026-10-18), so a local docs build needs it in every Python repository.
        "docs.python.org"
      ];
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

    # Removed (#1099): the places a policy covers are listed in `projects`. The name stays only
    # so a private wrapper that still sets it keeps evaluating; a non-empty value warns and has no
    # effect. Delete this option once every host's wrapper has dropped it.
    project_rules = mkOption {
      type = types.attrs;
      default = { };
      visible = false;
      description = "Removed; has no effect. List project paths in `projects`.";
    };

    policyFile = mkOption {
      type = types.path;
      readOnly = true;
      default = pkgs.writeText "agent-permissions-policy.json" (builtins.toJSON policy);
      description = "Validated CLI input, available before enabling deployment for inspection and migration.";
    };
  };

  config.warnings =
    lib.optional (cfg.project_rules != { })
      "dotfiles.agentPermissions.project_rules was removed (#1099) and has no effect; list the project paths in `dotfiles.agentPermissions.projects`.";

  # Compatibility inputs only during the three-host retirement. Product modules
  # consume these defaults; no shared policy, ownership check or gate is deployed.
}
