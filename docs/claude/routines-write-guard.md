# routines-write-guard — 宣言外の cron routine 作成/更新を deny する

`crates/routines-write-guard`(Rust、ADR-0024)の設計根拠
(ADR-519 D7)。

## 動機

ADR-519 は、Claude Code routine(claude.ai の
scheduled cloud agent)の設定・prompt の正本を対象リポジトリ自身の
`.claude/routines/*.{json,md}` に一本化した。しかし Claude セッションが
`RemoteTrigger`(action: create/update)を直接呼べば、この宣言を経ずに
cron routine を作れてしまう — `/schedule` でのその場限りの作成や、宣言に
取り込む前の手直しが、静かに宣言外の状態を生む。

## 仕組み

PreToolUse(matcher: `RemoteTrigger`)専用。`RemoteTrigger` は Bash ツール
ではないため、`rulesets-write-guard` が使う「コマンド文字列先頭の
`BYPASS=` env var 代入を読む」方式は成立しない — 1 回のツール呼び出しに
コマンド文字列という概念が無い。代わりに、正規の経路
(`config/claude/skills/claude-routines/scripts/routines-plan.sh` の
`build-body`/`classify` が出す body)が必ず満たす構造そのものを通過条件に
する:

- 対象は `action` が `create`/`update` かつ、body に非空の
  `cron_expression` を含む呼び出しだけ。run-once/webhook の create、
  read 系アクション(list/get/run/list_runs/get_run_log)は判定しない。
- `body.name` が `<owner>/<repo>:<routine-name>` の名前空間キー
  (routine-name は小文字英数字とハイフンのみ)でなければ deny。
- `body.job_config.ccr.events[0].data.message.content` の最終行が
  `routine-spec: <64桁の小文字16進数>` でなければ deny。
- **bypass は無い**。`RemoteTrigger` 呼び出し全体を対象にコマンド文字列
  由来の判定はできないため、意図的な手動書き込みは Web UI 経由で行う想定
  (`docs/claude/claude-routines.md` の「実測」節、meta connector にも
  ローカルの `RemoteTrigger` ツールにも delete は無い/限定的、という制約と
  同じ理由で、この guard も「間違って踏んだときに止める」ことを目的にし、
  意図的な迂回の完全阻止は狙わない)。

クラウドの meta connector(`Claude_Code_Remote` MCP)経由の書き込みは
この hook からは見えない(対象が Claude Code のローカルツールに限られる、
decision-colocation の Codex/Copilot adapter 不要判断〔ADR-396 D8〕と同じ
理由)——そちらは `auditor.md` の unmanaged 検出が事後に拾う。

## 先行例との差分

- **`rulesets-write-guard`**: 同じ「deny のみ返す・判定できない入力は
  素通し」の設計を踏襲するが、対象ツールが Bash ではないため
  `hook_io::shell::split` も bypass env var も使わない。判定条件を
  「コマンド文字列の字句解析」から「body の構造検査」に置き換えた。
- **実装言語**: ADR-0024 は新規 hook を既定で Rust とする。この hook は
  JSON の構造検査だけで完結し、既存の巨大 bash 資産を source する必要が
  無いため、既定どおり Rust で書いた。
