# Antigravity CLI (`agy`) configuration from the same sources Claude and Codex read:
#   - permissions: agy keeps its writable settings.json; only the declared keys are reconciled
#     (docs/agents/agent-permissions.md).
#   - global rules: the shared AGENTS.md plus an agy-specific file, cut into rule files that fit
#     agy's per-file cap (docs/agents/global-agents-md.md).
#
# Evaluated on every host, applied only where `dotfiles.antigravity.enable` is set (the personal
# identity: a cloud-connected AI tool does not go to the company hosts, #9). The permission
# declaration file under agent-native/ is written on every host, because `hms --follow` stops on a
# missing file (crates/hms/src/lib.rs, `policy_changed`).
{
  config,
  lib,
  pkgs,
  repoConfig,
  ...
}:
let
  settingsReconcile = "${pkgs.dotfiles-tools}/bin/agent-settings-reconcile";
  cfg = config.dotfiles.antigravityPermissions;
  home = config.home.homeDirectory;

  # --- global rules -------------------------------------------------------------------------
  # agy cuts a rule file at 24,000 bytes and demotes rules beyond a 20,000-token budget to a list
  # of paths and descriptions (Antigravity docs, `agy-customizations/docs/rules.md`, "Size Limits
  # and Context Budget", read 2026-11-27 from the installed CLI 1.5.17). The cap includes the
  # frontmatter, so the body gets a little less.
  ruleBodyMaxBytes = 23500;

  # Cut a Markdown text into units at `# ` and `## ` headings. A `#` inside a code fence is not a
  # heading (the shared AGENTS.md has shell comments in a fence), so the fence state is tracked.
  headingUnits =
    text:
    let
      step =
        acc: line:
        let
          isFence = lib.hasPrefix "```" line;
          isHeading = !acc.fence && (lib.hasPrefix "# " line || lib.hasPrefix "## " line);
          fence = if isFence then !acc.fence else acc.fence;
          current = lib.last acc.units;
          units =
            if isHeading then
              acc.units
              ++ [
                {
                  title = lib.removePrefix "# " (lib.removePrefix "## " line);
                  text = line;
                }
              ]
            else
              lib.init acc.units ++ [ (current // { text = current.text + "\n" + line; }) ];
        in
        {
          inherit fence units;
        };
    in
    (lib.foldl' step {
      fence = false;
      units = [
        {
          title = "";
          text = "";
        }
      ];
    } (lib.splitString "\n" text)).units;

  # Pack neighbouring units greedily into chunks of at most `ruleBodyMaxBytes`. A unit that is
  # alone over the cap fails the evaluation (asserted below): no oversized rule file is expressible.
  packUnits =
    units:
    lib.foldl'
      (
        chunks: unit:
        let
          current = lib.last chunks;
          merged = current.text + "\n" + unit.text;
          titles = lib.optional (unit.title != "") unit.title;
        in
        if current.text == "" then
          lib.init chunks
          ++ [
            {
              text = unit.text;
              inherit titles;
            }
          ]
        else if builtins.stringLength merged <= ruleBodyMaxBytes then
          lib.init chunks
          ++ [
            {
              text = merged;
              titles = current.titles ++ titles;
            }
          ]
        else
          chunks
          ++ [
            {
              text = unit.text;
              inherit titles;
            }
          ]
      )
      [
        {
          text = "";
          titles = [ ];
        }
      ]
      units;

  ruleFile =
    {
      description,
      body,
    }:
    assert lib.assertMsg (builtins.stringLength body <= ruleBodyMaxBytes)
      "home/modules/antigravity.nix: a rule unit is ${toString (builtins.stringLength body)} bytes, over the ${toString ruleBodyMaxBytes}-byte body cap (agy cuts a rule file at 24,000 bytes). Split the section in config/agents/AGENTS.md or config/antigravity/AGENTS.antigravity.md.";
    ''
      ---
      trigger: always_on
      description: ${builtins.toJSON description}
      ---
      ${body}
    '';

  # Claude の slash command のうち、手で打つ入口として agy でも使えるもの(`config/claude/commands/`)を、agy の
  # skill(`~/.gemini/config/skills/<name>/SKILL.md`)にする。agy は frontmatter の `name`(ディレクトリ名と同じ)と
  # `description` の無い skill を落とすので、ビルド時に足す(`migrate-agent-settings` の変換の規則、取得 2026-11-27)。
  # `copilot-plan-review`・`plan-view` は、手で打つ入口でなく `exit_plan_mode` の hook として載せる
  # (docs/claude/antigravity-hook-parity.md)。本文の Claude の道具名・主語だけを agy のものに読み替える。
  commandSkills = {
    resolve-pr-threads = "Use this skill to query and resolve GitHub PR review threads through the GraphQL API, with commit traceability and per-thread triage.";
    promote-permissions = "Use this skill to propose a change to an agent's native permission declaration (Claude, Codex, Antigravity) without exposing private paths or server values.";
  };
  agyWording =
    builtins.replaceStrings
      [ "AskUserQuestion" "Claude MUST" "Claude SHOULD" ]
      [ "ask_question" "The agent MUST" "The agent SHOULD" ];
  skillFiles = lib.mapAttrs' (
    name: description:
    lib.nameValuePair ".gemini/config/skills/${name}/SKILL.md" {
      text = ''
        ---
        name: ${name}
        description: ${builtins.toJSON description}
        ---
      ''
      + agyWording (builtins.readFile (repoConfig + "/claude/commands/${name}.md"));
    }
  ) commandSkills;

  sharedChunks = packUnits (headingUnits (builtins.readFile (repoConfig + "/agents/AGENTS.md")));

  rulesFiles = {
    ".gemini/config/rules/00-antigravity.md".text = ruleFile {
      description = "Antigravity CLI(agy)固有の施行配線: 道具名の読み替え・計画の書式・PR 運用";
      body = builtins.readFile (repoConfig + "/antigravity/AGENTS.antigravity.md");
    };
  }
  // lib.listToAttrs (
    lib.imap1 (i: chunk: {
      name = ".gemini/config/rules/${lib.fixedWidthNumber 2 (10 + i)}-shared.md";
      value.text = ruleFile {
        description = "共有の作業規範(" + lib.concatStringsSep "・" chunk.titles + ")";
        body = chunk.text;
      };
    }) sharedChunks
  );

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

  # Only `Bash(...)` rules convert. Claude's `mcp__…` allow rules name servers that are no longer declared
  # (brave-search, github) or connector tools (claude_ai_*); none of the servers agy now gets from
  # `dotfiles.agents.mcpServers` (playwright, playwright-chrome, esa) has an allow rule in Claude, so there is
  # nothing to convert, and agy asks per tool for them.
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

  # --- MCP servers ----------------------------------------------------------------------------
  # 宣言は共有の `dotfiles.agents.mcpServers`(Claude と同じ。home/modules/claude-mcp-servers.nix)。agy の
  # `mcp_config.json` が許すキーだけを残す。Claude の `type`・`timeout` などは agy が黙って捨てるので、
  # reconcile も、残っていれば書く前に止める(`migrate-agent-settings` の変換の規則、取得 2026-11-27)。
  mcpAllowedKeys = [
    "command"
    "args"
    "env"
    "cwd"
    "url"
    "headers"
    "disabled"
    "variables"
  ];
  mcpServersForAgy = lib.mapAttrs (
    _: lib.filterAttrs (k: _: builtins.elem k mcpAllowedKeys)
  ) config.dotfiles.antigravity.mcpServers;
  # build の検査(home.checks)は、所有の記録(実ホームのファイル)を書かないよう、`state` を持たない宣言を使う。
  mcpSpecForCheck = builtins.toJSON { servers = mcpServersForAgy; };
  mcpSpec = builtins.toJSON {
    servers = mcpServersForAgy;
    # 前回書いた名前の記録。宣言から外したサーバーだけを消し、`agy mcp add` で本人が足したものは残す。
    state = "${home}/.local/state/dotfiles/managed-mcp-antigravity.json";
  };

in
{
  options.dotfiles.antigravity.enable = lib.mkEnableOption "deploying Antigravity CLI configuration (permissions, global rules, hooks, MCP servers)";

  options.dotfiles.antigravity.mcpServers = lib.mkOption {
    type = lib.types.attrsOf lib.types.anything;
    default = config.dotfiles.agents.mcpServers;
    defaultText = lib.literalExpression "config.dotfiles.agents.mcpServers";
    description = ''
      MCP servers written to Antigravity CLI's `~/.gemini/config/mcp_config.json`. Follows the shared
      `dotfiles.agents.mcpServers` (Claude's declaration); override it to leave a server out of agy
      (its tool output goes to a different vendor's model).
    '';
  };

  options.dotfiles.antigravityPermissions = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = config.dotfiles.antigravity.enable;
      description = "Declare permissions in Antigravity CLI's settings.json (follows `dotfiles.antigravity.enable`).";
    };
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
    # Read-only store files: agy never writes into rules/ (the TUI's `/permissions` and `/hooks`
    # write settings.json and hooks.json, not rules).
    home.file = lib.mkIf config.dotfiles.antigravity.enable (rulesFiles // skillFiles);

    # `agy mcp add` で本人が足したサーバーは残す(所有の記録に載せた名前だけを管理する)。
    home.activation.registerAntigravityMcpServers = lib.mkIf config.dotfiles.antigravity.enable (
      lib.hm.dag.entryAfter [ "writeBoundary" ] ''
        run ${settingsReconcile} antigravity-mcp-servers "$HOME/.gemini/config/mcp_config.json" \
          ${lib.escapeShellArg mcpSpec}
      ''
    );

    # 宣言の形の検査を、build の時にも回す(適用で初めて落ちる形を、generation の build で落とす)。
    home.checks = [
      (pkgs.runCommand "antigravity-mcp-spec" { } ''
        f="$TMPDIR/mcp_config.json"
        : > "$f"
        ${settingsReconcile} antigravity-mcp-servers "$f" ${lib.escapeShellArg mcpSpecForCheck}
        touch $out
      '')
    ];

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
