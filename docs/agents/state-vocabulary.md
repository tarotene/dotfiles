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
scripts/vocab-graph-check --all                    # config/agents/*/vocabulary.json のうち states と events を持つもの
scripts/vocab-graph-check <語彙.json>...           # 指定した語彙
scripts/vocab-graph-check --skills                 # どの語彙にも external-objects.json にも載らない skill を列挙(まだ必須の job にしない)
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
別リポジトリの対象の名前と skill 名だけを持ち、中身は書かない)のどれかに載っているかを見る。全 skill を載せ切るまでは、CI の必須の job にしない。

並行(トークン)は扱わない。Quint の模型検査と実装照合は、タイマー・並行・対象横断の条件がある対象に、兆候で昇格させる
(ADR の D7。最初の候補はカレンダーの握手の実装照合)。Quint が読む `spec/vocab.jq` は、語彙から仕様のモジュールを生成する
(握手は `from` が `["none"]` だけの事象を遷移の表に含めない)。

## 語彙を変えるとき

- 語彙の JSON を直したら `scripts/vocab-graph-check --all` を回す。`worklog` が読む 2 つ(メール項目・作業の枠)は `cargo test -p worklog` も回す。
- 握手の語彙は、利用側(別リポジトリ)が内容の sha256 を固定して読む(`docs/agents/calendar-handshake.md`)。変えたら PR 本文に新しい hash を書く。
