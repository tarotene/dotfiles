# sign-prewarm — [S]/[E] パスフレーズ入力を安全な瞬間に前倒しする hook

git コミットに署名しているのは、GitHub の vigilant mode で自分名義のコミットに
Unverified が付かないようにするため。パスフレーズ入力そのものが地味に面倒、という
問題への対処。#252(ADR-0003 Amendment 4)で esa MCP の token.gpg 復号
([E] サブ鍵)も同じ問題を抱えることが分かり、この hook の対象に加わった。

| 部品 | イベント | 役割 |
|------|----------|------|
| `home/modules/gpg.nix` の `defaultCacheTtl`/`maxCacheTtl` | (agent 設定) | パスフレーズの再入力を「ログインに 1 回」まで落とす |
| `home/modules/gpg.nix` の `home.activation.assertNoLinger` | (activation) | 「agent の寿命 = ログインセッション」という前提を検査する |
| `sign-prewarm.sh` | SessionStart(`startup\|resume`) | 残る「その 1 回」を、利用者が画面を見ている安全な瞬間に前倒しする |

配備は `home/modules/claude.nix`(スクリプトは `home.file`、`~/.claude/settings.json`
への登録は activation 時の冪等 jq マージ)。

## なぜ「squash-only なら未署名にする」を採らなかったか

`tarotene/dotfiles` は GitHub 上で squash マージのみ許可されている
(`allow_squash_merge=true`、merge commit・rebase は両方 off)。main の全コミットは
`committer=GitHub` の `verified=true` ——ローカル署名は main の履歴には一切効いて
いない。ここから「squash-only なら `--no-gpg-sign` で構わない」という発想が出るが、
これは成立しない。

**PR のブランチコミットは squash 後も PR の Commits タブに永続的に残る。** 実測
(PR #32 の `5c183f99`)でも `verified=true` のまま残っており、これを未署名にすると
Unverified が main から PR ページへ**移動するだけ**で、総量は減らない。痛みの本体は
「署名していること」ではなく「パスフレーズ入力の頻度」だった。

## なぜ TTL 側を攻めるのか

原因は `services.gpg-agent` の旧設定 `defaultCacheTtl=3600` / `maxCacheTtl=7200`
——最長 2 時間で必ず再入力になっていた。`defaultCacheTtl` は idle タイムアウト
(使うたびリセット)、`maxCacheTtl` は初回入力からの絶対上限なので、**両方**を
実質無限(400d)にしないと効かない。

境界は「クロック」ではなく **agent プロセスの寿命**である。agent は systemd user
instance の下で動くので、ログアウトで instance ごと死ぬなら、TTL が何日でも
実質「ログインに 1 回」になる。この前提が崩れるのは `loginctl` の `Linger` が
有効なホストで、その場合 instance はログアウト後も生存し、パスフレーズは最大
400 日キャッシュされ続ける。

### なぜ `disable-linger` を強制しないのか(assert のみ)

`org.freedesktop.login1.set-user-linger` はこの OS で `auth_admin_keep`
(`/usr/share/polkit-1/actions/org.freedesktop.login1.policy:137`、上書きルール
なし。実測: `pkcheck --action-id org.freedesktop.login1.set-user-linger --process $$`
→ `Authorization requires authentication`)。activation から `loginctl
disable-linger` を叩くと admin 認証プロンプトが出て、ADR-0003 の Consequences
「`home-manager switch` stays non-interactive and CI-buildable (no PIN prompts)」を
破る。

**読み取り(`loginctl show-user -p Linger`)は認証不要**なので、`gpg.nix` の
`home.activation.assertNoLinger` は検査だけを行う: `Linger=yes` を検知したら
`writeBoundary` の前で `exit 1` し、ファイルを一切書かずに `home-manager switch`
自体を失敗させ、`sudo loginctl disable-linger <user>` を促す。logind が使えない
環境(コンテナ等)では `loginctl` が非 0 で落ちて出力が空になり、`yes` に一致しない
ので黙って通す(ADR-0005 の縮退方針)。

脅威モデルの変化(2 時間ごと → ログインごと)の記録は
`docs/adr/0003-secrets-and-identity.md` の Amendment 2。

## なぜ hook が cwd に依存しないのか

SessionStart 時の cwd で「署名 repo かどうか」を判定すると、非署名 repo や git 管理外
ディレクトリで開始したセッションが、同一セッション内で署名 repo に移動してコミット
する経路を取りこぼす——本来温めるべきだったのに温めない。

代わりに `sign-prewarm.sh` は常に `git -C <mktemp -d> config --get <key>` で
**scope なしのグローバル値だけ**を読む。結果、ルールは「そのホストで署名が有効なら、
ログイン後最初に Claude を開いたとき 1 回聞かれる」という cwd に依存しない予測可能
な形になる。`--global` は使わない —— 移行前の残骸 `~/.gitconfig` が home-manager の
`~/.config/git/config` より先に読まれて隠すケースが実際にこの環境で発生したため
(`git config --global --get user.signingkey` は空を返すが、scope なしなら正しく
解決する)。

## なぜオンディスク鍵だけを対象にするか

card-backed な鍵を温めようとすると PIN + タッチが要求され、これはまさに
ADR-0003 が smartcard `[S]` を避けて得た利益(毎コミットのタッチが要らないこと)
への回帰になる。判定は `gpg --list-secret-keys --with-colons --with-fingerprint`
の **field 15**(`S/N of a token`、GnuPG `DETAILS` ドキュメントに記載)で行う:

| field 15 | 意味 | 温めるか |
|---|---|---|
| `+` | オンディスクで利用可能 | 温める |
| `#` | simple stub(protect mode 1001、鍵材料が無い) | 温めない |
| token の serial number(例 `D27600...`) | card-backed(protect mode 1002) | 温めない |
| (空) | 判定できない | 温めない |

field 15 は `--with-secret` を付けなくても利用可能なオンディスク鍵に `+` を出す
(空ではない)——最初の実装ではここを「空ならオンディスク」と逆に書いており、
plan-review の BLOCKER 指摘で実測して訂正した。

## [E](esa MCP token.gpg)への拡張(#252)

#252 で esa MCP の [E] サブ鍵も per-machine on-disk 化されたことで、[S] と
まったく同じ構造の「ログインに 1 回だけパスフレーズを聞かれる」問題を抱える
ようになった。しかし [S] と [E] は **gpg-agent 内で独立したキャッシュ
エントリ**(鍵ごと = keygrip ごと)であり、同じ文字列のパスフレーズを設定
していても一方を解錠しても他方は温まらない——実機で `gpg-agent` を再起動し、
各鍵を個別に `--pinentry-mode error` でプローブして確認済み(2026-09-20/21)。
したがって 2 つの独立した警告を 1 つに統合することはできず、`run_prewarm`
は `warm_sign_if_configured` と `warm_decrypt_if_present` を**独立に**
呼び出す。一方が対象外・失敗でも他方の試行を妨げない。

[E] 側の対象判定は [S] と非対称である。[S] は `git config` という宣言的な
「対象かどうか」の情報源を持つが、[E] にはそれが無いため、代わりに
`scripts/esa-mcp-launcher` と同じ解決規則(`ESA_TOKEN_FILE` 環境変数 →
`XDG_CONFIG_HOME` → `$HOME/.config`)で求めた token.gpg の**実在**そのものを
ゲートにする(ADR-0005 のファイル存在ゲートと同じ考え方)。company ホストや
#252 ロールアウト前の personal-pop では token.gpg が存在しないため、この
経路は完全に無音でスキップされる。

温度判定・本番の温め呼び出しはどちらも [S] と対になる関数
(`is_warm_decrypt`/`warm_up_decrypt`)で行う。**注意**: `--decrypt <file>`
に対して `-o <output>` を後置すると GnuPG が
`usage: gpg [options] --decrypt [filename]` で即座に失敗し、そもそも
pinentry を経由しないまま exit 2 になる(`--detach-sign` 系の
`-o /dev/null --local-user <key> --detach-sign` という並びとは逆)。
`-o` は必ず `--decrypt <file>` より前に置く。スタブ gpg は引数を意味的に
解釈しないため selftest はこの並び順の誤りを検出できず、実機での動作確認で
見つかった。

libsecret/gnome-keyring への永続化(gpg-agent の外部パスワードキャッシュ)
でこの「ログインに 1 回」自体を無くせないか実機検証したが、この host
(pinentry-gnome3 + GCR 3.41.2)では保存 UI が現れず、Secret Service にも
何も保存されなかった。詳細と裏付けは ADR-0003 Amendment 4。

## なぜ同期ブロックなのか、なぜ hook 自身が `timeout` で刈るのか

`gpg-agent.conf` には home-manager の `services.gpg-agent.grabKeyboardAndMouse`
(既定 `true`)由来の `grab` が入っている——pinentry がキーボードを掴む。温め処理を
バックグラウンドに fork すると、利用者がプロンプトに文字を打ち込んでいる最中に
grab 付きダイアログが割り込んでキー入力を奪う事故が起きる(元の問題より悪い)。
そこで hook は**同期ブロック**で温める。

同期ブロックを選ぶと今度は「利用者が離席してダイアログを放置する」リスクが生じる。
settings.json 側の hook timeout(120)に到達すると Claude Code がプロセスを強制終了
するが、pinentry は grab 付きの孤児として残ってしまう。これを避けるため、
`warm_up()` は `timeout 90` で **hook 自身が Claude Code より先に刈る**。刈られた/
キャンセルされた場合は stderr に 1 行出すだけで exit 0(SessionStart は exit 2 でも
ブロックできない)。

## 温度判定: なぜプロンプトを出さずに判定できるか

`--pinentry-mode error` は定義上パスフレーズを一切要求しない——キャッシュがあれば
成功し、なければ失敗するだけ。このモードで空データに対する試し署名を行うだけで、
判定そのものがプロンプトを出すことはない。

本番の温め呼び出しは `--pinentry-mode ask` を明示し、`--batch` は付けない
(agent に pinentry を呼ばせるため)。

## 縮退

すべて exit 0(SessionStart は exit 2 でもブロックできない)。additionalContext は
一切出さない——この hook は副作用(キャッシュを温める)だけが目的で、承認フローや
コンテキストに干渉しない(`plan-view.sh` と同じ「表示/副作用専用ツールは無音で
exit 0」という設計)。

| 分類 | 条件 | 挙動 |
|---|---|---|
| [S] 対象外 | `gpg`/`git` 不在、`gpg.format` が `openpgp` 以外、`commit.gpgsign` が `true` でない、`user.signingkey` が空、鍵が card-backed/stub/不明、すでに warm | 完全沈黙(stdout・stderr とも空) |
| [E] 対象外 | `gpg` 不在、token.gpg が存在しない、すでに warm | 完全沈黙 |
| 温める | 対象かつ cold([S]/[E] それぞれ独立に判定) | その対象について本番の gpg 呼び出しを 1 回だけ行う |
| 失敗 | 温め呼び出しが刈られた/キャンセルされた | 対象ごとに stderr へ 1 行、stdout は空 |

## `register` の既知の制約

`home/modules/claude.nix` の `registerHooks` は command の一致だけで存在判定する
冪等マージなので、**matcher や timeout を後から変えても既存エントリは更新されない**。
変更する場合は `~/.claude/settings.json` の該当エントリを手で消してから
`home-manager switch` する。

## 検証

- `bash config/claude/hooks/sign-prewarm.sh --selftest` — [S] 側は cwd 非依存の
  gating・field 15 の 3 値判定・温度判定の分岐・本番失敗時の縮退・ローカル
  上書きに引っ張られない回帰テスト、[E] 側は token.gpg の実在ゲート・温度
  判定・本番失敗時の縮退、そして [S]/[E] が独立に温まることの回帰テスト
  (CI の `ci.yml` でも実行)。
- 手動 E2E: `gpgconf --kill gpg-agent` でキャッシュを完全に捨ててから hook
  本体(または新しい Claude セッション)を実行し、SessionStart のタイミングで
  [S]・[E] それぞれのパスフレーズが要求されること、以後の `git commit` や
  esa MCP 起動ではダイアログが出ないことを確認する(2026-09-20/21、
  personal-pop で実施、カードを完全に抜いた状態で確認済み)。
