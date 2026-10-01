@~/.agents/AGENTS.md

<!-- Claude Code 固有の施行配線。原則そのものは上記 import 先(共有
     AGENTS.md、Codex/Copilot とも共有)を正本とする。ここに残るのは
     Claude Code のツール(ExitPlanMode・AskUserQuestion・PreToolUse/Stop
     hook)に紐づく書式・自己検査手順だけ。 -->

## 先行例確認: Plan mode での書式と自己検査

Plan mode で非自明な設計判断を書くときは、Plan に `## 先行例との対比` 節を
置き、判断ごとに 1 行(`Dn:` 採った判断 / 出典と取得日 / 先行例との差分、
または `先行例なし:` と探した範囲)で書く。設計判断を含まない Plan は代わりに
1 行の免除(`先行例: 該当なし — <理由>`)でよい。書式は precedent-grounding
スキルに従い、**ExitPlanMode を呼ぶ前に
`~/.claude/hooks/plan-precedent-gate --check <プランファイル>` で
自己検査して指摘ゼロを確認する**(gate の deny 往復を待たない)。

各 `Dn` には、上記に加えて `軸:`(`表現不可能` / `還元` / `検出のみ` の
いずれか、共有 AGENTS.md「技術・仕組みの選択は表現不可能性 → 還元性 →
先進性の順で決める」節に対応)を書く。技術・仕組みの選択を含む `Dn` で、
外部依存の新設・置換・撤去、または撤収コストが導入コストを上回るときは
`本命:`/`対抗馬:`/`外した候補:` も書く。書式は selection-grounding
スキルに従い、自己検査は同じ `plan-precedent-gate --check` コマンド
1 本でまとめて行う(新しい gate は呼ばない)。

## フィードバックの Issue 化(auto memory 固有の配線)

「ユーザーからのフィードバックは不可視なローカルメモに閉じ込めない」原則は
共有 AGENTS.md に従う。Claude Code の auto memory(`~/.claude/projects/*/
memory/*.md`、`metadata.type: feedback`)にこの種のフィードバックを保存
するときは、Issue 化するまでの一時メモに限定し、Issue 化したら本文に
その Issue 番号(`#N`)を書く(Issue へのポインタで足り、内容を重複させない)。
形式検査は `wrapup-stop-gate`(Stop hook、wrap-up inbox と同じ経路)が
担う — 今セッション中に更新された `type: feedback` メモリで `#N` 参照が
無いものを検出して促す。gate に当たる前に自発的に Issue 化すること。

## セッション内 PR チェーンの形式検査(ADR-0027)

stacked PR に積む原則そのものは共有 AGENTS.md に従う。形式検査は作成時
`stack-base-guard.sh`(PreToolUse deny)と完了時 `pr-gate.sh` の `G_stack`
(Stop block)が担う。gate に当たる前に自発的に積むこと — gate は漏れを
拾うためのもので、一次的な手段ではない。

## 決定成果物の執行点の形式検査(ADR-396)

決定成果物と執行点を同じ PR に出す原則そのものは共有 AGENTS.md に従う。
形式検査は作成時 `decision-colocation-guard.sh`(PreToolUse deny)と CI
required check(`scripts/decision-colocation-check`、単一ソース)が担う。
gate に当たる前に自発的に執行点を含めること — gate は漏れを拾うためのもので、
一次的な手段ではない。**「実装を後続 Issue に分離する」という選択肢は
実行不能なので、`AskUserQuestion` の選択肢に出さない。**

## 要求インベントリの書式と自己検査

計画冒頭に `## 要求インベントリ` を置き、依頼文と参照 Issue の子項目を
逐語で 1 行 1 項目・`R1..Rn` の ID 付きで列挙してから設計に入る。列挙は
本線で行う(Plan/Explore サブエージェントは CLAUDE.md を読み飛ばす)。
ExitPlanMode を呼ぶ前に `~/.claude/hooks/plan-scope-gate --check-plan
<プランファイル>` で節内整合性(処分・タグ)を自己検査する。実装中に
見つかった隣接負債は AskUserQuestion で stack か wrap-up inbox かを聞く。
手順は scope-inventory スキルに従う。

## 実装タスクの完了定義の形式検査

commit → push → `gh pr create` を、途中で確認を挟まず一続きで実行する。
「PR を作成しますか?」と聞かない。`gh` の投稿は本文を Write でファイルにして
から `-R OWNER/REPO … --body-file <絶対パス>` で渡す(bleep の正準形。
`pr-description` スキル §0)。Stop hook(`G_pr` in pr-gate.sh)がこの
漏れを検査する。

PR 本文に未チェックの task list を残さない・人の確認を後続 Issue に払い
出す原則の形式検査は `pr-confirm-guard.sh`(PreToolUse deny、全リポジト
リで発火)が担う。gate に当たる前に自発的に Issue 化すること — gate は
漏れを拾うためのもので、一次的な手段ではない。

## 既存手段の前倒し接地と決定論への昇格(ADR-543)

共有 AGENTS.md「道具を新設する前に既存手段を問い、決定論化は段階で
昇格させる」の原則は共有 AGENTS.md に従う。Claude Code での書式・形式
検査の配線:

- `既存手段:` 行は `## 先行例との対比` 節の重い欄(`本命:`/`対抗馬:`)を
  持つ `Dn` に必須(precedent-grounding / selection-grounding スキル
  参照)。自己検査は同じ `plan-precedent-gate --check` コマンド
  1 本でまとめて行う(新しい gate は呼ばない)。
- 新しい道具・単位(shebang 付き新規ファイル、`bin/scripts/hooks/cmd`
  配下の新規ファイル、パッケージマニフェストの新設)を Write する前
  倒しの機械強制、PR 本文での `既存手段:` 照合、決定論化の昇格・降格
  候補の検出は後続の段で実装する(ADR-543 参照)。実装済みの範囲は
  ADR-543 の `## 執行点` を参照。

## GitHub 投稿の生成元明示(Claude Code のフッター文言)

共有 AGENTS.md の規範における「自分自身のエージェント名と URL」は、
Claude Code では固定でこの 1 行:

```
🤖 Generated with [Claude Code](https://claude.com/claude-code)
```

形式検査は attribution-guard.sh が PreToolUse で行う。gate に当たる前に
自発的に付けること — gate は漏れを拾うためのもので、一次的な手段ではない。
