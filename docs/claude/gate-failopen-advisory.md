# gate-failopen-advisory — fail-open で素通りし続けている gate を、セッション開始時に知らせる

## なぜ必要か

fail-open で設計した hook は、依存先(外部 CLI・モデル・認証)が壊れても作業を止めない。その代わり、
**壊れたまま素通りし続けている**ことに気づく手段が、毎回の警告メッセージを人が読むことしか無い。

実例は `copilot-plan-review`。固定していたモデルが Copilot CLI の catalog から消えた 2026-09-29 から、
約 2 週間半、ExitPlanMode のたびに fail-open の警告を出して素通りしていた。直近のセッションの記録では
同じ警告が 166 回出ていたが、利用者が「呼ばれていない気がする」と疑うまで検出されなかった。原因の
可視化(#819)は入ったが、素通りが続いていること自体を検出する仕組みは無かった。

## 何をするか

1. **gate が事実を残す。** `crates/copilot-plan-review` が、依存先の不調でレビューを実行できずに素通りした
   とき `fail-open`、レビューが成立して通ったとき `pass` を、既存の `gate-events.jsonl`
   (`crates/hook-io/src/gate_event.rs`、`~/.local/state/claude/gate-events.jsonl`)に 1 行残す。
   `decision` の語彙は `deny`・`skip`・`fail-open`・`pass` の 4 値になった。
2. **SessionStart で数える。** `crates/gate-failopen-advisory`(matcher `startup|resume`)が、監視する gate ごとに、
   **同じ gate の最後の `pass` より後の `fail-open` の数**を、ログの末尾から数える(`trailing_fail_open`)。
   状態ファイルは持たない。`pass` が 1 回入れば連続回数は 0 に戻る。
3. **閾値以上なら伝える。** 既定 3 回以上のとき、`additionalContext` で警告する。環境変数
   `GATE_FAILOPEN_THRESHOLD` で上書きできる(数値でない・0 は既定に戻す)。警告には、復旧の手がかり
   (`copilot-plan-review` なら、実行に失敗した原因が systemMessage にあること、モデルの catalog、ログイン、
   `docs/claude/copilot-plan-review.md`)を添える。
4. それ以外(ログが無い、読めない、閾値に届かない)は何も出さない。全経路 fail-open。hook は何も実行しない。

## 記録しない素通り

`copilot` CLI が無いホストでの無音の素通り(ADR-0005)と、プラン本文を取得できなかった素通りは記録しない。
どちらも依存先の不調ではなく、設計上の素通りだから。

## 閾値 3 の根拠

`promotion-detect` の `DEMOTION_SKIP_THRESHOLD`(3)にそろえた。タイムアウトなどの一過性の失敗は
1〜2 回で収まり、ExitPlanMode は 1 日に数回なので、壊れていれば当日中に検出できる(従来は約 2 週間半)。

## 既存手段と代替

- 既存手段: 拡張 — `crates/herdr-status-advisory` の型(stdin を使わず、警告があるときだけ `additionalContext`
  を出す SessionStart hook)と、`gate_event.rs` の語彙を広げた。新しい保存先や daemon は作らない。
  `promotion-detect` は未知の `decision` を読み飛ばすので、語彙を足しても降格候補の検出は変わらない。
- 対抗馬: bash と jq の SessionStart スクリプト(jsonl は jq で足りる)。ADR-0024 の既定は新規の hook に
  Rust で、型付きの読み取りと `cargo test` の負例を持てる。`promotion-detect` への組み込みは、日次の集計で
  次の SessionStart に警告が出る受け入れ条件を満たさない。
- 先行例: Martin Fowler, "CircuitBreaker"(2014-03-06、https://martinfowler.com/bliki/CircuitBreaker.html)は、
  失敗を数え、成功でゼロに戻し、閾値で trip する。本案は trip(遮断)せず、fail-open の設計を保つため警告だけ。
- 軸: 表現不可能 — 現行の語彙は成功を記録しないので「連続」を表現できない。語彙を足し、ログの末尾から導く。

## 他の fail-open の hook に使う

`crates/gate-failopen-advisory/src/main.rs` の `GATES` に 1 行(gate 名と復旧の手がかり)を足し、その gate が
`gate_event::record` で `fail-open` / `pass` を残すようにする。gate 名は、gate が `gate-events.jsonl` に書く
名前と同じにする。

## 登録

- `home/modules/claude.nix`: SessionStart hook(Claude)、`home.file` の配備、Codex の SessionStart 登録。
- `pipeline-map.toml`: ノード `gate-failopen-advisory` と、`copilot-plan-review` からのデータの流れ。
- 試験: `crates/gate-failopen-advisory/tests/cli.rs` が、一時の `XDG_STATE_HOME` に置いた `gate-events.jsonl` で
  実バイナリを動かす(無い・閾値未満は無出力、閾値で警告、`pass` でリセット、別の gate は数えない、閾値の上書き)。
  `crates/copilot-plan-review/tests/hook.rs` の `gate_events_record_fail_open_then_pass` が、偽の copilot で
  `fail-open` と `pass` が記録されることを確かめる(テストの `XDG_STATE_HOME` は一時の HOME に閉じる)。
