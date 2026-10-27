# claude-mcp-servers — `~/.claude.json` の `.mcpServers` を宣言的に管理する「口」

`~/.claude.json` の `.mcpServers` は Claude Code が `claude mcp add` で実行時に
書き換える(`~/.claude/settings.json` と同じ制約)。したがって
hook 登録 / permissions(`docs/claude/claude-permissions.md`)と
同じ「activation 時の冪等書き換え」パターンを敷く。実体は 3 つとも
`agent-settings-reconcile`(`crates/agent-settings-reconcile`、#414)のサブコマンド。

配備は独立モジュール `home/modules/claude-mcp-servers.nix`(`home/modules/quarantine.nix`
と同じ「1 option = 1 ファイル」の粒度、`home/common.nix` の imports に追加)の
`options.dotfiles.claude.mcpServers` /
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
   いた場合は何が消えたかがそこにしか残らないため、`agent-settings-reconcile:
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
(personal 限定、ADR-0022)と `home/identities/personal.nix` の `playwright` —
外部サービス(ブラウザ経由の Web UI)に繋ぐ経路なので identity 層に置いている
(warp-terminal #9 と同じ判断)。

かつて同じ理由で `slack`(remote HTTP、OAuth)も personal.nix に宣言していたが、
transcript 全件を確認したところ利用実績が 0 セッションで、未認証のまま
起動のたびに `⚠ 1 MCP server needs authentication` を出すだけの状態が続いていた。
使っていない外部サービス経路を認証だけして残す判断はせず、宣言ごと削除した。
reconcile が次の switch で `~/.claude.json` 側の該当エントリ(認証時に書き足された
`oauth` ブロックごと)も消す。

### playwright の運用上の裁定(#849、#841)

- **普段の Firefox のプロファイルの複製で、素の Firefox を操作する(2026-10-27)。**
  `command` は起動ラッパー `crates/playwright-mcp-launcher`(`~/.local/libexec/` に配備、
  `home/identities/personal.nix`)で、起動のたびに次を行う。
  1. `~/.mozilla/firefox/installs.ini`(darwin は `~/Library/Application Support/Firefox/`)
     の `Default=` を普段のプロファイルとして決める。`Default=` が 0 個・複数なら
     exit 1 する(Firefox を複数の場所に入れていると決められない)。
  2. それを `~/.cache/playwright-mcp-profile/firefox`(0700)へ `rsync -a --delete` で
     一方向に差分同期する。除外は `lock` / `.parentlock` / `user.js` /
     `prefs.js.playwright` / セッション復元 / クラッシュ記録 / キャッシュ
     (`EXCLUDES`)。複製の `lock` が生きた process を指す間は、同期を飛ばす。
  3. `npx @playwright/mcp@<pin> --browser moz-firefox --user-data-dir <複製>` を exec する。
     Linux は PATH の `firefox` を `--executable-path` に渡す(Playwright の既定は
     `/snap/bin/firefox` で、apt の Firefox では合わない)。

  複製を挟む理由は、Playwright が Firefox の起動のたびにプロファイルへ `user.js`
  (セーフブラウジング・トラッキング防止の無効化、拡張の自動更新の停止、パスワード保存の
  無効化など)を書き、`prefs.js` を `prefs.js.playwright` に退避するため
  (`playwright-core` 1.64.0-alpha の `lib/coreBundle.js` の `writePreferences` と
  `defaultProfilePreferences`、取得 2026-10-27)。普段のプロファイルを直接渡すと、
  その設定が普段の Firefox にも効いて残る。複製だけを渡す構成なら、その書き込みが
  普段のプロファイルへ届く経路が無い。起動中の Firefox につなぐ手段も無い
  (README の `--extension` は Edge/Chrome 専用、`--cdp-endpoint` は CDP)。Chrome も 136 から
  既定のプロファイルへの `--remote-debugging-port` を無視し、別の `--user-data-dir` を
  求めている(Will Harris, Google,
  <https://developer.chrome.com/blog/remote-debugging-port>、2025-03-17、取得 2026-10-27)。

  同期は一方向で、エージェント側のログインや変更は次の起動の同期で消える。ログインが
  要るサービスは、普段の Firefox でログインして `/mcp` から再接続する
  (`config/agents/skills/browser-login-handoff/SKILL.md` §5)。
  実測(vega、Firefox 158、普段の Firefox を起動したまま、2026-10-27): プロファイル
  約 1.1GB の 1 回目の同期は 1.2 秒、2 回目は 0.2 秒。複製の素の Firefox は headed でも
  別のインスタンスとして約 2 秒で起動し、`github.com` にログイン済みで入れた。普段の
  プロファイルには `user.js` も `prefs.js.playwright` も作られなかった。
  未確認: darwin(altair)の Firefox の場所と `installs.ini` の節の数。darwin では
  `lock` の symlink が無い(`.parentlock` の fcntl ロックを使う)ので、複製の使用中の
  検出は Linux でだけ働く。
- **版は pin する。** `@playwright/mcp@latest` のままだと、npx のキャッシュが更新される
  たびに別のサーバーが動く。2026-10-21 の npm の `latest`(0.0.83)に pin し、pin は
  ラッパーの `PLAYWRIGHT_MCP`(`crates/playwright-mcp-launcher/src/lib.rs`)が持つ。上げる
  ときはそこを意図して変える。
- **出力先は XDG の cache に逃がす。** 既定ではセッションの cwd 直下に `.playwright-mcp/`
  (ページのスナップショットとコンソールログ)を作り、worktree を汚す。引数 `--output-dir`
  で `~/.cache/playwright-mcp` に向ける。ただし screenshot に `filename` を明示すると cwd
  側に落ちる(0.0.83 の README)ので、`.playwright-mcp/` は `home/modules/git.nix` の
  グローバル ignore にも入れてある。
- **「unsupported command-line flag」の帯は許容する(Chromium で動かしていた間の裁定)。
  Firefox に替えた後は出ない。** ブラウザの起動のたびに出る帯の
  原因は、サーバー自身が Chromium 系に付ける `--disable-blink-features=AutomationControlled`
  である(0.0.83 の `coreBundle.js` が付ける。Chromium の `bad_flags_prompt.cc` の
  警告対象リストに `kDisableBlinkFeatures` が載っている。取得 2026-10-21)。0.0.83 には
  この旗を外す設定が無く、`--disable-infobars` では抑止されない。設定ファイルの
  `launchOptions` で旗の追加を回避するハックは、自動化検知の回避が消える副作用があり、
  先行例も無いので採らない。帯は表示だけで動作に影響しない。上流への Issue は出さない
  (本人の裁定、2026-10-21)。

## 使い方(populate する側)

```nix
# home/identities/company.nix — 秘密の要らない stdio server の例
{
  dotfiles.claude.mcpServers.playwright = {
    type = "stdio";
    command = "npx";
    args = [ "@playwright/mcp@latest" ];
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
  (`home/modules/esa.nix` + `crates/esa-mcp-launcher`、
  [`esa-mcp.md`](../agents/esa-mcp.md)、ADR-0022)。セッション環境に元から乗っている
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
