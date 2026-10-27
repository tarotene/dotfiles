# Codex keeps its writable config; reconcile only the declared keys (ADR-457).
{
  config,
  lib,
  pkgs,
  ...
}:
let
  repoConfig = ../../config;
  settingsReconcile = "${pkgs.dotfiles-tools}/bin/agent-settings-reconcile";

  cfg = config.dotfiles.codexPermissions;
  permissionSpec = {
    approval_policy = "on-request";
    approvals_reviewer = "auto_review";
    default_permissions = "dotfiles";
    features = {
      network_proxy = true;
      credential_masking = true;
    };
    permissions.dotfiles = {
      extends = ":workspace";
      filesystem = {
        ":root" = "read";
      }
      // builtins.listToAttrs (
        map (path: {
          name = path;
          value = "deny";
        }) cfg.secretPaths
      );
      network = {
        enabled = true;
        domains = builtins.listToAttrs (
          map (domain: {
            name = domain;
            value = "allow";
          }) cfg.domains
        );
      };
    };
    apps = {
      _default.default_tools_approval_mode = "prompt";
    }
    // lib.mapAttrs (_: tools: {
      tools = lib.mapAttrs (_: mode: { approval_mode = mode; }) tools;
    }) cfg.apps;
  };
  # OpenAI, Codex rust-v0.164.0 (retrieved 2026-10-26):
  # https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/protocol/src/config_types.rs#L175-L189
  # https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/tui/src/chatwidget/settings.rs#L751-L768
  # Codex 自身が端末タイトル(OSC)に描く項目。herdr はこれを pane の
  # `terminal_title_stripped` として受け取り、サイドバーの Codex 行 4 行目にそのまま
  # 出す(config/herdr/config.toml)。項目 ID は上流の `TerminalTitleItem`
  # (openai/codex、codex-rs/tui/src/bottom_pane/title_setup.rs)で、改名・削除は
  # 上流で破壊的変更として扱われる。取得 2026-10-20、rust-v0.160.0。
  #   activity         — 動作中のスピナー / 承認待ちの表示
  #   context-used     — context window の使用率
  #   five-hour-limit  — 5h rate limit の残り
  # サイドバーに見えるのは約 36 桁(`Context 0% used | medium | 5h ...` で切れた
  # 実機の表示からの推定、2026-10-20。幅そのものは herdr の API から取れていない)。
  # Codex が組む文字列は、context と 5h だけなら `Context 0% used | 5h 100% left`
  # (31 文字)で収まり、reasoning と weekly-limit を足すと 58 文字で 5h・週の値が
  # 切れる。上流の言い回し
  # (`Context … used` / `… left`)は短くできないので、項目の数で収める。
  # project-name は workspace 列と重なるので外している。
  codexConfigSpec = builtins.toJSON {
    # Native feature notices only; runtime protection failures remain visible.
    # OpenAI rust-v0.164.0 core/src/config/mod.rs:1162 (retrieved 2026-10-27).
    suppress_unstable_features_warning = true;
    "tui.terminal_title" = [
      "activity"
      "context-used"
      "five-hour-limit"
    ];
  };

in
{
  imports = [ ./agent-permissions.nix ];
  options.dotfiles.codexPermissions = {
    apps = lib.mkOption {
      type = lib.types.attrsOf (
        lib.types.attrsOf (
          lib.types.enum [
            "approve"
            "prompt"
          ]
        )
      );
      default = { };
      description = "Native app connector IDs and per-tool approval modes, separate from custom MCP servers.";
    };
    secretPaths = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = config.dotfiles.agentPermissions.secret_paths;
      description = "Native Codex filesystem denies (legacy inputs remain compatible).";
    };
    domains = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = config.dotfiles.agentPermissions.domains;
      description = "Native Codex network proxy destinations.";
    };
    mcpAllow = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = config.dotfiles.agentPermissions.mcp_allow;
      description = "Individual native MCP allowances, applied only to existing servers.";
    };
  };

  config = {
    # Kept as a manual retirement/recovery utility, never a runtime gate.
    home.file.".local/bin/agent-permissions".source = "${pkgs.dotfiles-tools}/bin/agent-permissions";
    xdg.configFile."agent-native/codex-permissions.json".text = builtins.toJSON {
      settings = permissionSpec;
      mcp_allow = cfg.mcpAllow;
    };

    # The native installer owns ~/.local/bin/codex; never replace that symlink.
    home.file.".local/libexec/codex-launcher/codex".source =
      "${pkgs.dotfiles-tools}/bin/codex-launcher";
    home.sessionPath = [ "${config.home.homeDirectory}/.local/libexec/codex-launcher" ];
    # A login shell can inherit HM's "session variables already sourced" marker
    # from a pre-switch parent. Reassert the entry point even when noninteractive.
    programs.zsh.profileExtra = lib.mkAfter ''
      typeset -U path PATH
      path=(${lib.escapeShellArg "${config.home.homeDirectory}/.local/libexec/codex-launcher"} $path)
      export PATH
    '';

    # Retire proven shared edits before either native writer runs. This is not a
    # runtime permission check: an unrelated setting never blocks tool execution.
    home.activation.retireAgentPermissions = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
      # pipeline-node: agent-settings-reconcile
      run ${pkgs.dotfiles-tools}/bin/agent-permissions retire --home "$HOME"
    '';

    home.activation.reconcileCodexPermissions = lib.hm.dag.entryAfter [ "retireAgentPermissions" ] ''
      # pipeline-node: agent-settings-reconcile
      run ${settingsReconcile} codex-permissions "$HOME/.codex/config.toml" \
        ${lib.escapeShellArg (
          builtins.toJSON {
            settings = permissionSpec;
            mcp_allow = cfg.mcpAllow;
          }
        )}
    '';

    # A single writer preserves Codex-owned trust, model selection, and formatting.
    # TUI overrides last until the next switch; new sessions load the declaration.
    home.activation.reconcileCodexConfig = lib.hm.dag.entryAfter [ "reconcileCodexPermissions" ] ''
      # pipeline-node: agent-settings-reconcile
      run ${settingsReconcile} codex-config "$HOME/.codex/config.toml" ${lib.escapeShellArg codexConfigSpec}
    '';
    home.file.".codex/AGENTS.md".text =
      builtins.readFile (repoConfig + "/agents/AGENTS.md")
      + "\n"
      + builtins.readFile (repoConfig + "/codex/AGENTS.codex.md");
  };
}
