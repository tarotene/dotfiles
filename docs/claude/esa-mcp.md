# esa-mcp — esa.io MCP サーバのトークン供給

esa.io MCP サーバ(`@esaio/esa-mcp-server`)を全セッションから動く状態に保つ
仕組み。実装は `scripts/esa-mcp-launcher`(配備先
`~/.local/libexec/esa-mcp-launcher`)+ `home/modules/esa.nix` が populate する
`dotfiles.claude.mcpServers.esa`(登録の仕組み自体は
[`claude-mcp-servers.md`](claude-mcp-servers.md) の共通機構)。裁定の経緯と
代替案の比較は `docs/adr/0022-esa-mcp-host-local-gpg-secret.md`。

## 何を直したか

`~/.claude.json` には esa MCP が既にグローバル登録されていたが、供給元
だった別の private リポジトリ(SOPS + direnv でトークンを 1 個だけ持つ
専用の器)の外では常に Unauthorized だった。

**注記**: `env.ESA_ACCESS_TOKEN = "${ESA_ACCESS_TOKEN}"` という記法自体は
Claude Code がセッション起動時の環境変数で正しく展開する(公式ドキュメント
確認済み)。実際の原因は `ESA_ACCESS_TOKEN` という環境変数が、その private
リポジトリの direnv セッションの外では一度も export されていなかったこと。
この変数をどのセッションでも見えるようにするには、ログインシェル起動の
たびに GPG 復号・export する必要があり、それは ADR-0010 が明示的に退役
させたパターン(シェル起動時の GPG PIN プロンプト)そのものになってしまう。
そのため `${VAR}` 展開そのものは使わず、MCP サーバ起動時だけ復号する
専用 launcher を command に据える設計にした(詳細は ADR-0022 の Context)。

## launcher の契約(`scripts/esa-mcp-launcher`)

- 入力: 環境変数 `ESA_TOKEN_FILE`(省略時 `~/.config/esa/token.gpg`)。
- 正常系: `gpg --quiet --batch --decrypt "$ESA_TOKEN_FILE"` の出力を
  `ESA_ACCESS_TOKEN` として `exec npx -y @esaio/esa-mcp-server "$@"` する。
- 縮退しない: 以下はすべて stderr に診断を出して **exit 1**。
  | 状況 | 診断 |
  |---|---|
  | `gpg` が PATH に無い | home-manager 未適用の疑い |
  | トークンファイルが無い | esa.io でのトークン発行 + `docs/setup.md` 手順への誘導 |
  | 復号失敗 | YubiKey([E] サブ鍵)の挿入・gpg-agent の状態確認を促す |
  | 復号結果が空 | トークンファイル自体が壊れている |
  | `npx` が PATH に無い | mise の node runtime を確認 |

  他の Claude Code hook 群(git-worktree-allow・git-stash-guard 等)は
  「対象外なら無出力で exit 0」という ADR-0005 系の silent no-op を守るが、
  この launcher は意図的に逆を行く。silent に Unauthorized のまま動き続ける
  状態こそが、この機構が解決しようとしている事故そのものだから。
- `--selftest`: `gpg`/`npx` をスタブ(固定文字列を返す/受け取ったトークンを
  そのまま出力する)し、5 ケース(トークン欠如・復号失敗・空トークン・
  npx 不在・token handoff)をネットワークなしで検査する。CI(`ci.yml`)が
  毎回実行する。

## PIN プロンプトについて

トークンの gpg 宛先は personal identity の master fingerprint で、GnuPG は
これを YubiKey 上の **[E](暗号化)サブ鍵**に解決する(ADR-0003 Amendment)。
`sign-prewarm`(`docs/claude/sign-prewarm.md`)が温めるのは on-disk の
**[S](署名)** サブ鍵のパスフレーズであり、カード PIN のキャッシュとは別の
機構である。したがって esa MCP をログイン後に最初に起動したセッションでは、
カード PIN の pinentry が 1 回出る(以後は gpg-agent がキャッシュする限り
再入力は不要)。発火点が「セッション開始 = 画面を見ている瞬間」である点は
sign-prewarm と同じ設計思想であり、Claude の Bash 呼び出し中に不意に
pinentry が奪う事故(sign-prewarm が防いでいるもの)とは性質が異なる。

## 落とし穴: `throw-keyids` との相互作用

`home/modules/gpg.nix` は通信のプライバシー対策として `throw-keyids = true`
(gpg 全体設定、privacy: 受信者の鍵 ID を暗号文に載せない)を有効にしている。
これはこのホストの **すべての** `gpg --encrypt` に効く汎用設定であり、
`docs/setup.md` の暗号化手順が `--recipient <fingerprint>` を指定していても
無条件に anonymous recipient になる。

実際に起きた事故: この状態で作成した `token.gpg` を launcher が復号すると、
gpg は宛先を特定できず、鍵束にある card-backed secret 鍵を **総当たり**する
(gpg(1) "Esoteric Options": "it may slow down the decryption process because
all available secret keys must be tried")。鍵束に別カードの secret stub が
残っていると、正しいカードを挿していてもそのカードの「挿入せよ」プロンプトが
先に出る。personal-pop の鍵束には company カードの stub が残っていたため、
セッション開始のたびに **2 枚のカードそれぞれに対して**挿入プロンプトが
立て続けに出る症状になった。

対策は `docs/setup.md` の手順どおり `--no-throw-keyids` を明示すること
(GnuPG Project, "GPG Esoteric Options",
<https://www.gnupg.org/documentation/manuals/gnupg/GPG-Esoteric-Options.html>、
取得 2026-09-20)。token.gpg のような単一ホスト内のローカル保管ファイルは
そもそも「誰に暗号化したか」を第三者から隠す必要がなく、トレードオフなしで
外せる。既存の `token.gpg` を直す場合は再暗号化が要る:

```bash
tok="$(gpg --decrypt ~/.config/esa/token.gpg)"
printf '%s' "$tok" | gpg --encrypt --no-throw-keyids \
  --recipient 1DCDC49510DCC9BF58C89751B7D596E9AA6F36E8 \
  --output ~/.config/esa/token.gpg.new
# ラウンドトリップが一致してから置換すること(先に decrypt が失敗すると
# 空データの暗号文で元ファイルを潰しうる)
[ "$(gpg --quiet --batch --decrypt ~/.config/esa/token.gpg.new)" = "$tok" ] && \
  mv ~/.config/esa/token.gpg.new ~/.config/esa/token.gpg
unset tok
```

鍵束に残った他カードの secret stub 自体は `gpg --delete-secret-keys
<fingerprint>` で削除できる(公開鍵は残る。stub はカードを挿して
`gpg --card-status` すれば再生成される、可逆な操作)。ただし stub の残存は
`throw-keyids` 事故の症状を増幅するだけで根本原因ではないため、
`--no-throw-keyids` の是正が先。

## `~/.claude.json` への登録

`home/modules/esa.nix` は `dotfiles.claude.mcpServers.esa` に値を populate
するだけで、`~/.claude.json` への冪等 jq merge の仕組み自体は持たない —
その仕組みは `home/modules/claude-mcp-servers.nix` が一元的に持つ共通の
「口」で、`docs/claude/claude-mcp-servers.md` が設計を記録している。esa は
この「口」の最初の実populate例になる。

## なぜ personal identity 層限定か

`home/modules/esa.nix` は `home/identities/personal.nix` からのみ import する
(`home/common.nix` には入れない)。トークンの gpg 宛先が personal の master
fingerprint である以上、company ホスト(company-pop-old / company-pop-new)の
YubiKey では原理的に復号できない。common.nix に置くと company ホストには
「毎セッション必ず失敗する MCP 登録」だけが残ることになる。「クラウド AI
ツールは personal identity のみ」という既存方針(warp-terminal, #9)とも
整合する。

## トラブルシュート

- `jq '.mcpServers.esa' ~/.claude.json` で launcher のフルパスと
  `env.LANG` だけが載っていることを確認。旧 `ESA_ACCESS_TOKEN` プレース
  ホルダが残っていたら `hms` を再実行。
- `~/.local/libexec/esa-mcp-launcher` を直接実行して stderr の診断を読む
  (Claude Code の MCP ログよりも直接的)。
- カード PIN を聞かれ続ける・pinentry が出ない等は `gpg-agent` の状態
  (`gpgconf --reload gpg-agent`)を確認。

## 手動プロビジョニング

`docs/setup.md` の「esa MCP token」節を参照。esa.io 側の操作(トークン名
`dotfiles-esa-mcp` を付け、esa-mcp-server の README が示す最小権限セット
`read:post write:post read:category read:tag read:attachment read:team
read:member admin:comment` を選ぶ — 包括的な `read write` は採らない)から、
`gpg -e` での暗号化・配置・ラウンドトリップ確認までを 1 ブロックにまとめて
ある。トークン発行は 1 回だけでよく、暗号化済みファイルはそのまま他の
personal ホストにコピーできる(GPG が挿さっている YubiKey の [E] サブ鍵に
その都度解決するため、ホストごとの再暗号化は不要)。
