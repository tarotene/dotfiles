# ADR-0000 — Issue に置くのは、マージ後も残る状態を持ち、ほかに置き場が無いものだけにする

- Status: Accepted
- Date: 2026-11-11
- Issue: No-Issue(本人の依頼「Issue が多すぎて飽和しているので、系統的に減らす方法を考えたい」を、計画の承認と裁定(2026-11-11)で決めた)
- Amends: 共有 AGENTS.md の、起票を義務にしていた 5 つの規約(#765・#1265・#1233・#1255・#1273)の行き先。ADR-1136(気づきは inbox)と ADR-598(人の確認は後続 Issue)は変えない

## Context

2026-11-10T15:27Z に、このリポジトリの Issue を全件読んで数えた(`gh issue list --state all` の 617 件。open の 72 件は本文とコメントまで読んだ)。

- 累計 617 件(16 週)に対し、open は 72 件。週の起票とクローズはほぼ釣り合っていて(W44 は起票 131・クローズ 133)、閉じた 545 件のうち 237 件(43%)は 1 日以内に閉じた。
- 約 55%(339 件)は、無くても誰も困らなかった型だった: 「24 時間以内に PR が閉じ、コメントが無い」(200 件)か、「3 件以上を一度に閉じる PR で閉じ、コメントが無い」(214 件)の和集合。
- 最大の経路(wrap-up の起票、270 件)は ADR-1136(2026-10-27)で vault の inbox に移ったが、流入は約 7.9 件/日から約 9.9 件/日に増えた。量は次へ移った: PR の直前に立てる Issue(31 件、中央値 30 分)、自分の計画の段の sub-issue(#1434〜#1453 の 20 件は 1 分間に起票され、0.6 日後に閉じた)、実機確認の払い出し。
- open の在庫の中心は、実機確認が PR ごとに割れたもの(32 件)と、ほかの正本の写し(flake.nix の撤去条件・ADR の再訪条件・docs の未確認事項)だった。
- 共有 AGENTS.md の 5 か所が「Issue にする」と書く一方、ADR-1136 と SessionStart の注入は「気づきは自分で起票しない」と書き、規約どうしが矛盾していた。

Issue は本人が見返す待ち行列である。見返さない一覧が積もると、遅れている感覚が増え、本当に待っている項目が埋もれる(ADR-1136 の Context と同じ症状)。

## Decision

### D1: 起票の前に、置き場を決まった順で選ぶ

共有 AGENTS.md に節「Issue に置くのは、マージ後も残る状態を持ち、ほかに置き場が無いものだけ」を置く。順は次のとおりで、上で足りれば下へ進まない。

1. 今のセッションで直す(stack の一段)。
2. PR 本文(`No-Issue:`)。`Closes #N` を書くためだけに、直前に Issue を立てない。
3. wrap-up inbox(ADR-1136)。
4. ADR・docs・コードのコメント(条件待ち・保留の裁定・再訪の条件・上流への報告の下書き)。
5. 予定データ(本人の生活の作業。`human-task-scheduling`)。
6. 確認の台帳 Issue(マージ後の実機確認。ADR-598 の払い出し先を台帳に束ねる #1506)。
7. Issue。型は閉じた一覧: 複数のセッションにまたがる実装の残り、upstream の修正待ち、本人が採ると決めた構想、他人の参加が要るもの、フィードバック。

エージェントは、自分の計画の段を sub-issue に起こさない。段は計画の要求インベントリと stack の PR が持ち、セッションの後に残った段だけを 1 件の Issue にする。

起票を義務にしていた 5 つの規約は、行き先をこの順で選び直す。「なぜ?」(#765)は、根本原因の特定と直す案を同じターンで出す義務を残し、行き先だけを変える。

- 本命: 置き場の順を 1 つの節に持ち、ほかの節はそこを参照する。
- 対抗馬: 件数の上限(open が N 件を超えたら起票を拒む。Holub "#NoBacklog")。表現不可能性: フィードバックと retro の user-correction は起票を義務にしているので、上限で拒むと規約どうしが衝突し、義務を果たせない状態を作る。順は「どこに置くか」を決めるだけで、義務と衝突しない。本命が勝つ。
- 既存手段: 拡張 — 置き場(inbox・ADR・予定データ・`No-Issue:`・台帳)はすでにある。新しい置き場は作らない。
- 先行例: Ryan Singer, "Shape Up" ch.7 "Bets, Not Backlogs"(Basecamp)https://basecamp.com/shapeup/2.1-chapter-07 — "Backlogs are a big weight we don't need to carry."(一次ページは 403 で、GitHub 上の転載 fenbox/shapeup の本文から引いた。取得 2026-11-11)
- 先行例: 37signals, "Getting Real" "Forget Feature Requests" https://basecamp.com/gettingreal/05.7-forget-feature-requests — "read them, throw them away, and forget them. … the ones that are important will keep bubbling up anyway."(転載 amirhhz/getting-real から引いた。取得 2026-11-11)
- 差分: 異なる — 先行例は捨てたものの再浮上を、外の多数の利用者に任せる。ここでは再浮上を担うのが本人と規約しかないので、捨てずに置き場を分ける。再発の信号は inbox の行数・feedback の `Target:` の集計(閉じた Issue も数える)・gate のイベント記録が持ち、Issue の open の件数には頼らない。
- 軸: 還元 — Issue でなくても担える仕事(PR・inbox・ADR・予定)を、Issue から外す。

### D2: 順から外れた起票を、日次で検出する

`promotion-detect` の日次の timer に、Issue の流量の検出(`crates/promotion-detect/src/issue_flow.rs`)を足す。

- PR の影: PR の `Closes` で、起票から 24 時間以内に閉じ、コメントが 0 件の Issue。直近 7 日に 2 件以上なら候補にする。
- 台帳の外の実機確認: 題が `human:`・`verify:`・`verify(` で始まるか `handoff:human` を持ち、題が `verify-ledger:` で始まらない Issue。直近 7 日に 1 件でもあれば候補にする。
- open の件数: bot の起票を除く open が 30 件を超えたら候補にする(件数は 10 件単位に丸め、毎日入れ直さない)。

候補は既存の出口(wrap-up inbox)に流し、本人が `/my-cleanup` で裁定する。同じ型が続けば、作成時の拒否(`issue-create-guard`)への昇格を検討する(ADR-543 の昇格型)。

- 本命: 検出から始め、再発したら拒否へ上げる。
- 対抗馬: 最初から `issue-create-guard` で拒否する。表現不可能性: 作成の時点では、その Issue が 1 日以内に PR で閉じるか、コメントを集めるかが分からない(未来の事実)。作成時に判定できる閉じた形(題・ラベル)だけで拒むと、正当な起票まで止める。検出は事後の事実で数えるので誤りが無い。
- 既存手段: 拡張 — `promotion-detect` の日次の timer と、wrap-up inbox への出口(`emit_candidate`)。
- 先行例: Allen Holub, "#NoBacklog" https://blog.holub.com/p/nobacklog (2026-07-14 公開)— "If you add AI to the mix, the backlog grows faster" / "Nothing goes on unless something comes off"(WebFetch の逐語引用で読んだ。取得 2026-11-11)
- 差分: 異なる — Holub は待ち行列の大きさを固定し、1 つ外さなければ入れない。ここは大きさを固定せず、無くても困らない型の発生率を数える(D1 の対抗馬のとおり、上限は義務の起票と衝突する)。open の件数は候補にするだけで、起票は止めない。
- 軸: 検出のみ — 作成時に判定できない事実(寿命・コメントの有無)を、事後に数える。

## 却下した案

- 通常の Issue に、無活動での自動クローズ(`actions/stale`)を入れる。閉じる判断は本人の裁定に残す(ADR-387、my-cleanup)。決まっているが未着手の意図が、活動が無いだけで閉じられて失われる。期限で閉じるのは、受付期限を本文に持つキリリク受付だけにする(別の段で扱う)。
- 一括の破産(open を全部閉じて、要るものだけ開き直す)。今の在庫は、本人の裁定ページで件ごとに処分を決める(推奨は全件の分類から作る)。

## Consequences

- 2026-11-11 に実データへ `promotion-detect --dry-run` を当てた結果: PR の影が直近 7 日に 20 件(#1434〜#1441 などの計画の段を含む)、台帳の外の実機確認が 11 件、bot を除く open が 60 件台。最初の日次の実行で、この 3 つの候補が inbox に入る。
- PR の影の判定は、PR の作成時刻を見ない(`gh issue list` の `closedByPullRequestsReferences` は PR の番号しか返さない)。計画では「起票から 1 時間以内に PR が付き」を条件にしていたが、寿命 24 時間・コメント 0 件・PR で閉じた、の 3 つに絞った。PR の直前の起票と、同じ日に別のセッションが PR を出した起票を区別しない。
- 検出は `tarotene/dotfiles` だけを読む(`PROMOTION_DETECT_REPO`)。ほかのリポジトリの流量は数えない。
- フィードバックの Issue 化は変えない。`promotion-detect` の再発の集計(`Target:`)の入力で、閉じた Issue も数えるので、早く閉じても信号は失われない。
- 起票を義務にしていた 5 つの規約は、行き先が変わるだけで、「その場で原因を特定する」「黙って落とさない」義務は残る。

## 執行点

- `config/agents/AGENTS.md`
- `crates/promotion-detect/src/issue_flow.rs`
- `crates/promotion-detect/src/main.rs`
- `crates/promotion-detect/tests/cli.rs`
- `config/agents/skills/pr-description/SKILL.md`
- `config/agents/skills/tracking-issue/SKILL.md`
- `config/agents/skills/stacked-pr/SKILL.md`
- `config/agents/skills/requirements-inventory/SKILL.md`
- `docs/agents/promotion-detect.md`
