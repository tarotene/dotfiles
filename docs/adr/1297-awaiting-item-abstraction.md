# ADR-1297 — 「裁定待ちの項目」の抽象の遷移表を定め、メール項目・気づき・設定のずれ・計画の要求項目を具体化として書く

- Status: Accepted
- Date: 2026-10-30
- Issue: #1267
- Amends: ADR-1293(`scripts/vocab-graph-check` に `--refinements` を足し、`--all` の探索に `config/agents/vocabulary/*.json` を加える。D1〜D7 の決定は変更しない)

## Context

「拾う → 本人にまとめて見せる → 本人が裁定 → 適用する/見送る/放置で失効」という同じ形の流れが、別々の語彙で複数の対象にある
(2026-10-30 に読んだもの)。

| 対象 | 状態の語彙 | 定義の場所 |
|---|---|---|
| メール項目 | awaiting / decided / closed / dismissed / stale / superseded | `config/agents/mail-lifecycle/vocabulary.json` |
| 作業中の気づき | open / fixed / issued / dropped | `crates/issue-inbox/src/vault.rs`(遷移表は無く、再開の規則がコードにだけある) |
| リポジトリ設定のずれ | GO / 修正 / 除外 / exempt | `config/agents/skills/github-audit-triage/SKILL.md` §4 |
| 計画の要求項目 | 段 N で実装 / Blocked-Upstream / Obsolete / User-Excluded | `config/agents/skills/requirements-inventory/SKILL.md` §2。`plan-scope-gate` が検査 |
| 裁定ページの選択 | decide / pass / later | `config/agents/skills/inbox-adjudication/SKILL.md` |

語彙は対象ごとに事情が違うので統一しない(本人の裁定、2026-10-30)。代わりに、共通の形を持つ抽象の遷移表を 1 つ定め、各対象をその具体化
(具体の状態を抽象の状態へ写す対応表)として書く。放置された項目の扱いのような共通の性質は、抽象の側で 1 度だけ定め、各対象が守ることを
CI で示す。気づきには放置で失効に当たる状態が無く、再開(`fixed`・`dropped` が再発で `open` に戻る)の規則はコードにだけあった。

## Decision

### D1: 抽象の遷移表 `awaiting-item` を `config/agents/vocabulary/awaiting-item.json` に置く

状態は `picked`(拾った)・`shown`(見せた。owner が person)・`decided`(裁定済み)・`settled`(適用または見送りのあと、再発を待つ。**非終端**)・
`applied`(適用)・`declined`(見送り)・`lapsed`(失効)・`handed_off`(別の追跡先へ移した)の 8 つ。終端は `applied`・`declined`・`lapsed`・`handed_off`。
事象は `pick`(none → picked)・`show`(none・picked → shown)・`remind`(shown の自己遷移、machine)・`decide`(shown → decided、person)・
`apply`(none・shown・decided → applied)・`decline`(none・picked・shown・decided → declined)・`lapse`(picked・shown・decided → lapsed、machine)・
`hand_off`(shown・decided → handed_off)・`settle`(shown・decided → settled)・`reopen`(settled → shown、machine)。
共通の形(ADR-1293 D3)の語彙なので、`scripts/vocab-graph-check --all` の検査(到達・出口・終端など)をそのまま受ける。

`settled` を非終端に置くのは、再発で閉じた項目が戻る対象(気づき)が、終端から出る遷移を許さないグラフ検査(ADR-1293 D5)と両立するため。
`handed_off` を `applied` と分けるのは、メール項目の `superseded`、気づきの `issued`、設定のずれの `routed` の 3 つが、
裁定の結果の適用でなく「別の追跡先へ移し、以後はそちらが追う」終わり方で、`applied` に混ぜると移した先を追う必要の有無が消えるため。

### D2: 具体化は `config/agents/vocabulary/refinements/<対象>.json` に書き、形は `refinement.schema.json` で閉じる

必須の欄は `concrete`(具体の語彙の `object`)・`abstract`(`awaiting-item`)・`states`(具体の状態 id → 抽象の状態 id、全状態)・`p1`。
任意で `reasons`(自明でない対応の理由)と `inputs`(本人の入力と具体の事象の対応)を持つ。具体の語彙は 4 つ: メール項目(既存)、
作業中の気づき(`object: "inbox-finding"`、`config/agents/wrapup-inbox/vocabulary.json`)、リポジトリ設定のずれ(`repo-config-drift`、
`config/agents/repo-config-drift/vocabulary.json`)、計画の要求項目(`plan-requirement`、`config/agents/plan-requirement/vocabulary.json`)。
裁定ページの選択(`decide`・`pass`・`later`)は独立の状態機械ではないので語彙を持たず、メール項目の対応表の `inputs` に書く:
`decide` → 事象 `decide`、`pass` → 事象 `pass`、`later` → 事象なし(何も書かない。状態は動かず、時計が動き続ける)。

対応づけられない状態は、次の抽象の状態にぶら下げ、理由を `reasons` に書く。

- `superseded`(メール)・`issued`(気づき)・`routed`(設定のずれ)→ `handed_off`。別の追跡先へ移した終わり方で、以後はそちらが追う。
- `fixed`・`dropped`(気づき)→ `settled`。適用か見送りかは具体が持ち、抽象は「閉じているが、再発を待つ」だけを見る。
- `obsolete`(計画の要求項目)→ `lapsed`。誰の裁定も経ずに役目を終えた。`blocked_upstream`・`user_excluded` → `declined`。
- `excluded`・`exempt`(設定のずれ)→ `declined`。`closed`(メール。`read` と `apply` の両方)→ `applied`。

### D3: 気づきの再開と失効を、語彙 JSON に書く

`fixed` と `dropped` は、再発で `open` に戻る**非終端**(「閉じているが、再発を待つ状態」)で、機械が行う事象 `reopen` を持つ。終端は `issued` だけ
(以後は Issue が追い、再発しても戻らない)。再発を別の項目にはしない(同じ題名は同じノートで、本文の行数が再発の回数)。
**失効の状態は設けない。** 性質 P1 は、セッション開始時に hook(`session-wrapup start`、`node:session-wrapup-start`)が出す「未裁定の気づきが N 件ある」を、
機械が行う再通知の事象 `remind` として語彙に載せて満たす。実装は変えない。語彙が実装と矛盾しないことは、`crates/issue-inbox/src/vault.rs` の
`vocabulary_matches_vault_rules` が、`add_item` と `set_status` の実際の動きから導いた集合と語彙を突き合わせて確かめる。

### D4: 共通の性質 P1・P2 を、抽象の側で 1 度だけ定め、`--refinements` が各対象の具体化を検査する

- **P1**: 放置された項目は、失効するか再通知される(または、gate が裁定なしに先へ進めさせない)。抽象の `shown` から出る machine の事象があること、
  `shown` に写る具体の状態ごとに `p1` が書かれ、その名前が具体の語彙に実在すること。`p1` の `kind` は閉語彙: `timer`(語彙の `timers` のキーと機械の事象)・
  `reminder-event`(機械の再通知の事象)・`notification`(機械の事象と、その `carriers` にある担い手)・`gate`(その状態から出る事象の `gates`)。
- **P2**: 裁定を待つ項目は、本人に見える場所に出す担い手を持つ。`shown` に写る具体の状態に `shown_by` があること。
- **準同型**: 具体の各事象(from → to)を抽象の状態へ写すと、抽象の事象になるか、同じ状態への自己遷移(stuttering)になる。
- 加えて `schema`・`参照`・`写像`(全状態が写される)・`終端`(具体の終端は抽象の終端に写る)・`入力` を見る。

各系統が P1・P2 を満たす手段は次のとおり。

| 対象 | P1 | P2(`shown` に写る状態の `shown_by`) |
|---|---|---|
| メール項目 | timer: `remind_after_days` と `remind`(7 日で再通知、さらに 7 日で `stale`) | `inbox-adjudication`・`daily-desk` |
| 作業中の気づき | notification: `session-wrapup-start` の `remind` | `backlog-cleanup` |
| リポジトリ設定のずれ | reminder-event: `redetect`(`github-audit` の次の実行が同じ組を報告する) | `github-audit-triage` |
| 計画の要求項目 | gate: `plan-scope-gate`(処分の無い項目があると、承認の依頼まで進めない) | `requirements-inventory` |

### D5: 検査器は ADR-1293 の `scripts/vocab-graph-check` を拡張し、CI の `dry-run` ジョブに配線する

`--refinements [--root <ルート>] [<対応表.json>...]` を足す。違反は `ファイル: 規則: 内容` の 1 行ずつを標準エラーに出し、終了コード 1。
`--selftest` は、実物の語彙と対応表を一時ディレクトリに写して 1 か所ずつ壊し、規則ごとに落ちることを確かめる(状態が写されていない、準同型を破る、
P1 の無い shown の状態、P2 の無い状態、P1 の名前が語彙に無い、抽象に再通知も失効も無い、など)。`--all` の探索は
`config/agents/vocabulary/*.json`(抽象の語彙)にも広げる。`.github/workflows/ci.yml` の `dry-run` に `--refinements` を足す。

## 先行例との対比

- D1: 「裁定待ちの項目」の抽象の遷移表を 1 つ置き、各対象をその具体化として書く。
  先行例: Martín Abadi, Leslie Lamport, "The Existence of Refinement Mappings", DEC Systems Research Center Research Report 29, 1988-08-14, https://lamport.azurewebsites.net/pubs/abadi-existence.pdf (取得 2026-10-30)。「A refinement mapping from a lower-level specification S1 to a higher-level one S2 is a mapping from S1's state space to S2's state space. It maps steps of S1's state machine to steps of S2's state machine」。後続の査読版(Theoretical Computer Science 82(2), 1991)は読んでいない。
  差分: 異なる — 原典は振る舞い(状態の列)の包含と活性まで扱う。ここは具体の事象 1 歩ごとが抽象の事象か stuttering に写ること(準同型)だけを検査し、活性は P1 を別に置く。
  軸: 表現不可能 — 各対象が抽象にない遷移を足すと CI で落ち、共通の形から外れた語彙を書けない。
- D2: 具体化を対応表(状態 id → 状態 id)として語彙の外に置く。
  先行例: ADR-1293 D3(語彙 JSON の共通の形と JSON Schema。語彙の欄を増やして 1 枚に持たせる) docs/adr/1293-state-transition-vocabulary.md (取得 2026-10-30)
  差分: 異なる — 抽象の状態を各語彙の欄に書くと、抽象の変更が全語彙の編集になる。対応表を別ファイルに分け、抽象の語彙を単一正本にする。
  軸: 表現不可能 — 単一正本。抽象の状態を各語彙へ複写しない。
- D3: 気づきの `fixed`・`dropped` を再発で戻る非終端にし、失効の状態を設けず、件数通知を再通知の事象として載せる。
  先行例: crates/issue-inbox/src/vault.rs の `add_item`(再発で fixed・dropped を open に戻し、issued は動かさない)と docs/adr/1136-wrapup-inbox-to-vault.md (取得 2026-10-30)
  差分: 一致 — 実装済みの規則を語彙に書き起こす。コードの挙動は変えない。
  軸: 検出のみ — 語彙と実装のずれはテストで検出する。
- D4: 性質 P1・P2 を抽象の側で定め、P1 の手段を 4 種の閉語彙にする。
  先行例: ADR-1259 D3(メール項目の `remind`・`stale` と、時間の決まり) docs/adr/1259-mail-lifecycle.md (取得 2026-10-30)
  差分: 異なる — 時間の決まりだけだった放置への手段を、機械の再通知の事象・件数通知・gate にも広げる。手段の実在だけを検査し、働くかは検査しない。
  軸: 検出のみ。
- D5: 具体化の検査を `scripts/vocab-graph-check` の拡張として足す。
  先行例: ADR-1293 D5(bash + jq のグラフ検査器と `--selftest`) docs/adr/1293-state-transition-vocabulary.md (取得 2026-10-30)
  差分: 一致 — 実装言語・検証器・負例の作り方を引き継ぐ。
  既存手段: 拡張 scripts/vocab-graph-check (新しいスクリプトは足さない)
  軸: 検出のみ。

## 却下した案

- 語彙を 1 つに統一する。対象ごとに事情が違い(メールは時計で失効し、気づきは再発で戻る)、本人が採らないと裁定した。
- 気づきに失効の状態(放置で `dropped`)を足す。再通知で P1 を満たせ、放置を自動で捨てる判断を機械に渡さない。
- 再発を新しい項目にする。同じ題名は同じノートで、行数が再発の回数という既存の設計(ADR-1136)を壊す。
- 抽象の `applied` に `superseded`・`issued`・`routed` をぶら下げる。移した先を追う必要の有無が抽象から消える(D1)。

## Consequences

- 4 つの対象が同じ抽象に写り、P1・P2 を満たさない対象を足すと CI(`--refinements`)が落ちる。
- 次の遷移は、本文に書かれていないので語彙に書かず、**不明**とする。
  - 設定のずれ: 「修正」を返したあとの遷移。「除外」の意味(今回の組だけ外すのか)。適用後に再び drift した組の扱い(監査が毎回導出するので、新しい項目として拾われると読めるが、本文にない)。
    低確信フラグの組と誘導の Issue 起票を `approved` からの `route` と置いたのは、GO の返答に続く送付という読みで、本文は明示していない。
  - 計画の要求項目: 承認されなかった計画の項目の扱い(セッションが終わったとき)。`Blocked-Upstream:` で閉じた項目の、ブロックが解けたあとの再開。
    閉じたタグを列挙の時点の処分として `listed` から終端へ直接出したのは本文の読みで、本人の承認を経るかは本文にない。
- 限界(P1 は手段の実在だけを見る):
  - 設定のずれの `redetect` は、`github-audit` を手動で実行したときだけ働く。定期の実行は `pipeline-map.toml` に無い(lane は manual)。
  - 気づきの `remind` は作業中のプロジェクトの `open` だけを数える。別のプロジェクトの `open` は、そこでセッションを始めるまで再通知されない。
  - 計画の要求項目の gate は、処分の有無の完全性を検査し、承認そのものは `ExitPlanMode` が担う。
- 気づきは「裁定済み」を記録しない。本人の裁定は `backlog-cleanup` の問いで行われ、結果(`fixed`・`issued`・`dropped`)だけがノートに残る。
- `issue-inbox --set-status` は現在の状態を見ずに状態を書くので、`issued` から `open` へも書ける。語彙は `open` からの遷移として書き、`issued` を終端とした。コードは変えていない。
- #1267 の「daily-desk は、これらを 1 か所で本人に見せる入口」は、`daily-desk` の SKILL.md と docs/agents/daily-desk.md に気づき・設定のずれ・要求項目の記述が無いので、語彙の `shown_by` には載せていない
  (手順の正本は別のリポジトリの `desk.md` で、ここでは読めない。未確認)。抽象の `shown_by` は、各具体化の見せ場の和集合。

## 執行点

- `config/agents/vocabulary/awaiting-item.json`
- `config/agents/vocabulary/refinement.schema.json`
- `config/agents/vocabulary/refinements/mail-item.json`
- `config/agents/vocabulary/refinements/inbox-finding.json`
- `config/agents/vocabulary/refinements/repo-config-drift.json`
- `config/agents/vocabulary/refinements/plan-requirement.json`
- `config/agents/wrapup-inbox/vocabulary.json`
- `config/agents/repo-config-drift/vocabulary.json`
- `config/agents/plan-requirement/vocabulary.json`
- `scripts/vocab-graph-check`
- `crates/issue-inbox/src/vault.rs`
- `flake.nix`
- `.github/workflows/ci.yml`
