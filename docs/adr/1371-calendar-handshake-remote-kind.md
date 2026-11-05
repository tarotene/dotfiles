# ADR-1371 — 握手の語彙に、勤務先の不在を入れない予定の種類 remote-trip を足す

- Status: Accepted
- Date: 2026-11-05
- Issue: No-Issue(利用側の設計の依頼。握手の仕様を入れた ADR-1242 の続き。利用側のリポジトリ名と番号は、この公開リポジトリには書かない)

## Context

握手の語彙(`config/agents/calendar-handshake/vocabulary.json`、ADR-1242)は、予定がかならず「勤務先のカレンダーに不在を入れる予定」である前提で組んである。
`[go] ` を受けた会社側 script は、不在を入れ、同じ組の他の候補を消し、印を `[blocked] ` にする。`hard`(確定した拘束)の状態は、不在を持つ。

別の場所で勤務する予定(帰省中のリモート勤務など)は、勤務先の会議と重なるかを会社側に読ませたいが、仕事は続くので、不在は入らない。
今の語彙では、この予定を載せる道が無い。載せると `[go] ` で不在が入る。不在にならない予定を、不在が入る予定と同じ印で受け渡すには、
語彙に「予定の種類」を足し、種類によって使える遷移を変える必要がある。

利用側(個人側)は、語彙の内容の sha256 を固定して読むので、語彙を変えれば外部契約を変える(`docs/agents/calendar-handshake.md`「版の固定」)。

## Decision

### D1. 種類は、題名の状態の印の直後の tag で表す。tag が無ければ既定の種類 `leave`

状態の印(7 つ)と、印の付け替えで状態を渡す規則は変えない。種類は、印の直後に `[remote] ` を置いて表す(`[hold] [remote] 〇〇`)。
種類は語彙の `kinds`(`id`・`default`・`tag`・`reaches`・`description`)で宣言する。既定の種類 `leave` は tag を持たず、不在を入れる従来の予定である。
不在にならない種類の名前は `remote-trip`(tag `[remote] `)。組の id に使う題名の残りは tag を含むので、tag が違えば別の組になる。

### D2. 事象ごとに、適用する種類を `kinds` で持たせる(種類ごとの遷移集合)

語彙の事象に `kinds` を足す。`remote-trip` に適用するのは `propose`・`judge`・`withdraw` だけで、`confirm`・`reserve`・`secure`・`reopen` は `leave` だけである。
`remote-trip` は `candidate`・`ok`・`resched`・`ng` にしか至らない(`reaches`)。`go` は語彙に無い遷移になるので、`remote-trip` に `[go] ` が書かれても、
会社側は ADR-1242 D5 の S6 のとおり、最後に受理した印へ差し戻す。不在は入らず、`blocked`・`secured` にも至らない。

- 本命: 事象ごとの `kinds`。状態・印・不変条件(`hard` の状態は不在を持つ)は変えず、種類の宣言と事象の `kinds` を足すだけで、不在が入る道を `remote-trip` から切る。
- 対抗馬: `[go] ` を受けても不在を入れない新しい事象を足す案。`go` → `secured` の事象を `remote-trip` に足し、不変条件を種類ごとに緩め(不在を持たない `hard`)、`secured` の意味を改める。
  白紙から 3 軸で比べる。表現不可能性: 本命は、不在を持たない種類が不在の入る状態に至る道を、語彙に表現しない(遷移が無い)。
  対抗馬は `hard` の状態の意味(不在を持つ)を種類で場合分けし、「`secured` なのに不在が無い」状態を許す。還元性: 本命が足すのは種類の宣言と事象の `kinds` だけで、
  対抗馬は事象・不変条件の緩和・状態の意味の改訂の 3 つを足す。先進性: 差が無い。本命が勝つ。
- 外した候補: 種類ごとに別の語彙(JSON を 2 つ持つ)。状態と印が重複して、単一正本が割れる(複写+同期)。分析で外した。
- 既存手段: 拡張 — `config/agents/calendar-handshake/vocabulary.json`、共通の形 `config/agents/vocabulary/vocabulary.schema.json`、検査器 `scripts/vocab-graph-check`、仕様 `config/agents/calendar-handshake/spec/`。
- 戻せない: 外部契約 — 語彙の内容の sha256 を、利用側が固定して読む。版 4 の hash を利用側に渡し、利用側が pin を上げるまで、利用側は読み込みに失敗する。
- 先行例: ADR-1242 D5 の S6(語彙に無い書き換えは、最後に受理した印へ差し戻す)`docs/adr/1242-calendar-handshake-spec.md` (取得 2026-11-05)
- 差分: 一致 — 差し戻しの規則はそのまま使う。語彙の遷移の集合を、種類ごとの集合に細かくしただけで、新しい応答は足さない。
- 軸: 表現不可能 — 不在になる道が語彙に無い。

### D3. 語彙の検査に規則「種類」を足し、種類の不備と到達の上限の超過を CI で落とす

`scripts/vocab-graph-check` に規則「種類」を足す。`kinds` を持つ語彙で、種類の id・tag の一意、既定の種類がちょうど 1 つで tag を持たないこと、
事象が宣言した種類だけを指すこと、種類ごとに(その種類の事象だけで)`none` から到達できる状態があること、`reaches` を持つ種類がそれ以外の状態に至らないことを見る。
`kinds` を持たない語彙の事象が `kinds` を持つのも違反である。負例は `fixtures/bad-kind-undefined.json`(宣言していない種類)・`fixtures/bad-kind-reach.json`(`reaches` を超える到達)と、
実物の握手の語彙を壊した 2 つ(`remote-trip` に `confirm` を持たせる、事象の `kinds` を外す)で、`--selftest` が確かめる。

- 先行例: ADR-1293 D5 の規則(到達・出口・孤立)と、その負例を `fixtures/` に置く方式 `docs/adr/1293-state-transition-vocabulary.md` (取得 2026-11-05)
- 差分: 異なる — 到達を、語彙全体でなく種類ごとの部分グラフ(その種類の事象だけ)に適用し、`reaches` で到達の上限を宣言させる。上限の宣言が語彙を直す人の意図の記録になる。
- 軸: 検出のみ — `remote-trip` に事象を足して `go` に至らせる誤りを、書けてしまうが、CI が落とす。

### D4. 仕様に種類ごとの遷移集合と不変条件 S7 を足し、種類を知らない規則での破れを回帰として残す

仕様(`spec/handshake.qnt`)は、予定に種類を持たせ、遷移の可否(`legalStep`)を種類ごとの集合で判定する。`spec/vocab.jq` は、語彙から検査用モジュールを作るときに、
遷移を 種類 × `from` × `to` に展開する(その種類の事象だけで到達できる `from` に限る。到達できない組は、被覆の検査で死んだ遷移になるので展開しない)。
既定の種類の組の並びは変えず、被覆の添字(`cover_0` から `cover_14`)は動かない。

- 不変条件 `s7`: `remote-trip` の予定には、不在が入らない(`s7_revised`、本人の手書きの印を含む環境)。
- 対照 `s7_unaware`: 種類を知らない会社側(版 3 までの規則。`KIND_AWARE` を false にする)では `s7` が破れる。会社側の配備が済むまでの穴を、期待を「反例が出る」にして残す。
- 被覆 `cover`: `remote-trip` の遷移 6 本(判定 3・取り下げ 3)も到達可能であることを確かめる。

- 先行例: ADR-1242 D2(語彙 JSON を遷移の単一正本とし、検査用モジュールを生成して読む)と D5(規則の穴を `asis` の対照として回帰に残す)`docs/adr/1242-calendar-handshake-spec.md` (取得 2026-11-05)
- 差分: 一致 — 生成と対照の方式は同じ。遷移の表に種類の列を足した。
- 軸: 検出のみ — 仕様の型は種類を持つが、会社側の実装が S7 を守るかは、利用側の実装の照合(ADR-1242 D6)に残る。

### D5. 会社側の配備が済むまで `remote-trip` の予定を置かない(利用側の運用)

種類を知らない会社側(版 3 までの実装)は、tag を題名の残りとして読み、すべて `leave` として判定する。`remote-trip` の予定に `[go] ` が書かれると、
その会社側は不在を入れる。利用側は、会社側の規則 9(`docs/agents/calendar-handshake.md`)の配備が済むまで、`remote-trip` の予定を置かない。
この PR は、会社側の実装を変えない(会社側は別のリポジトリで、配備は人手)。

- 戻せない: 人手 — 会社側の script の変更と配備(別リポジトリ、勤務先の環境)。
- 先行例: ADR-1242 の Consequences(利用側は、このリポジトリのマージ後に、自分の pin を新しい hash へ上げる)`docs/adr/1242-calendar-handshake-spec.md` (取得 2026-11-05)
- 差分: 一致 — 語彙の変更の公開と、利用側の追従を分ける。
- 軸: 検出のみ — 旧版の会社側が `remote-trip` を `leave` として判定することは防げない。`s7_unaware` が、その穴を回帰として残す。

## 却下した案

- `[go] ` を受けて不在を入れない新しい事象を足す案(D2 の対抗馬)。`hard` の状態の意味が種類で場合分けになる。
- 種類ごとに別の語彙を持つ案。状態と印の複写が、単一正本を割る。
- tag でなく、状態の印そのものを種類ごとに増やす案(`[hold-remote] ` など)。印が 7 から増え、利用側の印の読み取りと `marker_prefix` の既定の形に波及する。感触で外した(分析ではない)。

## 検査の結果

2026-11-05、手元で `nix build .#checks.x86_64-linux.calendar-handshake-spec -L --keep-going`(flake が固定する nixpkgs の Quint 0.32.0)を回した。
検査の行 41 本(表 20 行と、`cover` を展開した 21 本)がすべて期待どおりで、終了コードは 0(derivation は設定の生成を含めて 63 個)。

| 行 | 性質 | 期待 | 結果 | 状態数 |
|---|---|---|---|---|
| `s7_revised` | S7(本人の手書きを含む) | 通る | 通った | 172,689 |
| `s7_unaware` | S7(種類を知らない規則) | 反例 | 反例(`remote-trip` の `[go] ` を受理して不在が入る) | 12,340 |
| `cover_15`〜`cover_20` | `remote-trip` の遷移 6 本の到達可能性 | 反例(到達する) | 全部到達 | 223〜1,199 |

既存の行は、ADR-1242 の検査の結果と同じ結果・同じ状態数だった(例: `h1_revised` 40,993、`h3_revised` 13,761、`h5_revised` 10,697、`h6_revised` 15,785。
`cover_0`〜`cover_14` は全部到達)。`leave` の振る舞いは、種類を足しても変わっていない。

## 執行点

- `config/agents/calendar-handshake/vocabulary.json`
- `config/agents/vocabulary/vocabulary.schema.json`
- `scripts/vocab-graph-check`
- `config/agents/vocabulary/fixtures/bad-kind-undefined.json`
- `config/agents/vocabulary/fixtures/bad-kind-reach.json`
- `config/agents/calendar-handshake/spec/vocab.jq`
- `config/agents/calendar-handshake/spec/handshake.qnt`
- `config/agents/calendar-handshake/spec/scenarios.json`
- `home/tests/calendar-handshake-spec.nix`
- `docs/agents/calendar-handshake.md`

## Consequences

- 握手の語彙は版 4 に上がり、内容の sha256 が変わる。利用側は、このリポジトリのマージ後に、自分の pin を新しい hash へ上げる。pin を上げるまで、利用側は読み込みに失敗する(読み替えて続行しない)。
- 状態の印と、`leave` の遷移は変わらない。Quint の入力は、遷移の表に種類の列が付き、`remote-trip` の 6 本が増える(`leave` の 15 本の並びは変わらない)。
- 語彙を変える PR は、`scripts/vocab-graph-check --all` と `checks.x86_64-linux.calendar-handshake-spec` を通す必要がある。`remote-trip` に事象を足して不在の入る状態に至らせれば、前者で落ちる。
- 限界: 種類は予定ごとに固定したモデルで、本人が題名の tag を書き換えて種類を変えることは入れていない(書き換えれば別の種類の予定として読まれる)。
  S7 は、会社側の実装が規則 9 を守ることを前提にし、実装の適合は ADR-1242 D6 の再生に残る(未実施)。
