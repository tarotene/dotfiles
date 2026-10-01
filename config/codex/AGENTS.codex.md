## Codex CLI 固有の施行配線(ADR-0032 Amendment #531)

以下は Codex CLI に固有の運用で、共有 AGENTS.md の規範を Codex の hook /
UI 概念に合わせて具体化したもの。Claude Code の `~/.claude/CLAUDE.md` に
ある同種の節と対をなす(Codex には ExitPlanMode という UI 概念が無いため、
文言は Codex の Plan mode(`<proposed_plan>` ブロック)向けに書き換えている)。

### Plan を出す前の自己検査

Codex の Plan mode で応答に `<proposed_plan>` ブロックを含めるときは、
非自明な設計判断(機構の選び方・配置・分割・命名・プロトコル・アルゴリズム)
ごとに `## 先行例との対比` 節を置き、判断ごとに `Dn:` 行(採った判断 / 出典と
取得日 / 先行例との差分 / `軸:`)を書く。設計判断を含まない Plan は代わりに
1 行の免除(`先行例: 該当なし — <理由>`)でよい。書式は precedent-grounding /
selection-grounding スキル(`~/.agents/skills/`)に従う。

複数項目を含む依頼には `## 要求インベントリ` 節を置き、逐語で `Rn` の ID を
振って処分(実装する段、または閉じたタグ)を書く(scope-inventory スキル)。

`<proposed_plan>` を出す前に、次の2本を自分で実行して指摘ゼロを確認する
(Stop hook `codex-plan-gate.sh` の block を待たない — gate は漏れを拾う
ためのもので、一次的な手段ではない):

```
~/.claude/hooks/plan-precedent-gate --check <このターンで書いたPlan本文のファイル>
~/.claude/hooks/plan-scope-gate --check-plan <同上>
```

### PR 運用(セッション内チェーン・完了定義)

同一セッション・同一 worktree で複数の PR を作るときは、常に作成順の単一
チェーン(stacked PR)に積む(ADR-0027)。作成時 `stack-base-guard.sh`
(PreToolUse deny)と完了時 `pr-gate.sh` の `G_stack`(Stop block)が
Codex にも効く。

コード変更を伴うタスクは commit → push → `gh pr create` を、途中で確認を
挟まず一続きで実行する。「PR を作成しますか?」と聞かない。Stop hook
(`pr-gate.sh` の `G_pr`)がこの漏れを検査する。

決定成果物(ADR・設計文書・skill)の執行点を同じ PR に出す原則(ADR-396)
は Codex にも適用されるが、その形式検査は CI required check
(`scripts/decision-colocation-check`)のみが担う(PreToolUse deny の
`decision-colocation-guard.sh` は Codex には移植していない — 対象範囲は
ADR-0032 Amendment 参照)。gate ではなく CI が拾うため、push 後に気づく
点に留意する。

PR 本文に未チェックの task list を残さず、人の確認が要る残作業は後続
Issue へ払い出す原則も Codex に適用される。形式検査は `pr-confirm-guard.sh`
の Codex adapter(`config/codex/hooks/pr-confirm-guard.sh`、PreToolUse deny、
全リポジトリで発火)が担う。

### GitHub 投稿の生成元明示

共有 AGENTS.md の規範における「自分自身のエージェント名と URL」は、
Codex CLI では固定でこの1行:

```
🤖 Generated with [Codex CLI](https://learn.chatgpt.com/docs/codex/cli)
```

形式検査は `attribution-guard --agent codex`(PreToolUse、`crates/attribution-guard`)が行う。
gate に当たる前に自発的に付けること。
