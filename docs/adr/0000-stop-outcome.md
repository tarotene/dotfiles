# ADR-0000 — Stop hook の結果を `StopOutcome` と `Reentry` の型で持ち、描画を `Reply::render` の 1 か所に集める

- Status: Accepted
- Date: 2026-11-09
- Issue: #1436(親の tracking Issue は #1433)
- Related: ADR-625(Stop の出力の経路の表)、ADR-872(ルータの条件)、ADR-0024 Amendment 2(hook の共通入出力)

## Context

Stop hook を出す bin は 5 本あり、出力が 2 流儀に割れていた。

| 流儀 | bin | 運び方 |
|---|---|---|
| 終了コード 2 と標準エラー | `pr-gate`、`session-wrapup`(互換 bin の `wrapup-stop-gate` を含む) | 理由を標準エラーに書き、2 で終わる |
| 標準出力の JSON | `main-checkout-guard`、`ja-polish`、`codex-plan-gate` | `{"decision": "block", "reason": …}` を手で組み立てる(compact と整形が混在) |

`stop_hook_active`(直前の block で続けさせられたターンの再呼び出し)の扱いも、3 通りに割れていた。見たら黙る(`main-checkout-guard`、`ja-polish`、`issue-inbox` の判定部)、見ない(`pr-gate`、`codex-plan-gate`、振り返り)、1 つの入口の中で混在(`session-wrapup`)。`pr-gate` の「見ない」は設計である。見ると、未 push の block の直後の再呼び出しが CI の判定に一度も届かない(`docs/agents/pr-gate.md`)。ただし、その設計は散文の中にあり、型には無かった。

ブロックしない失敗(入力が読めない = 終了コード 5、移行の失敗 = 1)は、block と同じ整数の戻り値に混ざっていた。終了コード 2 は Claude Code で「止める」の意味なので、失敗の番号を取り違えると、失敗が block に化ける。

`docs/agents/hook-router.md` は、Stop をルータに載せない理由を 4 つの条件で書き、さらに「Stop を載せるなら、`Outcome` に Stop の結果を型で足し、2 流儀をそろえる必要がある」と条件を残していた。型と描画の統一は、載せる載せないとは別に価値がある(同じ事実の置き場が 1 つになる)。

Stop の出力の経路の記録: ADR-625 の表(Anthropic, "Hooks reference"、<https://code.claude.com/docs/en/hooks>、2026-09-30 取得)は、Stop の終了コード 2 の標準エラーも、`decision: "block"` と `reason` も、Claude に届き、利用者に見える(前者は hook error として)と記録している。この ADR は、その記録を輸送の根拠にする。上流の文書の再確認と、利用者の画面での表示の確認は、実機確認(#1469)に残す(未確認)。

## Decision

### D1: Stop の結果は `StopOutcome`(別の型)で表す

`Block { reason, notice }`(続けさせる。`notice` は利用者向けの `systemMessage`)と `Notice`(止めない注意書き)。沈黙は `Option` の `None`。ルータの `Outcome` に variant を足さない。ルータの併合は `Outcome` の網羅 match なので、足すと「Stop の規則を表に載せない」が型でなく取り決めになる。別の型にすると、載せられないことが型で守られる。

### D2: 再入の扱いを `Reentry` の値にする

`Quiet`(立っていたら検査せず黙る)と `Recheck`(見ずに検査する。block の直後の再呼び出しでも判定する)。検査ごとに 1 つ宣言し、`stop_hook_active` を読むのは `StopInput` だけにする。`pr-gate` の「見ない」設計は、`Recheck` として残る。無限に block しないための上限(`pr-gate`・`codex-plan-gate`・振り返りの 3 つのカウンタ)は、検査自身が持つ。3 つは、上限到達時の振る舞いが違う(`pr-gate` は escalate の文言で block、`codex-plan-gate` は形式検査の拒否だけを止める、振り返りは注記して黙る)ので統合しない。`Recheck` の検査が上限を持つことは、型では強制できない(検出のみ)。

### D3: 描画は `Reply::render` の 1 か所

Claude と Codex の block は、最上位の `decision: "block"` と `reason`(`hookSpecificOutput` を使わない)で、`systemMessage` を末尾に添える。Copilot の Stop は何も出さない(出力元が無く、形は未確認)。

### D4: block は終了コード 0 の JSON で運ぶ

終了コード 2 と標準エラーを残す設計は、「描画は 1 か所」に反し、`systemMessage` を運べない(`main-checkout-guard` の注意書き、#898)。ブロックしない失敗は `Fault`(`FAILED` = 1、`MALFORMED_INPUT` = 5)で、0 と 2 を作れない。Stop の標準出力は JSON だけにするため、検査が起動する子プロセスの標準出力は継承しない。

### D5: Stop はルータに載せない

`pr-gate` が CI を最大 300 秒待ち、ルータの予算に収まらない(`hook-router.md` の条件 3)。載せない理由は予算で、型の有無ではない。

### D6: 検査

`crates/repo-structure-check` の `hook_io` が、Stop の block(`"decision"` と `"block"`)を手で書いた行と、`stop_hook_active` を読む行を、`agent-session` の外で落とす。例外は無い。

## 先行例との対比

- D1〜D3: 先行例は `docs/adr/0024-hook-cli-scripts-target-rust.md` の Amendment 2(`permissionDecision` の出力が 10 個の独立実装だった問題を、共通入出力で解いた。取得 2026-11-09)。差分は、一致。同じ方向を Stop に広げる。軸は表現不可能(型 > 実行時、単一正本 > 複写+同期)。
- D4: 先行例は ADR-625 の表(上の Context。取得 2026-11-09)。差分は、一致。軸は表現不可能(終了コードを型で作れなくする)。
- D6: 先行例は `docs/agents/repo-structure-check.md` の `hook_io` の検査(取得 2026-11-09)。差分は、一致。軸は検出のみ(文字列の直書きは型にできない)。

## Consequences

- block が、終了コード 2 と標準エラーから、終了コード 0 の標準出力の JSON になる(`pr-gate`、`session-wrapup`)。モデルへ渡る文は同じ(標準エラーの文字列と JSON の `reason` は同一)。利用者への表示のラベルが変わりうる(未確認。#1469)。
- 互換 bin `wrapup-stop-gate` は、登録されていないが、テストが旧い観測形(終了コード 2)を要求していた。ハーネスに、block の JSON を旧い観測形に読み替える層を置いて吸収した。互換 bin の削除(段14)で、この層も不要になる。
- `stop_hook_active` の真偽は、厳密な bool だけを真とする(従来の jq 互換は文字列 `"true"` も真にした。実際の入力は bool)。
- Copilot の Stop は、出力元も実測の記録も無いので、何も出さない。必要になれば、`Reply::render` に 1 つの腕を足す。

## Alternatives considered

- `Outcome` に Stop の variant を足す: ルータの網羅 match に Stop が入り、載せられないことが型で守られなくなる。採らない。
- 終了コード 2 と標準エラーに統一する: `systemMessage` が運べず、`Fault` が 2 を作れないという性質とも両立しない。採らない。
- 3 つの上限カウンタの統合: 上限到達時の振る舞いが変わる。この ADR の範囲(結果の型と描画)の外。
- 互換 bin だけ旧流儀を残す: 「描画は 1 か所」と矛盾する。ハーネスの読み替えで吸収した。

## 執行点

- crates/agent-session/src/outcome.rs
- crates/agent-session/src/stop.rs
- crates/agent-session/src/event.rs
- crates/agent-session/src/reply.rs
- crates/agent-session/src/entry.rs
- crates/repo-structure-check/src/hook_io.rs
- crates/main-checkout-guard/src/main.rs
- crates/ja-polish/src/stop.rs
- crates/codex-plan-gate/src/main.rs
- crates/pr-gate/src/stop.rs
- crates/pr-gate/src/state.rs
- crates/session-wrapup/src/lib.rs
- crates/issue-inbox/src/lib.rs
