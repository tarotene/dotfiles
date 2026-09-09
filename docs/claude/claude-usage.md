# claude-usage — Herdr のタブバーに Claude の Rate Limit / Fable 使用量を常時表示する

`/usage` を毎回打たないと、5h セッション窓や Fable の週間上限にあとどれくらいで
到達するか、到達後いつ復活するかが分からない。この機能は Herdr の**タブバー
右端**にその数値を常時表示する。表示場所がサイドバー(`herdr-claude-metadata.sh`
等、[`herdr-sidebar-metadata.md`](herdr-sidebar-metadata.md))ではなくタブバーな
のは、usage がペイン単位ではなくアカウント全体の値だから — グローバルな情報は
グローバルな場所に置く。

## データ源: `/usage` が使う非公開 API

Claude Code の hook 入力 JSON にも statusline JSON にも rate limit 情報は来ない
(公式ドキュメント確認済み)。唯一の経路は `/usage` コマンドが内部で叩くエンド
ポイントで、実機で以下を確認した:

```
GET https://api.anthropic.com/api/oauth/usage
Authorization: Bearer <~/.claude/.credentials.json の .claudeAiOauth.accessToken>
anthropic-beta: oauth-2025-04-20
```

レスポンスの `limits[]` が必要な情報を全部持っている:

```json
{
  "limits": [
    {"kind": "session",       "group": "session", "percent": 21, "severity": "normal",
     "resets_at": "2026-09-01T09:19:59.944701+00:00", "scope": null},
    {"kind": "weekly_scoped", "group": "weekly",  "percent": 48, "severity": "normal",
     "resets_at": "2026-09-04T14:59:59.944907+00:00",
     "scope": {"model": {"id": null, "display_name": "Fable"}}}
  ]
}
```

**ドキュメント化されていないエンドポイントである**点はリスクとして設計に織り
込む。スキーマが変わっても、壊れたときの症状を常に「タブバーからこのセグメント
が消えるだけ」に収束させる(下記の縮退表)。Claude Code 本体や herdr の他機能に
波及しない。

## 表示先: Herdr の `ui.tab_bar_right` command エントリ

`config/herdr/config.toml` の `tab_bar_right` に `{ type = "command", command = ...,
interval_seconds = 60, timeout_seconds = 10 }` を追加した。Herdr の command エント
リは interval 実行(前回実行と重複しない)・**成功出力の最終行だけを表示**・
**失敗/空出力/timeout で表示クリア**・`/bin/sh -lc` 実行、という仕様(公式 config
リファレンス確認済み)。ANSI 色対応は未確認のため、出力は常にプレーンテキスト
1 行にしている。

`~/.claude/hooks/claude-usage.sh`(`home/modules/claude.nix` が配備)がこの
command の実体。**Claude Code hook ではない**ので `settings.json` には一切登録
しない — herdr が直接 `/bin/sh -lc` で呼ぶだけの独立スクリプト。

interval 60s / timeout 10s の根拠: `/usage` と同じエンドポイントに 1 rpm は保守
的な負荷。timeout はスクリプト内部の `curl --max-time 5` が先に諦めて空出力する
設計の 2 倍の余裕を持たせている。

## `limits[]` の動的処理

将来 `seven_day` 全体上限が非 null になっても壊れないよう、`limits[]` を
決め打ちの 2 件ではなく動的に処理する。ラベルとサンプル履行(後述)の系列キーは
`kind` から決める:

| `kind` | ラベル | 系列キー |
|---|---|---|
| `session` | `5h` | `session` |
| `weekly_scoped` | `scope.model.display_name`(null なら `wk`) | `weekly_scoped:<display_name>` |
| `weekly` | `wk` | `weekly` |
| 未知 | `group`(なければ `kind`) | `kind` そのまま |

`percent` か `resets_at` が無いエントリは丸ごと捨てる(表示不能なだけで、他の
エントリの表示は妨げない)。

## 燃焼率予測

「今の勢いだとあと何時間で 100% に達するか」を、直近サンプルの端点法で線形推定
する:

```
slope = (直近percent - 最古percent) / (直近epoch - 最古epoch)
eta    = (100 - 現在percent) / slope
```

- サンプルが 2 点未満、またはスパンが最小観測時間(session: 5 分、weekly 系:
  30 分)未満なら予測しない(サンプル不足)。
- `slope <= 0` なら予測しない(減っている/変化していない)。
- `今 + eta` が `resets_at` を超える(=リセットが先に来る)なら予測しない —
  「リセット前に切れる見込みのときだけ」出す要件のとおり。
- 表示は `eta >= 1h` で `(~2.4h)`、`< 1h` で `(~35m)`。

## ウィンドウリセットの検出(履歴クリア)

サンプル履歴は state file(後述)に系列キーごとに保持する。次のどちらかが起きた
ら「ウィンドウがリセットされた」とみなし、履行を空にして今回の 1 点から再スタ
ートする:

1. **`resets_at` が前回と変わった**(主信号)。
2. **`percent` が直近サンプルより 1pt を超えて下がった**(フォールバック — API
   が `resets_at` を返す前にリセットが先に見えるケースの保険)。

1pt 以内の低下は自然なブレとして履歴を継続する(スロットリング・四捨五入起因の
上下動をリセットと誤認しないため)。

サンプルは追記時に古いものを刈る: `session` は直近 90 分、`weekly` 系は直近
6 時間より古い点を捨てる。

## 上限到達時の表示

`percent >= 100`、または `severity` が exceed / block / critical のいずれかを
含む(大文字小文字無視)場合は上限到達とみなし、セグメント全体に `!` を前置して
予測を出さない: `!5h 100%→18:19`。`severity` の正確な取り得る値は非公開 API の
ため不明なので、この判定はヒューリスティックである。

## 表示フォーマットまとめ

| 状態 | 例 |
|---|---|
| 通常 | `5h 21%→18:19 · Fable 48%→9/4` |
| 予測あり | `5h 62%→18:19 (~1.8h) · Fable 48%→9/4` |
| 上限到達 | `!5h 100%→18:19 · Fable 48%→9/4` |
| 一部欠落 | 取れたセグメントだけ描画。全滅なら空出力 |

`→` の後のリセット時刻は、**今から 24 時間以内なら `HH:MM`、それより遠ければ
`M/D`**(ローカル TZ)。`session` は常に前者、`weekly` 系は通常後者になるが、
この規則自体は kind に依らず「24h 以内か」だけで決まるので将来の limit 種にも
そのまま耐える。セグメント間の区切りは ` · `。

## state file

`${XDG_RUNTIME_DIR:-/tmp}/claude-usage-tabbar.json`(tmpfs 相当。再起動で消えて
よい — 予測は数サンプルで復帰する)。同ディレクトリに `mktemp` してから `mv` で
atomic に書き換える。トークンも生レスポンスも保存しない:

```json
{
  "last_fetch": 1756710000,
  "last_line": "5h 21%→18:19 · Fable 48%→9/4",
  "series": {
    "session":             {"resets_at": "...", "samples": [[epoch, percent], ...]},
    "weekly_scoped:Fable": {"resets_at": "...", "samples": [...]}
  }
}
```

`last_fetch` が 30 秒未満のときは API を呼ばず `last_line` を再出力する。これは
herdr の interval(60s)と独立な保険で、`herdr server reload-config` 直後の
即時実行ストーム等が API を余計に叩かないようにする。通常のポーリング間隔の
制御は herdr の interval 側に一本化しており、スクリプト側に二重のキャッシュ
機構は持たない。

## トークンの取り扱い

`~/.claude/.credentials.json`(0600)から `jq` で `accessToken` を読む。**curl の
argv には Bearer トークンを載せない**(`/proc/<pid>/cmdline` は他プロセスから
読めるため) — `curl --config -` で標準入力から `Authorization` ヘッダを渡す:

```sh
curl -sS --fail --max-time 5 --config - -o "$usage_file" <<CURLCFG
url = "https://api.anthropic.com/api/oauth/usage"
header = "Authorization: Bearer ${token}"
header = "anthropic-beta: oauth-2025-04-20"
CURLCFG
```

`--selftest` はこの経路をスタブ curl(argv に `Bearer` が現れたら即 exit 9)+
偽 credentials で実際に通し、stdout・state file にトークン文字列が現れないこと
を grep で検証する。

## 縮退表

すべて **空出力 + `exit 0`**(stderr にも出さない。herdr の command 仕様が
失敗/空出力/timeout を表示クリアとして扱うので、これで十分):

| 状況 | 挙動 |
|---|---|
| `jq` / `curl` / `date` が無い | 空出力 |
| `~/.claude/.credentials.json` が無い・読めない・トークンが空 | 空出力 |
| HTTP 失敗(401 含む)・ネットワーク断 | 空出力(`last_fetch` も更新しないので次回すぐ再試行) |
| レスポンスが不正 JSON | 空出力、state file は書き換えない |
| `limits[]` から表示可能なエントリが 1 件も取れない | 空出力(この場合は `last_fetch` を更新し、無駄な再フェッチは避ける) |
| Herdr 外の素のターミナルで実行 | 通常どおり動く(表示するだけの副作用なので危険はない) |

## 自己検査

```sh
sh config/claude/statusline/claude-usage.sh --selftest
```

ネットワーク・実 credentials に依存せず、以下を検証する:

- `limits[]` のラベル決定・通常表示
- 燃焼率予測(表示される場合・リセットが先で表示されない場合・傾きが負で表示
  されない場合)
- 履歴クリア(`resets_at` 変化・`percent` 1pt 超低下)と、1pt 以内の低下では
  クリアされないこと
- 上限到達(`percent>=100` / `severity` ベース)の `!` 表示
- weekly のラベルフォールバック、未知 `kind` の扱い、`percent`/`resets_at` 欠落
  エントリのスキップ
- 不正 JSON・空 `limits[]` での空出力と state file 非破壊
- トークン非漏えい(スタブ curl 越しの実経路)
- 30 秒の再取得ガード(curl が 1 回しか呼ばれないこと)

内部専用の隠しサブコマンド `claude-usage.sh __render <usage_json_file>
<state_file> <now_epoch>` が fetch を挟まずレンダリングだけを行う — selftest は
これでフィクスチャを直接叩く。決定的にするため `TZ=UTC` 固定・`now` は実行時刻
に依存しない固定 epoch を使う。実運用では herdr の command がホストのローカル
TZ を継承するので、`→` の表示はユーザーのローカル時刻になる。

## 運用ノート

- **Herdr 外では無害**: `HERDR_ENV` 等のガードは持たない(表示するだけの副作用
  で、Herdr 外で実行しても API を叩いて 1 行出すだけ)。
- **config.toml は store symlink**: 変更はこのリポジトリの `config/herdr/config.toml`
  を編集して `home-manager switch`、反映は `herdr server reload-config`
  ([`herdr-sidebar-metadata.md`](herdr-sidebar-metadata.md) と同じ運用)。
