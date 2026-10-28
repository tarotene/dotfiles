# Claude-specific instructions and display tools; shared tooling is in agents.nix.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  repoConfig = ../../config;
  hooksDir = "${config.home.homeDirectory}/.claude/hooks";
  settingsReconcile = "${pkgs.dotfiles-tools}/bin/agent-settings-reconcile";
  # pipeline-node: claude-statusline
  statusLineCmd = "'${hooksDir}/claude-statusline'";
  legacyStatusLineCmd = "bash '${hooksDir}/claude-statusline.sh'";
  retiredStatusLineCommands = [ legacyStatusLineCmd ];
  planModelBin = "${pkgs.dotfiles-tools}/bin/claude-plan-model";
  hasMidWildcard = rule: builtins.match ".*(\\(| )\\*[[:space:]]+[^)]+.*" rule != null;

  permissionRules_ = [
    "Bash(git add *)"
    "Bash(git commit *)"
    "Bash(git push *)"
    "Bash(gh pr create *)"

    "Bash(git status *)"
    "Bash(git diff *)"
    "Bash(git log *)"
    "Bash(git show *)"
    "Bash(git grep *)"
    "Bash(git rev-parse *)"
    "Bash(git branch *)"
    "Bash(git fetch *)"
    "Bash(git ls-remote *)"
    "Bash(git worktree list *)"
    "Bash(git switch *)"
    "Bash(git checkout -b *)"
    "Bash(git shelve *)"
    "Bash(git shelve)"
    "Bash(git unshelve)"

    "Bash(uv run pytest *)"
    "Bash(uv run ruff *)"
    "Bash(uv run mypy *)"
    "Bash(uv run pre-commit run *)"
    "Bash(uv run docs-check *)"
    "Bash(./scripts/check-*.sh *)"

    "Bash(gh pr view *)"
    "Bash(gh pr list *)"
    "Bash(gh pr diff *)"
    "Bash(gh pr checks *)"
    "Bash(pr-checks-wait *)"
    "Bash(gh issue view *)"
    "Bash(gh issue list *)"

    "Bash(nix fmt)"
    "Bash(nix flake check)"
    "Bash(nix build *)"
    "Bash(home-manager generations)"

    "Bash(npm ci)"
    "Bash(npm run *)"
    "Bash(npm test *)"
  ];

  # #461: 上の `hasMidWildcard` で中間ワイルドカードを含むルールが無いことを
  # nix eval 時(`nix flake check`/`nix build`)に確認してから実際に使う値を返す。
  # `permissionRules_`(素の配列)という別バインディングを経由するのは、
  # `permissionRules = assert (中略 permissionRules …); …` のように自分自身を
  # 参照する assert を書くと Nix の遅延評価が自己再帰(infinite recursion)に
  # なるため。
  permissionRules =
    assert lib.assertMsg (lib.all (r: !hasMidWildcard r) permissionRules_)
      "home/modules/claude.nix permissionRules に中間 `*` を含むルールがあります: ${lib.concatStringsSep ", " (lib.filter hasMidWildcard permissionRules_)} — 値受け取り位置への任意オプション挿入を素通しするため書けません(#461)。個別ルールに絞るか、検証付きの allow hook を追加してください。";
    permissionRules_ ++ config.dotfiles.claudePermissions.extraAllow;

  # pkexec-guard(29番)向けの明示 ask ルール。`.permissions.allow` と違い
  # 「常に許可」ではなく「auto mode でも必ず確認を出す」ことが目的
  # (公式ドキュメント "auto mode config" — 明示 ask ルールは分類器より先に
  # 評価される、docs/agents/pkexec-guard.md 参照)。guard 自身が通すのは
  # `/usr/bin/pkexec <許可リスト> ...` の形だけなので、ここも同じ絶対パスで
  # 揃える(裸の `pkexec` はこのルールに一致せず、guard が deny する)。
  askRules = [ "Bash(/usr/bin/pkexec *)" ];
  retiredAskRules = [ ];

  # かつて配ったが撤回したルール。activation が全ホストの settings.json から削除する。
  # `Bash(git -C * add *)` 等の中間ワイルドカードを含む撤回は、もう個別に
  # ここへ列挙する必要が無い(#461) — agent-settings-reconcile claude-permissions が
  # settings.json 上の中間ワイルドカードルールを出自(宣言/promote/実行時
  # プロンプトのどれか)を問わず一律 strip するようになった。このリストに
  # 残す/追加するのは、中間ワイルドカード**以外**の理由で退役したルールだけ。
  #
  # `list-branch-inventory.sh` / `sweep-removed-vendor-symbols.sh` は使い捨ての
  # ワンオフ作業用ルールが陳腐化して残っていたもの(#101)。~/.ghr 配下の全ローカル
  # リポジトリおよび tarotene 名義の全 GitHub リポジトリを検索したが、該当スクリプトは
  # このファイル自身の permissionRules 記述以外に実体が存在しないことを確認済み。
  #
  # `mcp__brave-search__*` / `mcp__github__*` は、対応する MCP サーバーが user scope
  # から消えたことで宛先を失ったルール。brave-search は API キーが失効したまま
  # サーバー自体が撤去され(#299)、github MCP は運用が gh CLI 側に寄った
  # (グローバル CLAUDE.md の gh 前提)結果として
  # home/modules/claude-mcp-servers.nix の reconcile 対象外になった。存在しない
  # ツール名への allow は効果を持たないが、実態と乖離した宣言が残ると次に読む人が
  # 「このサーバーは生きている」と誤読する。
  # `mcp__plugin_context7_context7__*` は plugin 由来で現役なので触らない。
  retiredPermissionRules = [
    "Bash(./scripts/list-branch-inventory.sh *)"
    "Bash(./scripts/sweep-removed-vendor-symbols.sh *)"

    "mcp__brave-search__brave_web_search"
    "mcp__github__issue_write"
    "mcp__github__issue_read"
    "mcp__github__pull_request_read"
    "mcp__github__update_pull_request"
    "mcp__github__list_issues"
    "mcp__github__create_pull_request"
    "mcp__github__search_issues"
  ];
  cfg = config.dotfiles.claudePermissions;

  # Claude の Read ルールは、絶対パス(`/` 始まり)を `//` で書く。`~/…` はそのまま。
  nativeReadPath = p: if lib.hasPrefix "/" p then "/" + p else p;
  secretReadRules = lib.concatMap (p: [
    "Read(${nativeReadPath p})"
    "Read(${nativeReadPath p}/**)"
  ]) cfg.secretPaths;

  # Codex の `dotfiles` 権限 profile(home/modules/codex.nix)と同じ層構造:
  # sandbox は常時有効、sandbox 内のコマンドは確認なし、全シェルは分類器が見る
  # (`classifyAllShell`。Codex の auto_review に当たる)、秘密パスは読ませない。
  # 宣言したキーだけを合わせる。本人が足した `allowWrite` などには触れない。
  # 層別の対応表: docs/agents/agent-permissions.md。
  permissionsSpec = {
    retire_allow = retiredPermissionRules;
    allow = permissionRules;
    retire_ask = retiredAskRules;
    ask = askRules;
    deny = secretReadRules;
    auto_mode_classify_all_shell = true;
    # 分類器への例外（`autoMode.allow`、自然言語）。`Bash(git switch *)` などの allow ルールは、
    # `classifyAllShell` が有効なあいだ停止されるので効かない。自分の worktree でのブランチ作成が
    # 「共有資源の変更」(`[Modify Shared Resources]`)として拒否された（#939、2026-10-22 に 1 回）。
    # main の checkout・ブランチの削除・push は含めない。`"$defaults"` は照合側が先頭に足す。
    auto_mode_allow = [
      "Creating or switching to a branch inside the agent's own linked git worktree (git switch -c, git switch, git checkout -b) is routine local work and not a change to shared resources. This does not cover creating branches in the main checkout of a repository, deleting branches, or pushing."
    ];
    sandbox = {
      enabled = true;
      fail_if_unavailable = true;
      auto_allow_bash_if_sandboxed = true;
      allowed_domains = cfg.sandboxDomains;
      deny_read = cfg.secretPaths;
      allow_write = cfg.allowWrite;
      excluded_commands = cfg.excludedCommands;
      # #1169 が足したが、途中に `*` を持つパターンは効かないと分かった(#1164、#1173)。
      # 配列は「宣言との和」なので、宣言から外しただけでは settings.json に残る。
      retire_excluded_commands = [
        "git -C * commit *"
        "git -C * push *"
        "git -C * add *"
      ];
    };
  };
  # 適用のときだけ所有の記録（state）を足す。宣言から外した allow（`mcp_allow` から外した
  # 許可を含む）を、前回の記録をもとに settings.json から消すため（#1161）。
  # `agent-native/claude-permissions.json` には足さない（そちらは `PermissionsSpec` の
  # 他の読み手に渡る宣言そのもの）。
  # Claude Code の sandbox が起動に要るコマンド（socat、Linux は bubblewrap）。対話の Claude は
  # home.packages 経由で PATH に載るが、systemd の user unit のような headless の起動の PATH には
  # `~/.nix-profile/bin` が無く、`sandbox.failIfUnavailable` が効いて `socat not installed` で起動を
  # 拒否する（2026-10-28、mailctl-triage.service が 10 分ごとに失敗した）。
  sandboxDeps = [ pkgs.socat ] ++ lib.optionals pkgs.stdenv.isLinux [ pkgs.bubblewrap ];

  claudePermissionsApplySpec = permissionsSpec // {
    state = "${config.home.homeDirectory}/.local/state/dotfiles/managed-allow-claude.json";
  };
in
{
  options.dotfiles.claudePermissions = {
    extraAllow = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = config.dotfiles.agentPermissions.mcp_allow;
      description = "Additional native Claude permission allowances.";
    };
    sandboxBinPath = lib.mkOption {
      type = lib.types.str;
      readOnly = true;
      default = lib.makeBinPath sandboxDeps;
      description = ''
        PATH entries (colon-separated nix store paths) holding the commands Claude Code's sandbox
        needs to start. A headless unit that runs `claude` (a systemd user service, a timer) puts
        this in its `Environment=PATH=…`; otherwise `sandbox.failIfUnavailable` refuses to start
        with `socat not installed`. Same list as `home.packages` below, so there is one source.
      '';
    };
    secretPaths = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = config.dotfiles.agentPermissions.secret_paths;
      description = "Credential paths denied to the Read tool and to sandboxed commands (same source as Codex's secretPaths).";
    };
    sandboxDomains = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = config.dotfiles.agentPermissions.domains ++ [
        # Read version-pinned upstream sources during investigation, including Plan mode.
        "raw.githubusercontent.com"
      ];
      description = ''
        Network destinations sandboxed commands may reach (same source as Codex's domains).
        WebFetch(domain:...) allow rules also feed the sandbox's allowlist.
      '';
    };
    allowWrite = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = ''
        Paths sandboxed commands may write (sandbox.filesystem.allowWrite). Other modules add
        their own: each definition is merged, so a host module can add its state without
        replacing the shared ones. Keep every entry as narrow as a directory the tool owns.
      '';
    };
    excludedCommands = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        # They need the login keyring (gh's token), gpg-agent or the nix daemon socket,
        # which the sandbox does not show, so they run outside it and go through the
        # regular permission flow: the narrow allow rules above, then the auto-mode
        # classifier. Keep patterns narrow: an excluded command runs with full access.
        #
        # Matching (Anthropic, "Sandboxing", https://code.claude.com/docs/en/sandboxing,
        # retrieved 2026-10-27, and measured on vega 2026-10-28, #1164): the whole call
        # must match, so `git add . && git commit` needs `git add` excluded too, and a `cd`,
        # a redirect to a file or a `$(...)` keeps the call sandboxed. Only patterns with a
        # trailing `*` took effect. `git -C * commit *` (a `*` in the middle) did not, even with a
        # literal path, so run git in the worktree's cwd without `-C`; do not add `git -C *`
        # (it would take every `-C` call out of the sandbox).
        "gh *"
        "git commit *"
        "git push *"
        "git add *"
        # They rewrite or create commits, and `commit.gpgsign=true` re-signs them, which needs
        # gpg-agent: `git rebase` failed with `No secret key` inside the sandbox (measured on
        # vega 2026-10-28). `merge`, `cherry-pick`, `revert` and `tag` sign the same way
        # (not measured yet; docs/agents/agent-permissions.md).
        "git rebase *"
        "git merge *"
        "git cherry-pick *"
        "git revert *"
        "git tag *"
        # `git fetch` and `git pull` read the credential through gh's keyring helper, which the
        # sandbox does not show: `git fetch origin` failed inside it with `fatal: could not read
        # Username for 'https://github.com'` and passed outside (measured on vega 2026-10-28).
        "git fetch *"
        "git pull *"
        # nix needs socket(AF_UNIX) for its daemon, which the sandbox denies outright
        # (#1050; `cannot create Unix domain socket`), and ~/.cache/nix for its fetcher.
        "nix build *"
        "nix eval *"
        "nix flake *"
        "nix fmt *"
      ];
      description = "Command patterns that run outside the sandbox (sandbox.excludedCommands).";
    };
  };
  config = {
    # The tools hooks tell the agent to run through Bash (`issue-inbox`, `session-retrospective`,
    # `worklog`, ...) keep their state here. Inside the sandbox this was read-only, so they
    # failed with `Read-only file system` (measured on vega 2026-10-28, #1164).
    dotfiles.claudePermissions.allowWrite = [ "~/.local/state/claude" ];
    # Native Claude sandbox dependencies remain available for restored settings.
    home.packages = sandboxDeps;
    xdg.configFile."agent-native/claude-permissions.json".text = builtins.toJSON permissionsSpec;
    home.file.".claude/CLAUDE.md".source = repoConfig + "/claude/CLAUDE.md";
    home.file.".claude/hooks/claude-statusline".source = "${pkgs.dotfiles-tools}/bin/claude-statusline";
    home.file.".claude/hooks/claude-usage".source = "${pkgs.dotfiles-tools}/bin/claude-usage";
    home.activation.registerClaudeStatusLine = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
      run ${settingsReconcile} claude-statusline "$HOME/.claude/settings.json" \
        ${lib.escapeShellArg (
          builtins.toJSON {
            desired = statusLineCmd;
            retired = retiredStatusLineCommands;
          }
        )}
    '';
    home.activation.registerClaudeModelConfig = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
      run env PATH="$HOME/.local/bin":"$PATH" ${planModelBin} sync
    '';
    home.file.".local/bin/claude-plan-model".source = planModelBin;
    home.activation.registerClaudePermissions = lib.hm.dag.entryAfter [ "retireAgentPermissions" ] ''
      # pipeline-node: agent-settings-reconcile
      run ${settingsReconcile} claude-permissions "$HOME/.claude/settings.json" \
        ${lib.escapeShellArg (builtins.toJSON claudePermissionsApplySpec)}
    '';
  };
}
