# claude-mcp-servers — `~/.claude.json` の `.mcpServers` を宣言的に管理する「口」

`~/.claude.json` の `.mcpServers` は Claude Code が `claude mcp add` で実行時に
書き換える(`~/.claude/settings.json` と同じ制約)。したがって
`registerHooks` / `registerPermissions`(`docs/claude/claude-permissions.md`)と
同じ「activation 時の冪等 jq 書き換え」パターンを敷く。

配備は独立モジュール `home/modules/claude-mcp-servers.nix`(`home/modules/quarantine.nix`
と同じ「1 option = 1 ファイル」の粒度、`home/common.nix` の imports に追加)の
`registerMcpServers` / `options.dotfiles.claude.mcpServers` /
`home.activation.registerClaudeMcpServers`。

## 宣言集合が user scope の完全状態である(reconcile)

`dotfiles.claude.mcpServers` に挙がったサーバーが user scope の望ましい完全状態で、
activation は宣言外の user-scope サーバーを**削除する**。`claude mcp add` で足した
野良サーバーは次の `hms` で消える — 残したいなら宣言に格上げする。

当初この option は加法的 merge(宣言 key だけ上書きし、野良には触れない)だった。
狙いは、先例(退役済みの私設 AI 設定同期リポジトリ)が陥った「リポジトリの定義
ファイルと実機が長期 drift し、最終的に野良で生きているサーバーの方が正になる」
状態の再発防止で、そのために「値を持たない機構だけを作る」という判断をしていた。

この判断は実測で裏切られた。宣言されたのは esa(ADR-0022)1 件のまま、野良が
9 件まで積み上がり(ripgrep・rust-analyzer・github・drawio・motherduck・logic2 ほか)、
セッションごとにツール名一覧と server instructions がコンテキストへ注入され、
起動時には各サーバーへの接続待ちが乗るようになっていた。「野良を許容する」設計は
drift を避けるどころか、drift を定常状態として固定していた。加法的 merge は
home-manager 自身の宣言モデル(宣言 = 望ましい完全状態。`home.file` は非宣言物を
残さない)からも外れている。そこで reconcile に転換した。

副次的な帰結を 3 つ明記しておく:

1. **値が空 `{ }` でも activation は走る。** 空は「この host の user scope には
   MCP サーバーを置かない」という宣言であって no-op ではない。company ホストは
   `home/identities/personal.nix` を import しないので user scope が空になる
   (esa 自体、company のカードでは復号できず毎セッション起動に失敗する登録
   だった — ADR-0022)。
2. **宣言 key は deep merge(`*`)で書く。** 丸ごと置換にすると、リモート
   サーバーの認証時に Claude Code が書き足す `oauth` ブロック(clientId /
   callbackPort)を switch のたびに剥がし、再認証を強いる。宣言した field 自体は
   毎回上書きされるので「宣言が正」は変わらない。
3. **削除は activation ログに出す。** 不可逆操作で、しかも野良サーバーで実験して
   いた場合は何が消えたかがそこにしか残らないため、`register-claude-mcp-servers:
   宣言外の MCP サーバーを削除します: <name>` を stderr に出す。

**reconcile の対象は user scope だけ**である。project scope(リポジトリの
`.mcp.json`、`~/.claude.json` の `.projects[…].mcpServers`)、plugin 由来のサーバー
(context7 等)、claude.ai 標準コネクタ(`mcp__claude_ai_*` 系ツール名、
Gmail/Calendar/Drive/Claude Docs)はいずれも別の管理主体を持つので触らない。
プロジェクト固有のサーバーは project scope に置く方が正しい(実例: telepath の
`logic2`)。

## populate する場所は 1 つに決め打たない

identity 層(`home/identities/personal.nix` / `home/identities/company.nix`)からも
`home/modules/claude.nix` 自身からも、同じ属性に足せる。会社 workspace 専用の
接続情報を common 層に書いてしまう事故(全ホストへの意図しない配備)を、
モジュール分割そのもので防ぐ。現在の populate は `home/modules/esa.nix` の `esa`
(personal 限定、ADR-0022)と `home/identities/personal.nix` の `playwright` /
`slack` — 後者 2 つも外部サービスへ繋ぐ経路なので identity 層に置いている
(warp-terminal #9 と同じ判断)。

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

この option から値を削除すれば、次の switch で `~/.claude.json` 側のエントリも
消える。reconcile なので撤回のための別機構(`claude-permissions.md` の
`retiredPermissionRules` に相当するもの)は要らない。

ただし、そのサーバー向けに配った `permissions.allow` のルール
(`mcp__<server>__<tool>`)は `~/.claude/settings.json` 側の話であり、こちらは
加法的なままなので自動では消えない。サーバーを撤回したら、対応する allow ルールを
`home/modules/claude.nix` の `retiredPermissionRules` へ移すこと。
