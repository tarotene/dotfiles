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
# home/identities/company.nix — 秘密の要らない remote server の例
{
  dotfiles.claude.mcpServers.slack = {
    type = "http";
    url = "https://mcp.slack.com/mcp";
  };
}
```

- 値は Claude Code 自身の MCP server config スキーマ(stdio なら
  `command`/`args`/`env`、remote なら `type = "http"`/`url`/`oauth`)にそのまま従う。
- `${ENV_VAR}` はセッション起動時のプロセス環境変数として実際に展開される
  (Claude Code 公式ドキュメント確認済み)。ただし秘密がホストローカルの
  ファイル(gpg 暗号化など)にしか無い場合、`${VAR}` を機能させるには
  ログインシェル起動のたびにその変数を復号・export する必要があり、これは
  ADR-0010 が明示的に退役させたパターン(シェル起動時の GPG PIN プロンプト・
  全プロセスへの秘密展開)に戻ってしまう。そのケースでは `${VAR}` に頼らず、
  `command` にそのサーバー専用の launcher(起動時にだけ復号して `exec` する
  スクリプト)を据える — 実例は `dotfiles.claude.mcpServers.esa`
  (`home/modules/esa.nix` + `scripts/esa-mcp-launcher`、
  [`esa-mcp.md`](esa-mcp.md)、ADR-0022)。セッション環境に元から乗っている
  トークン(例: 別プロセスが供給する company workspace のトークン)なら
  `${ENV_VAR}` 間接参照のままでよい(`claude-permissions.md` と同じ規律)。
- 宣言した key は merge のたびに上書きされる。宣言していない key(`claude mcp add`
  で手で足したもの、他ツールが書いたもの)には一切触れない。

## 撤回

値をこの option から削除しても、`~/.claude.json` 側の既存エントリは自動では
消えない(`claude-permissions.md` の `retiredPermissionRules` と同種の制約 —
削除の宣言化が要るなら同じ `--retire` パターンを `registerMcpServers` に足す)。
`esa`(ADR-0022)が最初の populate 例で、この撤回パスはまだ実装していない。
