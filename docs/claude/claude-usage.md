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
リファレンス確認済み)。ANSI エスケープシーケンスは herdr 側で除去される
(詳細は後述)ため、出力は常にプレーンテキスト 1 行にしている。

`~/.claude/hooks/claude-usage.sh`(`home/modules/claude.nix` が配備)がこの
command の実体。**Claude Code hook ではない**ので `settings.json` には一切登録
しない — herdr が直接 `/bin/sh -lc` で呼ぶだけの独立スクリプト。

herdr 公式 Configuration ドキュメント(<https://herdr.dev/docs/configuration>、
2026-09-18 取得)は command エントリの出力について "removes ESC-prefixed
terminal control sequences instead of interpreting styles" と明記している —
ANSI エスケープシーケンスは解釈されず除去されるため、**出力自体での動的な
色分けはできない**。静的な色は config.toml 側の `fg`(16 進カラー)でしか
指定できず、値に応じて色を変える(例: 上限到達時だけ赤くする)ことは herdr
の仕様上できない。上限到達は前置記号 `!`(後述)で表現している。

interval 60s / timeout 10s の根拠: `/usage` と同じエンドポイントに 1 rpm は保守
的な負荷。timeout はスクリプト内部の `curl --max-time 5` が先に諦めて空出力する
設計の 2 倍の余裕を持たせている。

## `limits[]` の動的処理

将来 `seven_day` 全体上限が非 null になっても壊れないよう、`limits[]` を
決め打ちの 2 件ではなく動的に処理する。ラベルとサンプル履行(後述)の系列キーは
`kind` から決める:

| `kind` | ラベル |
|---|---|
| `session` | `5h` |
| `weekly_scoped` | `scope.model.display_name`(null なら `wk`) |
| `weekly` | `wk` |
| 未知 | `group`(なければ `kind`) |

`percent` か `resets_at` が無いエントリは丸ごと捨てる(表示不能なだけで、他の
エントリの表示は妨げない)。

## ペース着地予測

「今のペースを次のリセットまで維持したら何 % に着地するか」を、**窓開始
(`resets_at − 窓長`)からの平均ペース**で算出する:

```
経過率 = (now - 窓開始) / 窓長
着地%  = percent / 経過率
```

窓長は `kind` ごとに固定する:

| `kind` | 窓長 | 一次情報 |
|---|---|---|
| `session` | 5h(18000s) | Anthropic Help Center「Usage limit best practices」(Updated June 2, 2026、<https://support.claude.com/en/articles/9797557-usage-limit-best-practices>、2026-09-15 取得)— "your plan's five-hour session limit" |
| `weekly_scoped` / `weekly` | 7d(604800s) | 同記事は "weekly limits" とだけ書き、日数の明記はない。実測で裏取り: 別々に観測した 2 件の `resets_at`(`2026-09-04T14:59:59Z` と `2026-09-18T15:00:00Z`)がどちらも木曜で 14 日差 → 7 日周期と判断 |
| 未知 kind | なし(着地を出さない) | — |

着地%は整数丸め(上限キャップなし)。次のいずれかに当たる場合は着地を出さず、
セグメント本体(`ラベル 使用%→リセット表示`)だけを描画する:

- **上限到達**(`!` 前置の対象。後述)— 到達後の着地は意味を持たない。
- **`resets_at` が過去**(経過率が定義できない、または無意味に大きい)。
- **窓の序盤(経過率 5% 未満)** — session なら最初の 15 分、weekly 系なら
  最初の 8.4 時間。母数が小さすぎるうちはノイズが着地%に増幅されるため伏せる。

表示は `着地% ≥ 100` を `▲`、`< 100` を `▼` で前置する。中立帯は設けない
(`100%` はちょうど「ペースどおり」で `▲` 側に含める)。例:
`5h 21%→18:19 ▲131%` は「今のペースのままだとリセット時点で 131% 相当になる
見込み(使いすぎ方向)」、`Fable 38%→9/19 ▼82%` は「82% 相当で収まる見込み
(余る方向)」を意味する。

**旧実装(直近サンプルの傾きから ETA を出す方式)は撤去した**。理由:

- `(~1.8h)` という表示が「あと 1.8h で 100% に達する」という警告だと利用者に
  伝わらなかった(意味を取り違えていた)。
- 元の要望は「リセットからの経過」を基準にしたペース配分の指標であり、直近の
  バースト的な使用量の傾きとは主語が違う。
- 平均ペース方式は「使いすぎ」と「余る」の両方を `▲`/`▼` の 1 記号で同時に表現
  できる。旧実装は使いすぎ側の ETA しか出さず、余る側の指標が無かった。
- サンプル履歴・state の `series`・履歴クリアのロジックが丸ごと不要になり、
  再起動直後や herdr 起動直後の初回呼び出しでも(経過率 5% 以上なら)即座に
  着地%が出る。旧実装は 2 サンプル(session で最低 5 分間隔)集まるまで無表示
  だった。

## 上限到達時の表示

`percent >= 100`、または `severity` が exceed / block / critical のいずれかを
含む(大文字小文字無視)場合は上限到達とみなし、セグメント全体に `!` を前置して
予測を出さない: `!5h 100%→18:19`。`severity` の正確な取り得る値は非公開 API の
ため不明なので、この判定はヒューリスティックである。

## 表示フォーマットまとめ

| 状態 | 例 |
|---|---|
| 通常(着地予測なし: 序盤・過去 resets_at) | `5h 21%→18:19 · Fable 48%→9/4` |
| 着地予測あり(使いすぎ側) | `5h 62%→18:19 ▲131% · Fable 48%→9/4` |
| 着地予測あり(余る側) | `5h 21%→18:19 · Fable 48%→9/4 ▼82%` |
| 上限到達 | `!5h 100%→18:19 · Fable 48%→9/4` |
| 一部欠落 | 取れたセグメントだけ描画。全滅なら空出力 |

`→` の後のリセット時刻は、**今から 24 時間以内なら `HH:MM`、それより遠ければ
`M/D`**(ローカル TZ)。`session` は常に前者、`weekly` 系は通常後者になるが、
この規則自体は kind に依らず「24h 以内か」だけで決まるので将来の limit 種にも
そのまま耐える。セグメント間の区切りは ` · `。

## state file

`${XDG_RUNTIME_DIR:-/tmp}/claude-usage-tabbar.json`(tmpfs 相当。再起動で消えて
よい — 着地予測は履歴を使わない純関数なので、消えても次回呼び出しで即座に
復帰する)。同ディレクトリに `mktemp` してから `mv` で atomic に書き換える。
トークンも生レスポンスも保存しない。用途は 2 つ(いずれも同じ 2 フィールドを
読む)— 30 秒再取得ガードと stale-if-error:

```json
{
  "last_fetch": 1756710000,
  "last_line": "5h 21%→18:19 ▲131% · Fable 48%→9/4 ▼82%"
}
```

`last_fetch` が 30 秒未満のときは API を呼ばず `last_line` を再出力する。これは
herdr の interval(60s)と独立な保険で、`herdr server reload-config` 直後の
即時実行ストーム等が API を余計に叩かないようにする。通常のポーリング間隔の
制御は herdr の interval 側に一本化しており、スクリプト側に二重のキャッシュ
機構は持たない。

`last_fetch` から 900 秒(`STALE_TTL`)以内の fetch 失敗は同じ `last_line` を
stale-if-error として再出力する(前節)。30 秒ガードと stale-if-error は
排他的な分岐ではなく、前者は「直近すぎる再フェッチを間引く」、後者は
「フェッチした結果が失敗だったときの表示継続」という別レイヤーの役割。

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

## 縮退表: stale-if-error

herdr の command 仕様は失敗/空出力/timeout を「セグメント表示クリア」として
扱う。旧実装はすべての失敗を空出力に落としていたため、一時的な HTTP 失敗・
ネットワーク断・トークンリフレッシュ中の 401・`curl --max-time 5` の
タイムアウトのたびに、次の interval(60s)の成功までタブバーから usage
セグメントだけが消えていた(hostname・時計は残ったまま)。これは herdr 側の
レイアウト譲歩(後述の別経路)とは無関係の、スクリプト側の縮退だった。

対策として HTTP キャッシュの **stale-if-error**(RFC 5861 "HTTP
Cache-Control Extensions for Stale Content"、Mark Nottingham、2010-05、
<https://www.rfc-editor.org/rfc/rfc5861>、2026-09-18 取得 — "When an error
is encountered, a cached stale response MAY be used to satisfy the
request")と同型の意味論を採用した: **fetch が失敗しても、最終成功
(`last_fetch`)から `STALE_TTL`(900 秒 = 15 分)以内なら state file の
`last_line` をそのまま再出力する**。ネゴシエーション主体(サーバー側の
`stale-if-error` ヘッダ)は存在しないため、TTL は固定値。

fetch 自体が成功したが `limits[]` に表示可能なエントリが 1 件もない場合は
stale の対象外(fresh response が常にキャッシュを置換する、という同じ RFC の
意味論どおり)— 空出力にし、state もその場で更新して古い行を復活させない
(authoritative empty)。

| 状況 | 挙動 |
|---|---|
| `jq` / `curl` / `date` が無い | 空出力(state すら読めないため sticky 化の対象外) |
| `~/.claude/.credentials.json` が無い・読めない・トークンが空 | stale-if-error(TTL 内なら `last_line` を再出力、超過で空出力) |
| HTTP 失敗(401 含む)・ネットワーク断 | stale-if-error(`last_fetch` は更新しないので次回すぐ再試行) |
| レスポンスが不正 JSON | stale-if-error(state file は書き換えない) |
| `limits[]` から表示可能なエントリが 1 件も取れない(fetch は成功) | 空出力 + state をこの回の `now` で更新(authoritative empty、古い行は復活しない) |
| Herdr 外の素のターミナルで実行 | 通常どおり動く(表示するだけの副作用なので危険はない) |

### herdr 側レイアウト譲歩による非表示(スクリプト外)

stale-if-error はスクリプト側の縮退(上表)によるチラつきを解消するもので、
以下のレイアウト起因の非表示は対象外(そもそも state file の中身とは無関係
に herdr がステータス領域ごと描画しない)。

Alacritty を WM 上で横幅縮小したときにタブバー右端の usage セグメントが
消えることがあるが、これは上記の縮退表とは別経路であり、スクリプトの責任
範囲外である。herdr 公式ドキュメントに以下の仕様が明記されている:

> "On a narrow tab row, the complete status area yields to the tabs and
> their controls."
> — herdr 公式ドキュメント Configuration ページ
>   (https://herdr.dev/docs/configuration、2026-09-13 取得)

タブ行が窮屈になると `tab_bar_right` の右ステータス領域(usage・
hostname・時計)が **all-or-nothing で丸ごと** 非表示になる。セグメント
単位の切り詰め・優先度制御は herdr 側に存在しない。発生条件はそのときの
タブ本数・タブタイトル幅に依存するため、同じ画面幅でも起きたり起きなかっ
たりする。

切り分け方: usage だけでなく同じ右端の hostname・時計も同時に消えていれば
このレイアウト譲歩(herdr 仕様どおり)。usage だけが消えて hostname・時計
は残っていれば、`claude-usage.sh` 側の縮退(上記表のいずれか)を疑う。

なお `ui.mobile_width_threshold`(既定 64 列)を境にモバイル単一カラム
レイアウトへの切替もあるが、半画面表示程度で 64 列を割ることは通常なく、
本件の主因ではない。

## 自己検査

```sh
sh config/claude/statusline/claude-usage.sh --selftest
```

ネットワーク・実 credentials に依存せず、以下を検証する:

- `limits[]` のラベル決定・通常表示
- ペース着地予測(使いすぎ側 `▲`・余る側 `▼`・境界のちょうど 100%)
- 序盤ガード(経過 5% 未満)・`resets_at` が過去のときに着地を伏せること
- 窓開始ちょうど(経過 0%)で除算エラーが起きても他セグメントの描画が壊れないこと
- 上限到達(`percent>=100` / `severity` ベース)の `!` 表示・着地を出さないこと
- weekly のラベルフォールバック、未知 `kind` の扱い(着地を出さない)、
  `percent`/`resets_at` 欠落エントリのスキップ
- `__render` 単体では不正 JSON・空 `limits[]` は空出力かつ state file 非破壊
  (stale-if-error は `__render` の外、通常経路側の責務)
- state file が予測用の履歴(`series`)を持たないこと
- トークン非漏えい(スタブ curl 越しの実経路)
- stale-if-error: fetch 失敗(curl 失敗・不正 JSON レスポンス)時に
  `last_line` を TTL(900s)内で再出力し、`last_fetch` を更新しないこと
- stale-if-error: TTL 超過後は空出力になること
- authoritative empty: `limits[]` が空の成功レスポンスは stale を復活させず
  空出力・state クリアで応答すること
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
