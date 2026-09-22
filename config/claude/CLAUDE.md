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
`~/.claude/hooks/plan-precedent-gate.sh --check <プランファイル>` で
自己検査して指摘ゼロを確認する**(gate の deny 往復を待たない)。

## セッション内 PR チェーンの形式検査(ADR-0027)

stacked PR に積む原則そのものは共有 AGENTS.md に従う。形式検査は作成時
`stack-base-guard.sh`(PreToolUse deny)と完了時 `pr-gate.sh` の `G_stack`
(Stop block)が担う。gate に当たる前に自発的に積むこと — gate は漏れを
拾うためのもので、一次的な手段ではない。

## 要求インベントリの書式と自己検査

計画冒頭に `## 要求インベントリ` を置き、依頼文と参照 Issue の子項目を
逐語で 1 行 1 項目・`R1..Rn` の ID 付きで列挙してから設計に入る。列挙は
本線で行う(Plan/Explore サブエージェントは CLAUDE.md を読み飛ばす)。
ExitPlanMode を呼ぶ前に `~/.claude/hooks/plan-scope-gate.sh --check-plan
<プランファイル>` で節内整合性(処分・タグ)を自己検査する。実装中に
見つかった隣接負債は AskUserQuestion で stack か wrap-up inbox かを聞く。
手順は scope-inventory スキルに従う。

## 実装タスクの完了定義の形式検査

commit → push → `gh pr create` を、途中で確認を挟まず一続きで実行する。
「PR を作成しますか?」と聞かない。Stop hook(`G_pr` in pr-gate.sh)がこの
漏れを検査する。

## GitHub 投稿の生成元明示(Claude Code のフッター文言)

共有 AGENTS.md の規範における「自分自身のエージェント名と URL」は、
Claude Code では固定でこの 1 行:

```
🤖 Generated with [Claude Code](https://claude.com/claude-code)
```

形式検査は attribution-guard.sh が PreToolUse で行う。gate に当たる前に
自発的に付けること — gate は漏れを拾うためのもので、一次的な手段ではない。
