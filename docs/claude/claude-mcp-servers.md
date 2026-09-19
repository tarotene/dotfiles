# claude-mcp-servers — `~/.claude.json` の `.mcpServers` を宣言的に足す「口」

`~/.claude.json` の `.mcpServers` は Claude Code が `claude mcp add` で実行時に
書き換える(`~/.claude/settings.json` と同じ制約)。したがって
`registerHooks` / `registerPermissions`(`docs/claude/claude-permissions.md`)と
同じ「activation 時の冪等 jq マージ」パターンを敷く。

配備は独立モジュール `home/modules/claude-mcp-servers.nix`(`home/modules/quarantine.nix`
と同じ「1 option = 1 ファイル」の粒度、`home/common.nix` の imports に追加)の
`registerMcpServers` / `options.dotfiles.claude.mcpServers` /
`home.activation.registerClaudeMcpServers`。

## この option が「機構」だけで「値」を持たない理由

先例(退役済みの私設 AI 設定同期リポジトリ)は MCP サーバーの定義ファイルを
リポジトリで管理していたが、実機の `~/.claude.json` とは長期間 drift しており、
最終的には「野良で生きているサーバーの方が正」という状態になっていた。この
リポジトリを清算するにあたり、同じ轍を踏まないために 2 つの設計判断をした:

1. **値ではなく機構を作る。** `dotfiles.claude.mcpServers` は
   `home/modules/quarantine.nix` の `dotfiles.quarantine.managedFiles` と同型の
   extensible option で、既定値は `{ }`。値が空の間は
   `home.activation.registerClaudeMcpServers` 自体が生成されない
   (`lib.mkIf (cfg.mcpServers != { })`)ため、定常状態では `~/.claude.json` に
   一切触れない。
2. **populate する場所は 1 つに決め打たない。** identity 層
   (`home/identities/personal.nix` / `home/identities/company.nix`)からも
   `home/modules/claude.nix` 自身からも、同じ属性に足せる。会社 workspace 専用の
   接続情報を common 層に書いてしまう事故(全ホストへの意図しない配備)を、
   モジュール分割そのもので防ぐ。

MCP サーバーの中には claude.ai 標準コネクタ(`mcp__claude_ai_*` 系ツール名、
Gmail/Calendar/Drive/Claude Docs 等)で将来代替できるものもある。今すぐ全件を
宣言化するのではなく、実際に「このサーバーを恒久的に宣言したい」と判断した
時点で値を足せる状態を用意するに留めた。

## 使い方(populate する側)

```nix
# home/identities/personal.nix
{
  dotfiles.claude.mcpServers.esa = {
    type = "stdio";
    command = "npx";
    args = [ "-y" "@esaio/esa-mcp-server" ];
    env.ESA_ACCESS_TOKEN = "\${ESA_ACCESS_TOKEN}";
  };
}
```

```nix
# home/identities/company.nix
{
  dotfiles.claude.mcpServers.slack = {
    type = "http";
    url = "https://mcp.slack.com/mcp";
  };
}
```

- 値は Claude Code 自身の MCP server config スキーマ(stdio なら
  `command`/`args`/`env`、remote なら `type = "http"`/`url`/`oauth`)にそのまま従う。
- 秘密情報はリテラルで書かず `${ENV_VAR}` 間接参照にする(`claude-permissions.md`
  と同じ規律)。env の供給経路(environment.d / zshenv 等)は populate する側の
  モジュールが別途確保する。
- 宣言した key は merge のたびに上書きされる。宣言していない key(`claude mcp add`
  で手で足したもの、他ツールが書いたもの)には一切触れない。

## 撤回

値をこの option から削除しても、`~/.claude.json` 側の既存エントリは自動では
消えない(`claude-permissions.md` の `retiredPermissionRules` と同種の制約 —
削除の宣言化が要るなら同じ `--retire` パターンを `registerMcpServers` に足す)。
今のところ値を 1 件も populate していないため、この撤回パスは未実装。
