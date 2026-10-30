# ADR-0000 — skill を対象の状態遷移の担い手と定義し、語彙 JSON の共通の形とグラフ検査を置く

- Status: Accepted
- Date: 2026-10-30
- Issue: #1266
- Amends: ADR-1259(語彙 JSON の `events[].to` を配列にし、`object`・`carriers`・`gates` を足す。D1〜D8 の決定は変更しない)、ADR-1242(語彙 JSON の `transitions` を `events` に改める。仕様の検査は変えない)、ADR-1291(語彙 JSON を共通の形に移す。D4 の「共通の形は後の段で作る」を果たす)

## Context

skill が 43 本(2026-10-30 時点の `config/agents/skills/` で `SKILL.md` を持つもの。別名 11 本を含む)に増え、「何のためにどの skill を使うか」が
一覧から分からなくなった。skill の主機能は、ある対象の状態を遷移させることだと置ける。メール項目を読んで本人の裁定を待つ状態まで
運び、裁定を受け取って反映する、という具合である。

状態の表と遷移の表を持つ対象は 3 つある。メール項目(ADR-1259)、カレンダーの握手(ADR-1242)、作業の枠(ADR-1291)。
3 つの語彙 JSON は形が違った。メール項目の `events[]` は `to` が 1 つの文字列で、握手の `transitions[]` は事象の id が無く `to` が
配列で、行為者に `workplace-script` などを書いていた。担い手(どの skill・hook がその遷移を行うか)も、遷移の前提を検査する gate も、
どの語彙にも書かれていない。そのため、次の 3 つを機械ができない。

- skill の用途の一覧を、遷移表から生成する(手書きの一覧を持たない)。
- 遷移表の性質(終端から出ない・到達できる・出口がある)を CI で検査する。
- 同じ対象の同じ遷移を 2 つの skill が担う「重複」を拾う。

この ADR は、その土台(用語・語彙の共通の形・グラフ検査・どの語彙にも載らない skill を拾う検査)を決める。
対象の追加(開発の流れ、環境の保守など)と、skill の統合は、この土台の上で後の段が行う。

## Decision

### D1: 用語を次のとおり決める

- **対象**: 状態を持つもの。メール項目・作業の枠・カレンダーの枠など。対象ごとに、状態の表と事象の表を 1 つ持つ。
- **状態**: 対象のいる位置。どこにも入っていないことを表す予約語 `none`(初期状態)と、`same`(状態を変えない自己遷移の `to`)は、状態の id に使えない。
- **事象**: 状態を動かす出来事。`from`(状態の id か `none`)の配列と、`to`(状態の id の配列、または `["same"]`)を持つ。
- **行為者**: 事象を起こす者。`person`(本人)、`agent`(エージェント)、`machine`(hook・gate・CI・スクリプトなどの機械)の 3 種。
- **裁定**: 行為者が `person` の事象。
- **担い手**: 事象を実際に行う、または行う手順を持つ skill・hook・CLI(`carriers`)。
- **前提**: 事象の前に満たすべき条件を検査する gate(`gates`)。
- **skill**: エージェントが行う事象の手順書。裁定が要る状態まで対象を運び、裁定を受け取り、事象を適用する。hook・gate・CI は、機械が行う事象、または事象の前提の検査である。書き方の skill(PR 本文の書き方、要求インベントリの書き方など)は、ある事象の前提を満たす手順書として、その事象にぶら下げる。

### D2: すべての skill は、行う事象か前提を満たす事象を 1 つ以上名指しする。名指しできない skill は統合の候補

状態を動かさない「知識」型の skill も、自分が行う事象(`carriers`)か、自分が前提を満たす事象(`gates`)を、語彙 JSON に 1 つ以上名指しする。
本人に見せる状態の担い手として `shown_by` に名指しされてもよい。別リポジトリの対象に仕える skill は、dotfiles に語彙の中身を出さず、
`config/agents/vocabulary/external-objects.json` に対象の名前と skill 名だけを書く。名指しできない skill は、仕えている skill への統合候補とする。
新しい skill を足すときも同じ条件を課す(`agent-tooling-authoring` の配線チェックリスト)。別名の skill(`Compatibility entry for <X>`)は、正の skill X と同じ扱いにする。

### D3: 語彙 JSON の共通の形は、既存のメール項目の語彙を拡張した JSON と、その JSON Schema にする

`config/agents/vocabulary/vocabulary.schema.json`(JSON Schema 2020-12 の部分集合)が正本。トップは `version`・`object`(対象の id。kebab-case、全語彙で一意)・
`description`・`initial`(固定で `none`)・`states[]`・`events[]` を必須とし、`timers`(対象ごとの数値)と `group_key`(握手の語彙)を任意で持つ。
`states[]` は `id`・`terminal` を必須とし、任意で `owner`(person|agent|machine)・`owner_detail`・`shown_by`(担い手の配列)・`token`・`hold`(握手の印)を持つ。
`events[]` は `id`・`from`・`to`(常に配列)・`by` を必須とし、任意で `by_detail`・`carriers`・`gates` を持つ。担い手と前提の名前は名前空間つきの文字列にする:
`skill:<skill 名>`(`config/agents/skills/<名前>/` に実在)、`node:<id>`(`pipeline-map.toml` のノード id に実在)、`cli:<crate 名>`(`crates/<名前>/` に実在)、
`external:<名前>`(別リポジトリの担い手。解決せず、名前だけ)。`by` が `person` の事象でも、記録を書く担い手があれば `carriers` に書き、担い手が無い事象(本人が直接編集する等)は省く。

`pipeline-map.toml` は「いつ発火するか」の軸なので、語彙とは混ぜない。ノード id を担い手の名前の参照先としてだけ使う。

### D4: 既存 3 つの語彙を新しい形へ移し、位置は動かさない

メール項目(版 2)は `to` を配列にし、`object: "mail-item"`、各事象の `carriers`・`gates`、`awaiting`・`decided` の `owner`・`shown_by` を足す。
作業の枠(版 2)は `object: "task-slot"` を足す。カレンダーの握手(版 3)は `transitions` を `events` に改めて各事象に id を付け、
`owner` と `by` の `workplace-script` を `machine` + `owner_detail`・`by_detail: "workplace-script"`、`workplace-agent` を `agent` + `*_detail` に、`token`・`hold`・`group_key` は残す。
握手には、初期状態から全状態に到達する(D5 の規則)ために、予定を作る事象 `propose`(`none` → `candidate`、`person`)を足す。この事象は Quint の遷移の表に含めない:
仕様の `personCreate` は遷移の記録(`fired`)を更新せず、予定の作成は任意の題名・日で起きるので、被覆の検査の対象にできない。
Quint の仕様が import する `vocab.qnt` は、`vocab.jq` を新しい形に直したうえで、移行の前後で**バイト単位で同じ**になる(`cmp` で確かめた)。仕様の検査と ITF の照合は変わらない。
`worklog` の `parse_states_events` は `to` の配列を読み、1 要素でなければ止める。

握手の語彙は、利用側(別リポジトリ)が内容の sha256 を固定して読む。この移行で hash が変わるので、利用側は pin を上げる。

### D5: グラフ検査器は bash + jq の `scripts/vocab-graph-check` にし、単一対象の有限状態グラフの性質を見る

語彙ごとに次の順で検査する: Schema への適合、状態 id・事象 id の一意(状態 id に予約語を使わない)、`from`・`to` の参照、終端の状態から出る事象が無い、
`none` からすべての状態に到達できる、終端でない状態から別の状態へ出る事象がある(自己遷移だけでは出口にならない)、`owner` が person の状態に `shown_by` の担い手がある、
担い手・前提の名前が実在に解決する、どの事象の `to` にも現れない状態(孤立)が無い。渡した全語彙で `object` が一意であることも見る。
違反は `ファイル: 規則: 内容` の 1 行ずつを標準エラーに出し、終了コード 1。並行(トークン)は扱わない。
Schema の検証は、メール裁定ページの生成器が既に持つ `schema-validate.jq` を再利用し(複写しない)、`timers` のために `additionalProperties` にスキーマを許すよう拡張した。
`--selftest` が、`config/agents/vocabulary/fixtures/` の正例・規則ごとの負例と、既存の語彙を壊した版で、各規則が期待どおり落ちることを確かめる。
CI の `dry-run` ジョブが `--selftest` と、既存の語彙すべて(`--all`。`config/agents/*/vocabulary.json` のうち `states` と `events` を持つもの)への検査を回す。

### D6: 「どの語彙にも載らない skill を落とす」検査は実装して負例を確かめるが、CI の必須の job への配線は後の段に置く

`vocab-graph-check --skills` は、`config/agents/skills/` の各 skill が、全語彙の `shown_by`・`carriers`・`gates` の `skill:<名前>`、または
`external-objects.json` のどれかに載っているかを検査する。この段では 3 つの語彙が名指しする skill は 3 本(`inbox-adjudication`・`daily-desk`・`slot-availability`)だけで、
43 本のうち 40 本(別名 11 本を含む)が載らない。載せ切る前に必須にすると、すべての PR が落ちる。検査の実装と負例(載らない skill、別名の読み替え、`external-objects.json` の typo)は
`--selftest` が確かめ、必須の job への配線は、環境の保守の語彙を足す段(#1269)で、全 skill が載ったことを確かめてから行う。
`external-objects.json` はこの段では空の配列とする。

### D7: Quint の模型検査と実装照合への昇格は、タイマー・並行・対象横断の条件が出た対象に、兆候で行う

ADR-543 の D2(安定の兆候が出たときに決定論へ昇格させる)を、語彙 JSON の検証に当てはめる。全対象にはグラフ検査(D5)を掛ける。
Quint の模型検査と、仕様の軌跡の実装への再生(ADR-1259 D7)は、次の条件のいずれかを持つ対象に昇格させる。
(1) タイマーで事象が起きる(`timers` を持つ、または `machine` の事象が時間で書かれる)。(2) 複数の書き手が並行・遅延して書く(機体をまたぐ同期、重複実行)。
(3) 複数の対象にまたがる性質を持つ(1 つの対象の事象が他の対象の状態に依る)。作業の枠(ADR-1291 D5)は、いずれも持たないので昇格させない。
最初の昇格候補は、カレンダーの握手である: 仕様はあるが、実装との照合が無い(#1262 の 12 番)。ADR-543 自体は書き換えない。

## 先行例との対比

- D1: 用語を、対象・状態・事象・行為者・裁定・担い手・前提と決める。
  先行例: W3C, "State Chart XML (SCXML): State Machine Notation for Control Abstraction", 2015-09-01, https://www.w3.org/TR/scxml/ (取得 2026-10-30)。「Each state contains a set of transitions that define how it reacts to events. Events can be generated by the state machine itself or by external entities.」
  差分: 異なる — SCXML は機械が実行する記法だが、ここの語彙は記録であり実行しない。行為者(本人・エージェント・機械)と担い手(skill・hook・CLI)を足す。
  軸: 表現不可能 — 状態・事象・行為者を閉語彙で書き、表にない遷移を書かせない。
- D2: すべての skill に、語彙で名指しされる条件を課す。
  先行例: ADR-872 の `pipeline-map.toml` と `crates/pipeline-map/tests/coverage.rs`(`auto = true` のノードは登録箇所から 1 つ以上ポインタされている) docs/adr/872-pipeline-map.md (取得 2026-10-30)
  差分: 異なる — ポインタの向きが逆(地図のノードから実体を探すのでなく、実体の skill が地図に載るかを見る)。載らないものは落とさず、統合の候補として列挙する。
  軸: 検出のみ — skill を足す操作そのものは防げず、載っていない skill を検出する。
- D3: 語彙 JSON の共通の形を、既存の語彙を拡張した JSON と JSON Schema にする。
  本命: なし — 白紙から 3 軸で比べた結果で、先に決めていない。
  対抗馬: SCXML(W3C 勧告、2015-09-01)。状態・終端・事象・ガードを標準の記法で表せ、評価軸(Rust・jq・Nix の評価から単一正本として読めるか、状態・終端・事象・担い手を表せるか)で戦わせた。`<final>` は終端、`<transition>` の `cond` は「The guard condition for this transition.」で式の文字列であり、担い手を閉語彙で持てない。XML を読む手段が jq にも Nix の評価にも無く(`builtins.fromXML` は無い。2026-10-30 に `nix eval` で確かめた)、Quint の生成器も JSON を読むため、SCXML を正本にすると読む側ごとの複写が要る。
  外した候補: XState の機械定義 — 感触で外した(JavaScript の実行系が前提に見えた)。分析ではない。
  既存手段: config/agents/vocabulary/vocabulary.schema.json — 拡張: config/agents/mail-lifecycle/vocabulary.json(Schema は既存の語彙 3 つが持つ欄の和集合)
  先行例: ADR-1259 D2(語彙 JSON 1 枚を正本にし、書き込み口と仕様が同じ表を読む)と ADR-1242(握手の語彙) docs/adr/1259-mail-lifecycle.md (取得 2026-10-30)
  差分: 異なる — 2 つの語彙の形を 1 つに揃え、担い手と前提の欄を足した。
  軸: 表現不可能 — 単一正本。読む側が複写を持たなくてよい形を選ぶ。
- D4: 既存 3 つの語彙を新しい形へ移し、位置を動かさない。
  先行例: ADR-1242 の版管理(利用側が `vocabulary.json` の内容の sha256 を pin し、一致しなければ失敗させる) docs/agents/calendar-handshake.md (取得 2026-10-30)
  差分: 一致 — 版の根拠は内容の hash だけで、`version` は人が読む番号。握手の語彙を変えるので、利用側が pin を上げる。
  戻せない: 外部契約 — 握手の語彙は、別リポジトリの利用側が内容の sha256 を固定して読む。変更後の hash を利用側に渡す必要がある。
  軸: 表現不可能 — 移行の前後で Quint の入力 `vocab.qnt` がバイト単位で同じになることを確かめ、仕様と語彙がずれることを許さない。
- D5: 単一対象の有限状態グラフの性質を、CI でグラフ検査器が見る。
  本命: なし — 実装言語は、本人が bash + jq と裁定した(Rust のクレートと Python は採らない)。
  対抗馬: Rust のクレート `crates/vocab-graph-check`(`worklog` と語彙の型を共有でき、単一正本の評価軸で優れうる)。
  外した候補: Python — 本人の裁定で採らない。分析で外していない。
  既存手段: scripts/vocab-graph-check — 拡張: config/agents/skills/inbox-adjudication/reference/verdict-page/schema-validate.jq(JSON Schema の部分集合の検証器を再利用し、グラフ検査だけを足す)
  先行例: W. M. P. van der Aalst, "The application of Petri nets to workflow management", Journal of Circuits, Systems and Computers 8(1), 1998, pp. 21–66。二次情報として Wikipedia, "Petri net", Workflow nets の節 https://en.wikipedia.org/wiki/Petri_net (取得 2026-10-30)。「WF-nets have the soundness property, indicating that a process with a start marking of k tokens in its source place, can reach the termination state marking with k tokens in its sink place」。原典は未読
  差分: 異なる — 並行(トークン)を扱わず、単一対象の有限状態グラフに絞る。終端への到達は検査せず、初期状態からの到達と、非終端の出口を見る(握手は終端を持たない循環の対象で、メール項目の「いずれ終端に入る」は Quint の活性で検査する)。
  軸: 検出のみ — 語彙を手で書く限り不正な語彙は書けてしまうので、CI で検出する。
- D6: どの語彙にも載らない skill の検査を実装して負例を確かめ、必須の job への配線を後の段に置く。
  先行例なし: 探した範囲は、この dotfiles の `docs/adr`・`crates/pipeline-map`・`scripts` で「skill」「coverage」「registered」を探した(取得 2026-10-30)。skill 一覧と、状態遷移の語彙を突き合わせて落とす検査は見つからなかった。外部の文献は探していない。
  軸: 検出のみ。
- D7: Quint と実装照合への昇格の条件を、タイマー・並行・対象横断にする。
  先行例: ADR-543 D2(安定の兆候で決定論へ昇格させる)と ADR-1291 D5(仕様を要した条件を、並行・タイマー・対象横断として整理した) docs/adr/543-existing-means-and-deterministic-promotion.md (取得 2026-10-30)
  差分: 一致 — 昇格の兆候を、語彙 JSON の検査に当てはめて閉じた条件にした。
  軸: 検出のみ — 条件に当たる対象が出たことを、語彙の `timers`・`machine` の事象・複数の書き手から人が読む。

## 却下した案

- 語彙を SCXML で書く。D3 のとおり、jq と Nix の評価が XML を読めず、読む側ごとに複写が要る。
- 語彙ごとに形を保つ。skill の用途の一覧、グラフ検査、重複の検出を、形ごとに書くことになる。
- 検査器を Rust のクレートにする。本人が bash + jq と裁定した(D5)。
- `--skills` 検査をこの段で必須の job にする。D6 のとおり、40 本が載っておらず、すべての PR が落ちる。
- 握手の予定を作る事象を足さず、`none` から到達できない語彙を許す。D5 の到達の規則が、握手の状態すべてを到達できないと数える。

## Consequences

- 3 つの語彙が同じ形になり、`scripts/vocab-graph-check --all` が CI で回る。`--skills` は、載っていない skill を列挙するだけで、まだ落とさない。
- 握手の語彙の hash が変わる。利用側は pin を上げるまで、読み込みに失敗する(読み替えて続行しない)。握手の `owner` は、版 2 までの意味(印を置く者)のまま `person|agent|machine` に写した。
  `owner` を「状態にいる間に次の手を打つ者」と読むと、握手では `candidate` と `go` の担当が逆になる。この ADR は意味を変えず、読み替えは握手の語彙を次に変える段で決める。
- 限界: 名前の実在は検査するが、その skill が実際にその事象を行うことは検査しない。担い手の記述が実体と食い違うことは、読んで確かめる。

## 執行点

- `config/agents/vocabulary/vocabulary.schema.json`
- `config/agents/vocabulary/external-objects.json`
- `config/agents/vocabulary/fixtures/ok-minimal.json`
- `scripts/vocab-graph-check`
- `config/agents/skills/inbox-adjudication/reference/verdict-page/schema-validate.jq`
- `config/agents/mail-lifecycle/vocabulary.json`
- `config/agents/calendar-handshake/vocabulary.json`
- `config/agents/task-slot/vocabulary.json`
- `config/agents/mail-lifecycle/spec/vocab.jq`
- `config/agents/calendar-handshake/spec/vocab.jq`
- `crates/worklog/src/mail.rs`
- `home/tests/calendar-handshake-spec.nix`
- `home/tests/mail-lifecycle-spec.nix`
- `.github/workflows/ci.yml`
