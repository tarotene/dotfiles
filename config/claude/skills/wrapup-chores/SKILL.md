---
name: wrapup-chores
description: wrap-up inbox に溜まった未起票の気づきと、起票済みの wrapup 由来 open Issue をまとめて triage し、1 回の確認(GO)後に 1 つの chores PR で一括対処する手順。inbox 消化・wrapup 消化・chores をまとめて・雑務を片付けて・wrapup をまとめて処理・溜まった Issue を一掃、sweep the wrap-up inbox, batch chores PR, clean up wrapup issues といった文脈で使う。
---

wrap-up inbox の項目は「判断無しで即対処できる」ものが多く、1 件ずつ Issue → PR を回すより、まとめて 1 つの chores PR で片す方が速い。このスキルは収集 → triage → 1 回の確認 → 一括対処 → 消化を定式化する。

## 1. 候補収集

- **inbox**: パスは SessionStart 注入(`additionalContext`)に出ているものを使う。無ければ `${XDG_STATE_HOME:-~/.local/state}/claude/wrapup/<slug>.jsonl`(slug はプロジェクト絶対パスの `/` `.` を `-` に置換したもの)。読むのは `cat` + `jq` で構わない — 禁止されているのは書き込みで、変更は `--add` / `--mark-filed` 経由に限る(この制約はこのスキルでも不変)。
- **起票済み wrapup 由来 Issue**: `gh issue list` + `jq` で列挙する。`gh search issues` は使わない(Search API のインデックス遅延があり、絵文字入りフッターの一致も不安定なため)。
  ```
  gh issue list --state open --limit 200 --json number,title,body \
    | jq '[.[] | select(.body | contains("Filed from Claude Code wrap-up inbox"))]'
  ```
  フィルタ文字列は絵文字を含まない ASCII 部分だけを使う。`--limit` は既定値(30)だと取りこぼすので明示する。

## 2. triage 基準

「即対処」と判定するのは、次を**すべて**満たす項目だけ。1 つでも外れたら「要判断」に回す:

- やり方が一意で、選択肢の提示が発生しない(ユーザーに聞きたくなった時点で要判断)
- リポジトリ内に複製できる既存パターン・参照実装がある
- 変更が局所(目安: 単一〜少数ファイル・数十行以下)
- 検証が機械的(既存の selftest / lint / ビルドチェックがそのまま合否を出す)
- 非互換な挙動変更を含まない(落ちても単純 revert で戻せる)

## 3. 一覧提示と GO 確認(1 回だけ)

出自(`inbox` / `#N`)・title・振り分け(即対処 / 要判断)・判定理由を表で提示し、1 回だけ確認を取る(GO / 一部除外して GO / 中止)。**GO 後は項目ごとの確認をしない** — それがこのスキルの存在理由であり、毎回止まるならスキル化する意味がない。

## 4. 一括対処

- 最新の default branch から作業ブランチを切る(`Closes` が効くのは default branch 向き PR だけなので、ベースブランチを必ず確認する)。
- 項目ごとに 1 commit。Issue に対応する項目は commit message に `#N` を含める。
- PR は 1 つにまとめ、リポジトリの PR 本文スケルトンに従う:
  - 対処した Issue はすべて `Closes #N` として列挙する
  - inbox 由来で対応する Issue が無い項目は、**新たに起票しない**(起票して即クローズする churn を避ける)。代わりに「課題」節に「inbox 直接消化分」として title / detail を列挙し、何を直したかを本文で監査できるようにする
  - 全項目が inbox 直接消化分のみ(Issue が 1 つも無い)の場合に限り、`No-Issue:` で理由を明記する
  - Before/After は視覚的な変更があれば証跡を、無ければそのリポジトリの規約に沿って明示する

## 5. inbox の消化タイミング

PR 作成(`gh pr create` 成功)の直後に、対処済みの inbox 行を 1 行ずつ `--mark-filed` で削除する。行は完全一致で渡す(inbox の直接編集は不変で禁止)。

- **commit 直後ではなく PR 作成直後**にする理由: セッションが PR 作成前に終了しても inbox はそのまま残り、既存の収集・起票の仕組みが自己修復的に拾い直せる。commit しかしていない段階で inbox から消してしまうと、push 前にセッションが落ちた場合に気づきそのものが失われる。
- **merge 後ではなく PR 作成直後**にする理由: PR 作成から merge までの間、対処済みの項目が inbox に残っていると、その間に走る別セッションの Stop ゲートが同じ項目を再び起票しようとする。merge を待って消すと「merge 後に inbox を消す主体」がこの機構には存在しない。PR 作成直後に消しておけば、記録は PR 本文と commit に恒久的に残るため、PR が閉じられても気づきの内容自体は失われない。

## 6. 要判断の扱い

- inbox 行はそのまま残す。次のセッションの Stop ゲートが通常どおり個別起票のフローで拾う。
- Issue はそのまま open で残す。コメントは付けない。
- 最終報告で「要判断として残した一覧」をユーザーに明示する。

## 7. 完成の定義

次をすべて満たすまで完成と呼ばない:

- PR の URL を提示済みで、base が default branch である
- 対処した項目と `Closes #N` が 1:1 対応している(inbox 直接消化分は本文の列挙で代替)
- 対処済みの inbox 行が inbox から 0 件になっている
- リポジトリ標準の検証(selftest / lint / ビルド等)が green
- 要判断として残した一覧をユーザーに報告済み

事例が積み重なったら `cases.md` を新設して追記する。サニタイズ規則は `skill-gardening` を参照。
