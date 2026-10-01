# ~/.claude.json の .mcpServers を宣言的に管理する「口」(docs/claude/claude-mcp-servers.md)。
#
# ~/.claude.json は Claude-Code-owned(`claude mcp add` が実行時に書き換える)なので
# ~/.claude/settings.json と同じ制約が掛かる — store symlink にはできず、activation
# 時の冪等書き換えにする(home/modules/claude.nix の hook / permissions
# 登録と同じ settings-reconcile の別サブコマンド)。ただしこのモジュール自身はサーバーの値を
# 一切持たない: home/modules/quarantine.nix の `dotfiles.quarantine.managedFiles` と
# 同型の extensible option として「口」だけを公開し、personal.nix / company.nix /
# このファイル自身のいずれからも `dotfiles.claude.mcpServers.<name> = {...}` で
# populate する。
#
# 書き換えは reconcile(宣言集合 = user scope の望ましい完全状態)。当初は加法的
# merge で、`claude mcp add` で足した野良サーバーを残す設計だった — が、野良が
# 9 件まで積み上がってセッションごとのコンテキスト(ツール名一覧・server
# instructions)と起動時の接続待ちを純粋に食う状態になり、このモジュールが避け
# ようとしていた「野良の方が正」ドリフトそのものを許していた。宣言外のキーは
# 次の switch で消える — 残したいサーバーは宣言に格上げする。値が空({})なら
# 「user scope には何も置かない」という宣言であり、no-op ではない(company ホスト
# では esa も含めて user scope が空になる。これは意図した動作)。
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.claude;
in
{
  options.dotfiles.claude.mcpServers = lib.mkOption {
    type = lib.types.attrsOf lib.types.anything;
    default = { };
    description = ''
      MCP servers to declaratively manage in `~/.claude.json`'s
      top-level `.mcpServers` (user scope). Each attribute name is the
      server name Claude Code shows; the value is the raw MCP server
      config object (Claude Code's own schema — `command`/`args`/`env`
      for stdio servers, `type = "http"`/`url` for remote ones).

      This attribute set is the desired *complete* state of user-scope
      MCP servers: activation removes any user-scope server not declared
      here, including ones added by hand via `claude mcp add`. A server
      worth keeping belongs in this option. Declared keys are deep-merged
      over whatever that key currently holds, so fields Claude Code
      writes at runtime (an `oauth` block obtained by authenticating a
      remote server, say) survive a switch while the declared fields are
      re-asserted.

      Project scope (`.mcp.json` in a repository, or
      `.projects[…].mcpServers`) and plugin-provided servers are out of
      scope and never touched. An empty value here is itself a
      declaration — "this host has no user-scope MCP servers" — not a
      no-op. See docs/claude/claude-mcp-servers.md.
    '';
    example = {
      example-server = {
        command = "npx";
        args = [
          "-y"
          "@example/mcp-server"
        ];
      };
    };
  };

  config = {
    # 使い方: settings-reconcile claude-mcp-servers <claude.json> <servers-json>
    #   <servers-json> は { "<name>": {<Claude Code の MCP server config>}, ... } の
    #   JSON 文字列で、user scope の望ましい完全状態。宣言外の key は削除し(削除名は
    #   stderr に残す — 不可逆なので黙って消さない)、宣言した key は宣言値を既存値の
    #   上に deep merge する。deep merge にするのは、Claude Code がリモートサーバーの
    #   認証時に書き足す `oauth` 等の下位 key を毎回の switch で剥がして再認証を強いない
    #   ため — 宣言した field 自体は毎回上書きされるので、宣言が正であることは変わらない。
    #   `.projects` 配下(project scope)には一切触れない。変化が無ければ書き込みしない
    #   (mtime を汚さない)。実装は crates/settings-reconcile(#414)。
    home.activation.registerClaudeMcpServers = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
      run ${pkgs.dotfiles-tools}/bin/settings-reconcile claude-mcp-servers "$HOME/.claude.json" ${lib.escapeShellArg (builtins.toJSON cfg.mcpServers)}
    '';
  };
}
