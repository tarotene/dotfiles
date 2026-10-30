# state-vocabulary — 対象の状態遷移の語彙 JSON の書き方と検査

決定の記録: `docs/adr/1293-state-transition-vocabulary.md`
形の正本: `config/agents/vocabulary/vocabulary.schema.json`
検査: `scripts/vocab-graph-check`(bash + jq。CI の `dry-run` ジョブ)

## 何のためか

対象(メール項目・作業の枠・カレンダーの枠など)ごとに、状態と事象(遷移)の閉語彙を JSON 1 枚に持つ。
skill は、エージェントが行う事象の手順書で、語彙の `carriers`(行う)・`gates`(前提を検査する機械)・`shown_by`(本人に見せる)に
`skill:<名前>` で名指しされる。名指しできない skill は統合の候補(`agent-tooling-authoring` §2)。

## 書き方

語彙は対象のデータがある側に置く(`config/agents/<対象のディレクトリ>/vocabulary.json`)。

```json
{
  "version": 3, "object": "task-slot", "description": "…", "initial": "none",
  "states": [
    { "id": "done", "terminal": true },
    { "id": "dropped", "terminal": true }
  ],
  "events": [
    { "id": "done", "from": ["none"], "to": ["done"], "by": "person", "carriers": ["cli:worklog", "skill:daily-desk"], "gates": ["cli:worklog"] },
    { "id": "drop", "from": ["none"], "to": ["dropped"], "by": "person", "carriers": ["cli:worklog", "skill:daily-desk"], "gates": ["cli:worklog"] }
  ]
}
```

- `to` は常に配列。`["same"]` は自己遷移、複数は「どれか」(握手の判定)。`from` は状態の id か `none`。
- `by` / `owner` は `person`(本人)・`agent`(エージェント)・`machine`(hook・gate・CI・スクリプト)。補足は `by_detail` / `owner_detail`。
- 担い手の名前: `skill:<skill 名>`(`config/agents/skills/` に実在)、`node:<id>`(`pipeline-map.toml` のノード)、`cli:<crate 名>`(`crates/`)、
  `external:<名前>`(別リポジトリの担い手。解決しない)。`by` が `person` でも、記録を書く担い手があれば `carriers` に書く。無ければ省く。
- `owner` が `person` の状態は、`shown_by` に本人へ見せる担い手を書く(必須)。
- `timers`(対象ごとの数値)は時間の決まりを持つ対象だけ。握手の語彙は `token`・`hold`・`group_key` も持つ。

## 検査

```text
scripts/vocab-graph-check --all                    # config/agents/*/vocabulary.json と config/agents/vocabulary/*.json のうち states と events を持つもの
scripts/vocab-graph-check <語彙.json>...           # 指定した語彙
scripts/vocab-graph-check --skills                 # どの語彙にも external-objects.json にも載らない skill を列挙(CI の dry-run ジョブが回す)
scripts/vocab-graph-check --selftest               # fixtures/ の正例・負例と、skill の負例
```

違反は `ファイル: 規則: 内容` の 1 行ずつを標準エラーに出し、終了コード 1(使い方の誤りは 2)。規則は次の順に見る。

| 規則 | 内容 |
|---|---|
| `schema` | `vocabulary.schema.json` に適合(`schema-validate.jq` の部分集合) |
| `一意` | 状態 id・事象 id が一意。状態 id に `none`・`same` を使わない |
| `参照` | `from` は状態か `none`、`to` は状態か `same` |
| `終端` | 終端の状態から出る事象が無い |
| `到達` | `none` からすべての状態に到達できる |
| `出口` | 終端でない状態に、別の状態へ出る事象がある(自己遷移だけでは出口にならない) |
| `見せる担い手` | `owner` が `person` の状態に `shown_by` の担い手がある |
| `担い手の解決` | `shown_by`・`carriers`・`gates` の名前が実在する |
| `孤立` | どの事象の `to` にも現れない状態が無い |
| `対象の一意` | 渡した全語彙で `object` が一意 |

`--skills` は、`config/agents/skills/` の各 skill(別名 `Compatibility entry for <X>` は X に読み替える)が、全語彙の
`shown_by`・`carriers`・`gates`、または `config/agents/vocabulary/external-objects.json`(`[{"object": "<名前>", "skills": ["<skill 名>"]}]`。
別リポジトリの対象の名前と skill 名だけを持ち、中身は書かない)のどれかに載っているかを見る。CI の `dry-run` ジョブが回すので、
載っていない skill を足した PR は落ちる(例外の一覧は持たない)。`external-objects.json` の `object` は、kebab-case で、そのファイル内で一意で、
dotfiles の語彙の `object` と重ならない。

## 新しい skill の足し方

skill を足す PR は、同じ PR で、その skill が行う事象か前提を満たす事象を、語彙に名指しする(`agent-tooling-authoring` §2 の配線チェックリストの 4)。

1. 対象が dotfiles にある(状態をこのリポジトリの語彙が持つ)とき: `config/agents/<対象>/vocabulary.json` の事象の `carriers`(行う)か `gates`
   (前提を検査する機械)に `skill:<skill 名>` を足す。本人に見せる状態なら `shown_by` でもよい。対象の語彙がまだ無いときは、skill 本文から
   読み取れる状態と事象だけで、語彙を新しく起こす(本文に無い状態は作らない。読めない遷移は ADR に「不明」と書く)。
2. 対象が別の person-state リポジトリにあるとき: `config/agents/vocabulary/external-objects.json` に
   `{"object": "<一般語の名前>", "skills": ["<skill 名>"]}` を足す。中身も個人の対象の詳細も書かない(名前と skill 名だけ)。
3. 別名の skill(`description` が `Compatibility entry for <X>` で始まる)は、正の skill X が載っていれば足りる。
4. `scripts/vocab-graph-check --all` と `--skills` を回す。どの語彙にも載せられない skill は、仕えている skill への統合の候補で、
   `--skills` の例外にはしない。

環境の保守の対象(リポジトリ設定・routine・GPG 鍵・モデルの pin・GAS プロジェクト・Drive のファイル・ブラウザのセッション・skill 自体)の語彙は
`config/agents/{repo-settings,routine,gpg-subkey,model-pin,gas-project,drive-file,browser-session,skill-authoring}/vocabulary.json`
(決定の記録: ADR-0000)。

並行(トークン)は扱わない。Quint の模型検査と実装照合は、タイマー・並行・対象横断の条件がある対象に、兆候で昇格させる
(ADR の D7。最初の候補はカレンダーの握手の実装照合)。Quint が読む `spec/vocab.jq` は、語彙から仕様のモジュールを生成する
(握手は `from` が `["none"]` だけの事象を遷移の表に含めない)。

## 抽象と具体化: 「裁定待ちの項目」

「拾う → 本人にまとめて見せる → 本人が裁定 → 適用する/見送る/失効する」という同じ形の流れが、別々の語彙で 4 つの対象にある。
語彙は統一せず、抽象の遷移表を 1 つ定め(`config/agents/vocabulary/awaiting-item.json`、`object: "awaiting-item"`)、各対象は
自分の語彙の状態を抽象の状態へ写す対応表(`config/agents/vocabulary/refinements/<対象>.json`、形は `refinement.schema.json`)を持つ
(決定の記録: ADR-1297)。

抽象の状態は、`picked`(拾った)・`shown`(見せた。owner が person)・`decided`(裁定済み)・`settled`(適用または見送りのあと、
再発を待つ。非終端)・`applied`(適用)・`declined`(見送り)・`lapsed`(失効)・`handed_off`(別の追跡先へ移した)。
事象は `pick`・`show`・`remind`(再通知。`shown` の自己遷移)・`decide`・`apply`・`decline`・`lapse`・`hand_off`・`settle`・`reopen`
(`settled` → `shown`、再発)。

| 具体化(対象) | 語彙 | 写し方 | P1(放置への手段) |
|---|---|---|---|
| メール項目 | `mail-lifecycle/vocabulary.json` | awaiting → shown、decided → decided、closed → applied、dismissed → declined、stale → lapsed、superseded → handed_off | timer: `remind_after_days` と事象 `remind`(失効は `stale`) |
| 作業中の気づき | `wrapup-inbox/vocabulary.json` | open → shown、fixed・dropped → settled、issued → handed_off | notification: セッション開始時の件数通知(`node:session-wrapup-start`)の事象 `remind` |
| リポジトリ設定のずれ | `repo-config-drift/vocabulary.json` | drifted → picked、drafted → shown、approved → decided、applied → applied、routed → handed_off、excluded・exempt → declined | reminder-event: `redetect`(次の `github-audit` の実行が同じ組を再び報告する。定期の実行は無い) |
| 計画の要求項目 | `plan-requirement/vocabulary.json` | listed → picked、planned → shown、approved → decided、implemented → applied、obsolete → lapsed、blocked_upstream・user_excluded → declined | gate: `plan-scope-gate`(処分の無い項目があると承認の依頼まで進めない) |

裁定ページの選択(`decide`・`pass`・`later`)は独立の状態機械ではなく、メール項目の事象への入力である。対応表 `mail-item.json` の
`inputs` に、`decide` → 事象 `decide`、`pass` → 事象 `pass`、`later` → 事象なし(何も書かない。状態は動かず、時計が動き続ける)と書く。

共通の性質は抽象の側で 1 度だけ定め、各対象が守ることを `--refinements` が示す。

- **P1**: 放置された項目は、失効するか再通知される(または、gate が裁定なしに先へ進めさせない)。抽象の `shown` から出る、
  行為者が machine の事象(`remind`・`lapse`)があること。`shown` に写る具体の状態ごとに、`p1` に手段(`timer`・`reminder-event`・
  `notification`・`gate`)を書き、その名前が具体の語彙に実在すること。
- **P2**: 裁定を待つ項目は、本人に見える場所に出す担い手を持つ。`shown` に写る具体の状態に `shown_by` があること。

```text
scripts/vocab-graph-check --refinements           # refinements/*.json を検査(CI の dry-run)
scripts/vocab-graph-check --refinements <対応表>   # 指定した対応表
```

| 規則 | 内容 |
|---|---|
| `schema` | `refinement.schema.json` に適合 |
| `参照` | `concrete`・`abstract` が語彙の `object` に実在 |
| `写像` | 具体の全状態が `states` に載り、写り先が抽象の状態 |
| `終端` | 具体の終端の状態は、抽象の終端の状態に写る |
| `準同型` | 具体の各事象(from → to)を抽象へ写すと、抽象の事象に存在するか、同じ状態への自己遷移になる |
| `P1` | 上記。抽象の側と、写された具体の状態ごと |
| `P2` | 上記 |
| `入力` | `inputs` の事象が具体の語彙に実在し、`shown` に写る状態から出る(事象なしは `note` が要る) |

限界: P1 は手段の**実在**を見る。手段が実際に働くか(`github-audit` を定期に回す人がいるか)は検査しない。

## 語彙を変えるとき

- 語彙の JSON を直したら `scripts/vocab-graph-check --all` を回す。`worklog` が読む 2 つ(メール項目・作業の枠)は `cargo test -p worklog` も回す。
- 握手の語彙は、利用側(別リポジトリ)が内容の sha256 を固定して読む(`docs/agents/calendar-handshake.md`)。変えたら PR 本文に新しい hash を書く。
