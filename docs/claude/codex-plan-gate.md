# codex-plan-gate — Codex CLI の Plan mode に ExitPlanMode 相当の機械検査を課す Stop hook

判定エンジン: `crates/plan-scope-gate` / `plan-precedent-gate`(再利用、新規ロジックなし)
adapter: `config/codex/hooks/codex-plan-gate.sh`
決定: `docs/adr/0032-global-agent-instructions-canon.md` の Amendment(#531)
Claude 側の対: `crates/plan-scope-gate` / `plan-precedent-gate`(ExitPlanMode の PreToolUse hook)
無限 block 対策の型: `config/claude/hooks/pr-gate.sh`(同じ escalate カウンタ設計)

## なぜ必要だったか

ADR-0032 の Decision 4 は「Plan の `## 先行例との対比` 節の書式、
`plan-precedent-gate` / `plan-scope-gate` の自己検査手順は Claude
Code の Plan Mode / hook に固有の運用のため、Codex/Copilot にこれらの
adapter は存在しない」と明示的に据え置いていた。

Claude 高額利用者に「1週間 Claude 使用禁止 + Codex CLI 代替」を割り当てる
社内検証(#531)への備えで、この据え置きの一部を解除する必要が生じた。
Codex 週の間 Plan の要求インベントリ・先行例接地チェックが完全に指示文
頼りになると、Claude 側で機械 gate が担っている規律が抜け、検証結果が
「Codex の実力」ではなく「規律の有無」の差になってしまう。

## Codex に ExitPlanMode という執行点は無い

Claude Code の Plan mode は `ExitPlanMode` という明示的なツールコールで
終わり、そこに PreToolUse hook(`plan-scope-gate` 等)を挟める。Codex
CLI の Plan mode(TUI バイナリの文字列リテラルで確認: 「Plan Mode
(Conversational)」)にはこれに相当するツールコールが無く、モデルは応答
本文に `<proposed_plan>...</proposed_plan>` ブロックを埋め込むだけで
Plan を提案する。ユーザーが Plan mode を抜けて実装を求めるか、Plan mode
に留まって Plan を練り直すかを選ぶ、という UI 上の分岐点であって、
hook を挟める「ツールコール直前」という地点が存在しない。

Codex の hook イベントのうち、この提案が確定した直後に必ず通る地点は
**Stop**(そのターンの完了)だけである。よって codex-plan-gate は Stop
hook として実装し、直前の応答(`last_assistant_message`)から
`<proposed_plan>` ブロックを検出したときだけ発火する。

## 判定は 1 つも増やさない

`codex-plan-gate.sh` は `<proposed_plan>` ブロックの中身を一時ファイルに
書き出し、既存の

```
plan-scope-gate --check-plan <file>
plan-precedent-gate --check <file>
```

をそのまま呼ぶだけで、新しい判定ロジックは一切持たない。両方が exit 0
(「OK: ...」)なら無出力で通す。どちらかが非 0(指摘あり)なら、両方の
出力を連結して `{"decision":"block","reason":"..."}` を返す。

`plan-fresh-gate`(worktree の drift 検査)は今回の対象外 — 入力が
プラン参照ファイルの一覧など Claude Code の worktree 運用に依存する部分が
大きく、SessionStart の `worktree-fresh-base.sh`(Codex にも段2で展開済み)
がカバーする範囲で足りると判断した。`plan-view`(Chrome 表示)・
`copilot-plan-review`(Copilot 批評)は元々ブラウザ操作/別 CLI 呼び出し
を伴い、Codex 週の premium request/ブラウザ操作を増やさないため対象外の
まま(ADR-0032 Amendment 参照)。

## 無限 block 対策

`pr-gate.sh`(`docs/claude/pr-gate.md`)と同じ設計を踏襲する:
`stop_hook_active` は見ず、`session_id` ごとの独自カウンタ
(`~/.codex/codex-plan-gate/state/<sid>.count`)が
`${CODEX_PLAN_GATE_MAX_BLOCKS:-4}` に達したら 1 回だけ
`<sid>.escalated` を touch し、以後そのセッションは無条件で通す。

`stop_hook_active` を見て即座に素通す設計(`wrapup-stop-gate.sh` 型)を
採らない理由も pr-gate.sh と同じ: block した直後の再呼び出しでも判定に
到達させたい(素通しにすると「block → 続行 → 素通り」で1回も再検査され
ない)。

## エスケープハッチ

`touch ~/.codex/codex-plan-gate/skip` または `SKIP_CODEX_PLAN_GATE=1`
(`stack-base-guard.sh` と同型)。

## 縮退

ADR-0005 の binary-existence gating に倣い、次はすべて黙って exit 0(判定
不能を deny に変えない):
`jq` 不在 / `plan-scope-gate`・`plan-precedent-gate` が実行可能でない /
`hook_event_name != "Stop"` / `last_assistant_message` が空 /
`<proposed_plan>` タグが無い。

## 既知の限界

- `last_assistant_message` に `<proposed_plan>` タグが verbatim で残るかは
  Codex の公式ドキュメントに明記が無い(2026-09-28 時点)。段4の「要確認」
  節にある実機疎通確認(`codex exec`)で確認する。タグが失われる場合、この
  hook は静かに no-op になる(縮退と同じ挙動 — 誤って block する側には
  倒れない)。
- 1 ターンに `<proposed_plan>` が複数回出ることは Codex の設計上想定され
  ていない(TUI 文字列リテラル: 新しい `<proposed_plan>` は「complete
  replacement」)ため、`extract_plan()` は最初の1ブロックのみを対象にする。
