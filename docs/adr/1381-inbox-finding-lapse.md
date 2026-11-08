# ADR-1381 — 作業中の気づきの `open` に 14 日の期限を置き、超えたものを非終端の `lapsed` にする

- Status: Accepted
- Date: 2026-11-08
- Issue: No-Issue(本人の Issue 台帳の運用で、wrap-up inbox の `open` が無限に溜まる問題を、裁定として決めた。台帳を持つリポジトリの名前と番号は、この公開リポジトリには書かない)
- Amends: ADR-1297(D3「失効の状態は設けない」と、「却下した案」の「気づきに失効の状態を足す」を改める。抽象の遷移表 `awaiting-item` は変えない)

## Context

wrap-up inbox(各セッションが範囲外の気づきを vault の `inbox/` に 1 件ずつ足す。ADR-1136)の気づきは、本人が `/backlog-cleanup` で裁定するまで `open` のままである。
裁定が追いつかないと、`open` は無限に溜まる(2026-11-08 に実 vault を読んだ件数: `open` 129 件。最初の登録は 2026-10-25 で、1 日に約 12 件ずつ増えている)。

放置への手段は、ADR-1297 D4 の性質 P1(放置された項目は失効するか再通知される)の担い手として、セッション開始時の hook(`session-wrapup start`)が出す件数の再通知だけだった。
この再通知は「そのプロジェクトでセッションを始めたとき」にしか出ず、別のプロジェクトの `open` には届かない(ADR-1297 の Consequences に限界として書いた)。
ADR-1297 は失効の状態を足す案を「機械に捨てる判断を渡さない」として却下していた。

本人は 2026-11-08 に裁定した。**気づきの `open` に 14 日の期限を置き、超えたものを非終端の `lapsed` にする。**
`lapsed` は終端でなく、同じ題の気づきが再発すれば `open` に戻る。裁定の手順(`--set-status`)で `settled`(裁定済み)にも進める。

## Decision

### D1: 状態 `lapsed`(非終端)を足し、`open` が最後の行から 14 日以上たつと `lapsed` になる

- 状態: `lapsed`(owner は machine、非終端)。事象 `lapse`(`open` → `lapsed`、行為者 machine、担い手は `node:promotion-detect` と `cli:issue-inbox`)。
- 時計は**最後の行の日付**から数える。行は初出と再発の 1 回ごとに 1 行ずつ足されるので、初出の日(`first_seen`)ではなく、行の日付のうち最も新しいものを使う。
  再発を続けている気づきは、まだ生きている。行の日付を 1 つも読めないときだけ `first_seen` を使い、それも読めなければ判断できないので `open` のまま残す。
- 期限は 14 日(語彙 JSON の `timers.lapse_after_days`)。**14 日ちょうどで失効する**(13 日では失効せず、14 日と 15 日は失効する)。
  根拠は本人の裁定の例で、2026-10-25 の登録は 14 日後の 2026-11-08 に初めて失効する。
- 再発(`--add` で同じ題が足される)は、`fixed`・`dropped` と同じく `lapsed` を `open` に戻す(`reopen` の `from` に `lapsed` を足す)。別の項目にはしない(同じ題は同じノート、本文の行数が再発の回数)。
  戻ったあとの時計は、足された再発の行から数え直す。
- 裁定の手順: `lapsed` から `fixed`・`dropped` へは `--set-status` で進める(`fix`・`drop` の `from` に `lapsed` を足す)。`issued` へは進めない。
  抽象 `awaiting-item` に `settled` から `handed_off` への事象が無く、その表は変えないので、準同型(D3)を破る。Issue にするには、再発して `open` に戻ってからにする。
  `--set-status` は現在の状態を見ずに書く(ADR-1297 の Consequences)ので、コードは `lapsed` から `issued` へも書ける。語彙が想定する使い方に含めない、という意味である。
- `--set-status` は `lapsed` を書かない。`lapsed` を書くのは掃除(D2)だけで、行為者は machine のままである。
- `lapsed` のノートは `--list-open` に出ず、セッション開始の件数通知にも数えない。再発して `open` に戻るか、Obsidian で見つけて裁定するまで、静かに待つ。

- 本命: 非終端の `lapsed` に、再発で `open` へ戻る事象を持たせる(`fixed`・`dropped` と同じ型)。
- 対抗馬: 失効を終端(`dropped` に畳む、または終端の `lapsed`)にする案。表現不可能性: 終端にすると、同じ気づきが再発したとき `open` に戻せず、別のノートを作る必要が出る。
  同じ題が別のノートになると、本文の行数が再発の回数、という ADR-1136 の設計が壊れる。非終端にすると、再発した気づきを `open` に戻す経路が 1 つ(`reopen`)に保てる。本命が勝つ。
- 既存手段: 拡張 — `reopen`・`fix`・`drop` の `from` に `lapsed` を足し、`add_item` の再発の判定に `lapsed` を加える(`crates/issue-inbox/src/vault.rs`)。新しい仕組みは足さない。
- 先行例: GitHub Actions `actions/stale`(README。"Warns and then closes issues and PRs that have had no activity for a specified amount of time."、"If an update/comment occur on stale issues or pull requests, the stale label will be removed and the timer will restart") https://github.com/actions/stale (README を `gh api repos/actions/stale/contents/README.md` で取得 2026-11-08。リポジトリの最終 push は 2026-10-28、archived は false。著者は GitHub、公開日は取得した資料に記載なし)
- 先行例: ADR-1259(メール項目の `stale`。7 日で再通知し、さらに 7 日で `stale`) docs/adr/1259-mail-lifecycle.md (取得 2026-11-08)
- 差分: 一致 — 一定の日数の無活動で状態が変わり、その後に活動があれば元に戻って時計が数え直しになる点は `actions/stale` と同じ。異なる — `actions/stale` は stale のあとさらに日数がたつと close する(終端がある)。
  ここは `lapsed` から先の自動の遷移を持たず、終端は本人の裁定(`issued`)だけである。ADR-1259 の `stale` は終端(抽象の `lapsed`)で、再発で戻らない。気づきは再発で戻る。
- 軸: 表現不可能 — 終端にしないことで「再発の気づきが別のノートになる」状態を作れなくする。

### D2: 掃除は `issue-inbox --sweep` が行い、日次の `promotion-detect` が全 repo のノートに対して呼ぶ

- `issue-inbox --sweep [--dry-run] [--as-of YYYY-MM-DD]`: `open` のうち期限を超えたものを、全 repo について `lapsed` にする。`status` だけを書き換え、本文はバイト単位でそのまま残す(一時ファイル + rename)。
  `--dry-run` は何も書かず、対象の件数と一覧を出す。`--as-of` は日付を差し替える見積り用で、`--dry-run` とだけ組み合わせられる(本番の掃除で日付を誤って渡して失効させる事故を、引数の形で防ぐ)。
- **冪等**: 一度 `lapsed` にしたノートは `open` でないので、繰り返しても二重に書かない。`fixed`・`dropped`・`issued` には触れない。
- 呼び出しは `promotion-detect` の日次 timer(ADR-543、ADR-1311 が Git の外の状態の整合検査を載せた timer)。セッション開始の hook には載せない。
  vault の `inbox/` は repo をまたいで 1 つなので、掃除は repo を絞らず全ノートを読む。これで、別のプロジェクトの `open` も、セッションを始めなくても失効する。
  `--dry-run` で呼ぶと、書かずに件数だけを標準エラーに出す(標準出力は候補の JSON 用)。vault を宣言していない機体では何もしない(jsonl の行は状態を持たない)。
- 失効したノートの題が、同じ run で検出される候補の題と同じなら、`promotion-detect` が記録(`emitted.json`)から外して入れ直す。再発として `open` に戻る。
  外さないと、条件が続いている候補は「前回と同じ」のまま二度と入らず、14 日で黙って消える。

- 本命: `promotion-detect` の既存の日次 timer に、vault の掃除を載せる。
- 対抗馬: (a) セッション開始の hook(`session-wrapup start`)に掃除を載せる案。(b) 掃除専用の新しい timer の unit を足す案。
  表現不可能性: (a) は「そのプロジェクトでセッションを始めたとき」にしか動かず、ADR-1297 が限界として書いた状態(別のプロジェクトの `open` が放置される)を、掃除の側にも持ち込む。
  (b) は、日次の timer・サービス・launchd の双子・pipeline-map のノードを 1 組増やす。還元性: 既存の timer は同じ vault を日次で読んでいて(語彙の較正、`worklog mail check`)、(b) の担う仕事を担える。本命が勝つ。
- 既存手段: 拡張 — `crates/promotion-detect/src/main.rs` が `issue_inbox::vault::sweep` を呼ぶ(依存は既にある)。`home/modules/promotion-detect.nix` の timer は変えない。
- 先行例: ADR-1311(日次の `promotion-detect` に、vault の状態の検査を載せる) docs/adr/1311-git-external-state-integrity.md (取得 2026-11-08)
- 差分: 一致 — 同じ timer が、vault を宣言した機体だけで、vault の状態を日次で読む。異なる — 1311 は読むだけで、書き込みは `worklog` の書き込み口が担う。
  ここは timer が vault のノートの `status` を書く。書くのは機械の事象 `lapse` だけで、`--dry-run` は書かない。
- 軸: 還元 — 新しい timer を足さず、既存の日次の担い手に載せる。

### D3: 具体化は `lapsed → settled` と宣言し、`p1` の担い手を `timer` に改める。`awaiting-item.json` は変えない

- `config/agents/vocabulary/refinements/inbox-finding.json` に `lapsed → settled`(閉じているが、再発を待つ)を足す。抽象の `lapsed`(終端)には写さない。
  抽象の `lapsed` は終端で、`vocab-graph-check` は終端から出る写像を許さない(ADR-1293 D5)。再発で戻る `lapsed` は、抽象では `settled` と同じ形である。
- `p1.open` を、`notification`(セッション開始の `remind`)から `timer`(`timers.lapse_after_days`、事象 `lapse`)に改める。
  放置された `open` は、失効する(`lapse`)ことで P1 を満たす。`remind` は事象として語彙に残り、失効の前の再通知として動き続ける(P1 の担い手としては数えない)。
- 準同型は、具体の事象を抽象の状態へ写して確かめる。`lapse`(`open` → `lapsed`)は `shown` → `settled` で抽象の `settle` に、`reopen`(`lapsed` → `open`)は `settled` → `shown` で抽象の `reopen` に、
  `lapsed` から `fixed`・`dropped` へは `settled` → `settled`(stuttering)に写る。

- 本命: 具体の側だけを変え、抽象の表(`awaiting-item.json`)は変えない。
- 対抗馬: 抽象に「再発で戻る失効」の状態か、`settled` から `handed_off` への事象を足す案。表現不可能性: 抽象を変えると、4 つの対象すべての具体化の検査が影響を受ける。
  具体の側で `settled` に写せば足り、抽象を広げる必要が無い。抽象は単一正本のまま保てる。本命が勝つ。
- 既存手段: 拡張 — `scripts/vocab-graph-check --refinements`(新しいスクリプトは足さない)。
- 先行例: ADR-1297 D1・D2・D4(抽象の遷移表と、具体化の対応表、P1・P2) docs/adr/1297-awaiting-item-abstraction.md (取得 2026-11-08)
- 差分: 一致 — 抽象と対応表の分担、P1 の手段が閉語彙の `timer` であることは 1297 のとおり。異なる — 1297 の D3 は「失効の状態は設けず、件数通知を `remind` として載せる」としていた。それを改める。
  ADR-1297 が気づきを `fixed`・`dropped` → `settled` と写した型を、`lapsed` にも使う。
- 軸: 還元 — 抽象の表を広げず、具体の宣言で足りる仕事を具体に置く。

## 先行例との対比

D1〜D3 の各項の中に書いた。ADR-1297 の「却下した案」との差分をここにまとめる。

- ADR-1297 は「気づきに失効の状態(放置で `dropped`)を足す」案を、「再通知で P1 を満たせ、放置を自動で捨てる判断を機械に渡さない」として却下した。
  この ADR の `lapsed` は、その案と次の 3 点で違い、本人の 2026-11-08 の裁定で採った。
  - **非終端**: 却下された案は放置を `dropped`(捨てた)にする。`dropped` も再発で戻るが、「捨てた」という本人の判断の記録になる。`lapsed` は「捨てた」でなく「裁定されないまま期限が来た」で、判断を記録しない。
    機械が書くのは行為者 machine の `lapse` だけで、本人の裁定の結果である `fixed`・`issued`・`dropped` を機械が書くことは無い。
  - **再発で戻る**: 同じ題の気づきが再発すれば `open` に戻る。機械が失効させた気づきを、再発という新しい証拠が自動で取り戻す。捨てる判断が取り返しのつかないものにならない。
  - **本人の裁定で覆った**: 却下は 2026-10-30 の ADR-1297 の判断で、2026-11-08 に本人が、`open` に 14 日の期限を置く裁定で覆した。
    動機は Context のとおり、`open` が無限に溜まり、件数の再通知がそのプロジェクトでセッションを始めたときにしか出ないことである。

## 却下した案

- 時計を初出(`first_seen`)から数える。再発を続ける気づきが、再発のたびに生き返っても、次の日にまた失効する。最後の行から数えれば、続いている気づきは失効しない。
- 失効を終端にする、または `dropped` に畳む。D1 の対抗馬のとおり、再発で別のノートになるか、本人の「捨てた」判断を機械が書くことになる。
- セッション開始の hook に掃除を載せる。D2 の対抗馬のとおり、別のプロジェクトの `open` に届かない。
- `--set-status` で `lapsed` を書けるようにする。失効は行為者 machine の事象で、書くのは掃除だけにする。人や agent が手で `lapsed` を書けると、期限を待たずに隠せてしまう。
- `lapsed` から `issued` への遷移を語彙に足す。抽象の表を変える必要があり(D3 の対抗馬)、再発して `open` に戻ってからで足りる。

## Consequences

- 最初の失効は、最初の登録(2026-10-25)の 14 日後の 2026-11-08 に起きる。2026-11-08 に実 vault を `--dry-run` で数えた見込み(件数だけ。ノートは書き換えていない):

  | 日付 | `open` の件数 | 失効の対象 |
  |---|---|---|
  | 2026-11-08 | 129 | 1 |
  | 2026-11-15 | 129 | 110 |
  | 2026-11-22 | 129 | 129 |

  登録が最近の 2 週間に集中しているので、timer が走り始めて 1〜2 週間のうちに、ほぼ全件が失効する。この間に再発した気づき、または本人が `/backlog-cleanup` で裁定した気づきは、対象から外れる。
  失効は消去でなく、ノートは vault に残り、再発で `open` に戻る。
- `lapsed` のノートは `--list-open` にも件数通知にも出ない。再発しない限り、本人が Obsidian の Bases で `status = lapsed` を見に行かなければ目に入らない。
  見に行く手段(`--list-lapsed` のような一覧)は、この PR には入れていない(未実装。必要になったら足す)。
- **検査の限界**: `scripts/vocab-graph-check --refinements` の準同型は、具体の事象の行為者(`by`)を見ない。検査器の jq に行為者の比較が無く(準同型の規則)、`--selftest`(56 件が通る)にも行為者を壊す負例が無い。
  一時ディレクトリの複製で、`reopen` の行為者を `machine` から `person` に書き換えても `--all`・`--refinements` が通ることを実行して確かめた(2026-11-08)。したがって、行為者が machine の `lapse` を足しても検査が通ることは、準同型の検査が守っていない限界である。
  行為者を見るのは、P1 の検査(`p1` に書いた事象の行為者が `machine` であること)と、語彙と実装の突き合わせ(`vocabulary_matches_vault_rules` が `lapse` の行為者を `machine` と確かめる)だけである。
  `reopen` など `p1` に書かない事象の行為者は、どちらも見ていない。
- `vocabulary_matches_vault_rules` は、`sweep` を実際に動かして `lapse` の `from`・`to` を導き、`set_status` の行き先から `lapsed` を除き、`timers.lapse_after_days` が実装の定数と一致することを確かめる。
- `--set-status` で `open` を書き戻しても、時計は最後の行の日付のままなので、古い気づきは次の日の掃除でまた失効する。`open` に戻すのは再発(新しい行)である。
- 掃除は vault を宣言した機体だけで動く。jsonl の機体は、行が状態を持たないので対象外(従来どおり、`--set-status` が行を取り除く)。
- `promotion-detect` の掃除が失敗したときは、理由を標準エラーに 1 行出して続ける(次の日にやり直せる)。「検査を読めなかった」を候補にして inbox に流す形(`worklog mail check`)にはしていない。
- 「ADR-1297 の Consequences」の限界「気づきの `remind` は作業中のプロジェクトの `open` だけを数える」は、失効が全 repo の `open` に効くことで解消する。`remind` は失効の前の通知として残る。

## 執行点

- `config/agents/wrapup-inbox/vocabulary.json`
- `config/agents/vocabulary/refinements/inbox-finding.json`
- `crates/issue-inbox/src/vault.rs`
- `crates/issue-inbox/src/lib.rs`
- `crates/issue-inbox/tests/sweep.rs`
- `crates/promotion-detect/src/main.rs`
- `crates/promotion-detect/tests/cli.rs`
- `scripts/vocab-graph-check`
- `pipeline-map.toml`
