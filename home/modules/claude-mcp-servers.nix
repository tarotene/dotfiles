# ~/.claude.json の .mcpServers を宣言的に足す「口」(docs/claude/claude-mcp-servers.md)。
#
# ~/.claude.json は Claude-Code-owned(`claude mcp add` が実行時に書き換える)なので
# ~/.claude/settings.json と同じ制約が掛かる — store symlink にはできず、加法的
# merge にする(home/modules/claude.nix の registerHooks / registerPermissions と
# 同じパターン)。ただしこのモジュール自身はサーバーの値を一切持たない:
# home/modules/quarantine.nix の `dotfiles.quarantine.managedFiles` と同型の
# extensible option として「口」だけを公開し、personal.nix / company.nix / この
# ファイル自身のいずれからも `dotfiles.claude.mcpServers.<name> = {...}` で後から
# populate できるようにする。値が空({})の間は home.activation 自体が生成されない
# ため、定常状態は無条件で no-op — 実機の野良サーバー(`claude mcp add` で追加された
# もの)を home-manager 未対応のまま残すことを許容する意図的な設計であり、bug では
# ない。
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
      MCP servers to declaratively (and additively) merge into
      `~/.claude.json`'s top-level `.mcpServers`. Each attribute name is
      the server name Claude Code shows; the value is the raw MCP server
      config object (Claude Code's own schema — `command`/`args`/`env`
      for stdio servers, `type = "http"`/`url`(`/oauth`) for remote
      ones).

      The merge is additive and last-writer-wins per key: keys not
      listed here — including ones added by hand via `claude mcp add`,
      or managed by another tool — are left untouched. Declaring a key
      here always overwrites whatever value that key currently holds.

      The default is intentionally empty: this option only exists to
      give identity-scoped modules (`home/identities/personal.nix`,
      `home/identities/company.nix`) a place to add MCP servers once
      one is actually worth declaring, without inventing a new merge
      mechanism each time. See docs/claude/claude-mcp-servers.md.
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

  config = lib.mkIf (cfg.mcpServers != { }) {
    home.activation.registerClaudeMcpServers = lib.hm.dag.entryAfter [ "writeBoundary" ] (
      let
        # 使い方: register-claude-mcp-servers <claude.json> <servers-json>
        #   <servers-json> は { "<name>": {<Claude Code の MCP server config>}, ... } の
        #   JSON 文字列。既存の .mcpServers に key 単位で merge する — 宣言した key は
        #   毎回上書きするが、宣言していない key には一切触れない。変化が無ければ
        #   書き込みしない(mtime を汚さない)。
        registerMcpServers = pkgs.writeShellScript "register-claude-mcp-servers" ''
          set -eu
          settings="$1"
          new_servers_json="$2"
          jq=${pkgs.jq}/bin/jq

          if [ ! -f "$settings" ]; then
            mkdir -p "$(dirname "$settings")"
            printf '{}\n' > "$settings"
          fi

          if "$jq" -e --argjson new "$new_servers_json" \
              '((.mcpServers // {}) + $new) == (.mcpServers // {})' \
              "$settings" >/dev/null; then
            exit 0
          fi

          tmp="$(mktemp "$settings.hm.XXXXXX")"
          "$jq" --argjson new "$new_servers_json" \
            '.mcpServers = ((.mcpServers // {}) + $new)' \
            "$settings" > "$tmp"
          chmod --reference="$settings" "$tmp" 2>/dev/null || chmod 600 "$tmp"
          mv "$tmp" "$settings"
        '';
      in
      ''
        run ${registerMcpServers} "$HOME/.claude.json" ${lib.escapeShellArg (builtins.toJSON cfg.mcpServers)}
      ''
    );
  };
}
