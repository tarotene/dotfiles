# ADR-0000 — 開発の流れの対象(計画・PR・Issue・WIP の PR)の語彙を起こし、skill を担い手・前提として載せる

- Status: Accepted
- Date: 2026-10-31
- Issue: #1268
- Amends: なし(ADR-1293 の土台と ADR-1297 の抽象の上に、語彙を足す。D1〜D7 の決定は変更しない)

## Context

ADR-1293 は、skill を「対象の状態遷移を行う、または前提を満たす手順書」と定め、語彙 JSON の共通の形とグラフ検査を置いた。
載せ切るまでは `--skills`(どの語彙にも載らない skill の列挙)を必須の job にしない(D6)。ADR-1297 は、計画の要求項目の語彙 `plan-requirement` を、
「裁定待ちの項目」の具体化として足した。

開発の流れ(計画を書く、PR を作る、Issue を束ねる、作業を中断して引き継ぐ)で動く skill 12 本は、どの語彙にも載っていなかった
(`requirements-inventory` だけが `plan-requirement` に載っていた)。これらの skill は、すでに機械の検査(`plan-scope-gate`・`pr-gate`・
`stack-base-guard` など)と結びついているものと、散文だけのもの(`tracking-issue`・`issue-hygiene`・`living-description`・`issue-ref-freshness`)がある。
2026-10-31 に `config/agents/skills/*/SKILL.md` の本文を読み、状態と遷移を本文から読み取れる範囲に限って、4 対象の語彙を起こした。

## Decision

### D1: 4 対象の語彙を、共通の形(ADR-1293 D3)で `config/agents/<対象>/vocabulary.json` に起こす

| 対象 | `object` | 状態 | 終端 |
|---|---|---|---|
| 計画 | `plan` | `drafting` → `checked` → `awaiting_approval`(owner は person、`shown_by` は `node:plan-view`)→ `approved` → `executed` | `executed` |
| PR | `pull-request` | `created` → `stack_linked` → `merged` | `merged` |
| Issue(tracking Issue の親子を含む) | `issue` | `open` → `parent` → `completed`・`not_planned` | `completed`・`not_planned` |
| WIP の PR | `wip-pull-request` | `handed_off`(`shown_by` は `node:issue-index`)→ `resumed` → `ready` | `ready` |

事象の主なものは次のとおり。

- `plan`: `draft`・`ground`・`compose`・`self_check`・`submit`(ExitPlanMode)・`deny`(gate が差し戻す)・`approve`(person)・`execute`。
- `pull-request`: `create`・`check_completion`(`pr-gate` の Stop 判定)・`wait_checks`・`update_body`・`stack_link`・`unstack`・`merge`(person)。
- `issue`: `file`・`promote`・`attach`・`update_body`・`refresh_refs`・`complete`・`abandon`・`close_by_merge`(machine。`close-linked-issues`)。
- `wip-pull-request`: `handoff`・`relax`(`pr-gate` が Draft と `Handoff:` で `G_link`・`G_CI` を緩和)・`resume`・`ready`。

`gates` の名前は `pipeline-map.toml` のノード id に解決する(`plan-scope-gate`・`plan-precedent-gate`・`plan-fresh-gate`・`copilot-plan-review`・`new-tool-guard`・
`stack-base-guard`・`pr-title-guard`・`pr-followup-guard`・`ci-wait-guard`・`pr-checks-wait`・`pr-gate-stop`・`bleep`)。`pr-gate` の `G_link`・`G_visual`・`G_stack`・`G_prior`・`G_CI`・`G_pr` は
1 つのノード `pr-gate-stop` の中の判定なので、事象の `by_detail` に書く。機械の検査が無い `issue` の事象(`close_by_merge` 以外)は `gates` を省いた。

### D2: 計画の語彙 `plan` は計画 1 枚の流れを持ち、要求項目の処分は `plan-requirement` に任せる

`plan-requirement`(ADR-1297)は要求インベントリの 1 行(R1..Rn)の処分を持ち、`awaiting-item` の具体化である。`plan` は計画 1 枚の
ライフサイクルを持ち、具体化にしない(1 つの計画が、状態の違う複数の項目を持つ)。要求項目の処分は `plan` に複写しない。

### D3: PR の語彙は `pull-request`(作成からマージ)と `wip-pull-request`(Draft と `Handoff:`)に分ける

`wip-pull-request` は、ユーザーの指示で作業を打ち切るときの Draft PR である。`pr-gate` は Draft かつ `Handoff: #N`(N が OPEN)のときだけ `G_link`・`G_CI` を緩める。
`ready`(Draft の解除)のあとは、通常の PR(`pull-request` の `created`)になる。2 つの語彙は `ready` の事象でつなぐだけで、状態は複写しない。
残タスクの子 Issue の起票(`handoff` §4〜§6)は `issue` の対象で、`wip-pull-request` には書かない。

### D4: 書き方の skill は、その事象の担い手(`carriers`)に並べる

ADR-1293 D1 の「書き方の skill は、ある事象の前提を満たす手順書として、その事象にぶら下げる」に従う。載せ方は次のとおり(語彙の事象は D1 の表を参照)。

| skill | 語彙 | 事象(担い手として) |
|---|---|---|
| `requirements-inventory` | `plan` | `draft`・`self_check`(`plan-requirement` にも既に載っている) |
| `precedent-grounding` | `plan` | `ground`・`self_check` |
| `technology-selection` | `plan` | `ground`・`self_check` |
| `test-grounding` | `plan` | `compose` |
| `diagramming` | `plan` | `compose` |
| `stacked-pr` | `pull-request` | `create`・`stack_link`・`unstack`・`merge` |
| `pr-description` | `pull-request`・`wip-pull-request` | `create`・`handoff` |
| `living-description` | `pull-request`・`issue` | `update_body` |
| `tracking-issue` | `issue` | `file`・`promote`・`attach`・`complete`・`abandon` |
| `issue-hygiene` | `issue` | `promote`・`attach`・`complete` |
| `issue-ref-freshness` | `issue` | `refresh_refs` |
| `handoff` | `wip-pull-request` | `handoff`・`resume`・`ready` |

12 本はすべて名指しされた。`--skills` の載っていない skill は 34 本から 22 本になった(残りは別の対象に属する skill で、この段では扱わない)。

### D5: 機械の検査が無い遷移は、昇格しない

`tracking-issue`・`issue-hygiene`・`living-description`・`issue-ref-freshness` の遷移について、ADR-543 の昇格の兆候(同じ規範違反が繰り返し起きる、同じ手順が何度も逐語で実行される)を探した。
見つかった記録の範囲では、**兆候なし**とする。根拠は Consequences の「昇格の判断」に書く。

### D6: 重複・衝突は、統合せず Issue に切り出す

遷移表にして初めて見えた重複・衝突を、同じ事象の `carriers` に両方の skill を並べて残し、別の Issue に切り出す(ADR-1293 D2 のとおり、この段で skill の統合はしない)。

## 先行例との対比

- D1: 4 対象の語彙を、共通の形で `config/agents/<対象>/vocabulary.json` に起こす。
  先行例: ADR-1293 D3・D4(語彙 JSON の共通の形と JSON Schema。対象のデータがある側に置く) docs/adr/1293-state-transition-vocabulary.md (取得 2026-10-31)
  差分: 一致 — 形・置き場・グラフ検査をそのまま使い、対象を足す。本文に無い状態は作らない。
  軸: 表現不可能 — 閉語彙。担い手・前提の名前が実在に解決しない語彙は書けない(`vocab-graph-check` が落とす)。
- D2: `plan` は計画 1 枚の流れを持ち、要求項目の処分は `plan-requirement` に任せる。
  先行例: ADR-1297 D2(具体化は対応表として語彙の外に置き、抽象の状態を各語彙へ複写しない) docs/adr/1297-awaiting-item-abstraction.md (取得 2026-10-31)
  差分: 異なる — `plan` は `awaiting-item` の具体化にしない(1 計画が項目を複数持つ関係で、状態の対応表にならない)。要求項目の処分を持たないことで、単一正本を保つ。
  軸: 表現不可能 — 単一正本。項目の処分を 2 つの語彙に複写しない。
- D3: PR の語彙を `pull-request` と `wip-pull-request` に分け、`ready` の事象でつなぐ。
  先行例: docs/agents/handoff.md の D3(pr-gate は Draft かつ `Handoff: #N`(N が open Issue)のときだけ G_CI を advisory に落とし、G_link の充足とみなす) docs/agents/handoff.md (取得 2026-10-31)
  差分: 一致 — 緩和の条件を `relax` の事象と `shown_by`(`issue-index`)として書き起こす。pr-gate の挙動は変えない。
  軸: 表現不可能 — Draft の PR が通常の PR の状態に混ざる語彙を書けなくする(`ready` の前後で別の対象)。
- D4: 書き方の skill を、その事象の `carriers` に並べる。
  先行例: ADR-1293 D1・D2(書き方の skill は事象の前提を満たす手順書として、その事象にぶら下げる) docs/adr/1293-state-transition-vocabulary.md (取得 2026-10-31)
  差分: 一致 — `gates` は機械の検査に使い、skill は `carriers` に置く(`plan-requirement` の `requirements-inventory` と同じ)。
  軸: 検出のみ — 名前の実在は検査するが、その skill が実際にその事象を行うかは読んで確かめる。
- D5: 機械の検査が無い遷移を昇格させない。
  先行例: ADR-543 D2(散文は仕様書として残るので昇格コストが小さく、安定の兆候が出たときに決定論へ昇格させる) docs/adr/543-existing-means-and-deterministic-promotion.md (取得 2026-10-31)
  差分: 一致 — 兆候の探し方と、探した範囲を Consequences に残す。
  軸: 検出のみ。
- D6: 重複・衝突を、統合せず Issue に切り出す。
  先行例: ADR-1293 D2(名指しできない skill は統合の候補とし、新しい skill を足すときも同じ条件を課す) docs/adr/1293-state-transition-vocabulary.md (取得 2026-10-31)
  差分: 一致 — 統合の判断は Issue に委ね、この段では skill の本文を変えない。
  軸: 検出のみ。

## 却下した案

- `plan` を `awaiting-item` の具体化にする。D2 のとおり、1 計画が複数の項目を持ち、対応表にならない。
- `plan` に要求項目の状態(`listed` など)を複写する。`plan-requirement` と二重になる。
- 1 つの語彙 `pull-request` に Draft と `Handoff:` の状態を入れる。`relax` の前提(Draft かつ `Handoff:`)が通常の PR の状態に混ざる。
- この段で `tracking-issue` と `issue-hygiene` を統合する。ADR-1293 D2 のとおり、この段では統合しない。

## Consequences

### 担い手が重なる事象(遷移表で見えたもの)

- **`tracking-issue` と `issue-hygiene`**: 親の昇格(`promote`)・子の操作(`attach`)・親の閉じ方(`complete`)の 3 事象で、2 つの skill が担い手に並ぶ。
  `docs/agents/issue-hygiene.md` は、`tracking-issue` を切り出した理由として「両スキルが競合せず住み分ける」と書く。それでも `issue-hygiene` の SKILL.md §2・§5・§6 に、
  操作の手順・`tracking` ラベル・閉じ方が残る。食い違いは次の 3 点で、#1299 に切り出した: (1) §2 のコマンドに `-R` が無い(`tracking-issue` §2 は正準形で必須と書く)。
  (2) 閉じる条件が、`tracking-issue` §7 は「全 sub-issue closed かつ完了定義の充足」、`issue-hygiene` §5 は前者だけ。(3) ラベルの記述が両方にある。
  `issue-hygiene` は 3 事象とも `tracking-issue` と共有で、単独で担う事象が無い。#1299 の裁定が出れば、統合の候補になる。
- **`plan` の `approve` と `plan-requirement` の `approve`**: どちらも ExitPlanMode の承認で、行為者は person。`plan` は計画 1 枚、`plan-requirement` は項目 1 行の承認で、
  対象が違う。`plan-requirement` の `approve` の `by_detail` も ExitPlanMode なので、1 回の承認の操作が 2 つの語彙の `approve` に当たると読める(この読みは、語彙の記述の突き合わせによる)。
  この関係は語彙に書いていない(語彙は 1 対象の遷移表で、対象をまたぐ性質は書かない。ADR-1293 D7 の条件(3))。重複の候補として記録する。
- 重なりだが、一方が他方に委ねているので Issue にしないもの: `living-description` と `tracking-issue` §4(「living-description の一種」と書く)、`issue-ref-freshness` §5 と `living-description`(本文の直し方を委ねる)、
  `requirements-inventory` §4 と `stacked-pr` §1(隣接負債の振り分け)、`precedent-grounding` §3 と `technology-selection` §3(先行例の節の書式。後者が重い欄を足す層)。
- 別の食い違い(語彙には関係しない)として、`precedent-grounding` §3 が `既存手段:` の書式の参照先を `technology-selection` §6 と書く(書式は §4)。#1300 に切り出した。

### 載せ方の弱いもの

- `diagramming`・`test-grounding` は、計画の `compose` の担い手として載せた。載せたのは #1268 の表による。どちらの SKILL.md の本文も、計画に載せる図・検証項目に限定していない
  (`test-grounding` は検証項目・試験手順を書く手順書、`diagramming` は docs・README・Artifact の図を描く手順書)。本文からは、計画との結びつきを読み取れない。統合の候補ではない。
  載せ方を変えるなら、別の語彙(検証項目の起草、図の作成)を足す段で決める。

### 不明にした遷移(本文に書かれていないので語彙に書かない)

- 計画: 承認されなかった計画(本人が承認の画面で差し戻したとき)の遷移。`awaiting_approval` から `drafting` へ戻る事象は、本文に書かれていない。`deny` は、gate が ExitPlanMode を拒否して計画が差し戻される場合(`requirements-inventory` §1a)に限る。
- PR: マージせずに閉じた PR の扱い。`stack_link` のあとに最上段以外を単独でマージしたときの挙動(`stacked-pr` §5 が「中間段を指定したときの挙動は未確認」と書く)。
- Issue: 放棄以外の理由で手で閉じる通常の Issue の遷移。`issue-hygiene` §8 が「ユーザーの明示判断なしに閉じない」とする停滞 Issue の扱い。親の完了定義が満たされているかを誰がどう判断するか(`tracking-issue` §7 は条件を書くが、判断の手順を書かない)。
- WIP の PR: 子 Issue のうち一部だけを終えたときに、親の `Closes` を書く境界(`handoff` 再開の 3 が「この merge で親の全 sub-issue が closed になり、かつ完了定義も満たすときだけ」と書く条件の判断)。`handoff` を使わずに Draft のまま放置された PR の扱い。

### 昇格の判断(ADR-543。機械の検査が無い 4 つの skill)

探した範囲(2026-10-31)と結果。

- リポジトリの Issue と PR(`gh search issues`・`gh search prs --repo`)で、`tracking-issue`・`issue-hygiene`・`living-description`・`issue-ref-freshness`・鮮度点検を探した。
  導入 PR は `living-description` が #83、`issue-hygiene` が #116、`tracking-issue` が #145(`issue-ref-freshness` の導入 PR は検索で見つからなかった。docs/agents/issue-ref-freshness.md に初回適用が 1 回ある)。
  導入後に、同じ規範違反を扱った Issue・PR は見つからなかった(検索語は skill の名前と「鮮度点検」。語の選び方による取りこぼしは未確認)。
- `docs/agents/tracking-issue.md` に、2026-09-09 の実測(チェックボックス 30 個のうちチェック済み 0 個、`#10`・`#11` は未チェックのまま閉じられていた)がある。これは `tracking-issue` を導入する前の状態で、導入後の再発の記録ではない。
- 導入後の再発を直接数えた。最新 400 件の Issue(すべて 2026-09-10 以降の作成)に、子 Issue を指す未チェックの `- [ ] #N` を本文に持つものは 0 件だった。
- `config/agents/skills/living-description/cases.md` の事例は 1 件。`docs/agents/living-description.md`・`docs/agents/issue-ref-freshness.md` も、事例 1 件の記録である。

**兆候なし**とする。`issue-hygiene` §7 の 3 経路の洗い出しと `issue-ref-freshness` §3 の走査は、すでにコマンドの形で書かれていて、スクリプトに昇格できる形をしている。
実行の回数(同じ手順が逐語で何度も実行されたか)を示す記録は読めていない(コマンド履歴の集計は未確認)ので、昇格の根拠にしない。

### そのほか

- `scripts/vocab-graph-check --skills` の載っていない skill は 34 本から 22 本になった。必須の CI に配線するのは、全 skill を載せ切る段(ADR-1293 D6)のままである。
- 作業の枠(`task-slot`)は触らない。時間の定義が予定データ側にあり、dotfiles の語彙は完了記録(`none` → `done`・`dropped`)だけである(ADR-1295)。
- `config/agents/vocabulary/external-objects.json` は触らない。

## 執行点

- `config/agents/plan/vocabulary.json`
- `config/agents/pull-request/vocabulary.json`
- `config/agents/issue/vocabulary.json`
- `config/agents/wip-pull-request/vocabulary.json`
- `scripts/vocab-graph-check`
- `.github/workflows/ci.yml`
