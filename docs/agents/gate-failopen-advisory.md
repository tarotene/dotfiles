# gate-failopen-advisory — fail-open で素通りし続けている gate をセッション開始時に通知する

## なぜ必要か

fail-open で設計した hook は、依存先(外部 CLI・モデル・認証)が壊れても作業を止めない。その代わり、
**壊れたまま素通りし続けている**ことに気づく手段が、毎回の警告メッセージを人が読む以外になかった。

実例は `copilot-plan-review`。固定していたモデルが Copilot CLI の catalog から消えた 2026-09-29 から、
約 2 週間半、ExitPlanMode のたびに fail-open の警告を出して素通りしていた。直近のセッションの記録では
同じ警告が 166 回出ていたが、利用者が「呼ばれていない気がする」と疑うまで検出されなかった。原因の
可視化(#819)は入ったが、素通りが続いていること自体を検出する仕組みはなかった。

## 何をするか

1. **gate が実行結果を記録する。** `crates/copilot-plan-review` が、依存先の不調でレビューを実行できずに素通りした
   とき `fail-open`、レビューが成立して通ったとき `pass` を、既存の `gate-events.jsonl`
   (`crates/agent-session/src/gate_event.rs`、`~/.local/state/claude/gate-events.jsonl`)に 1 行記録する。
   `decision` の語彙は `deny`・`skip`・`fail-open`・`pass` の 4 値になった。
2. **SessionStart で数える。** `crates/gate-failopen-advisory`(matcher `startup|resume`)が、監視する gate ごとに、
   **同じ gate の最後の `pass` より後に続く `fail-open` の回数**をログの末尾から数える(`trailing_fail_open`)。
   状態ファイルは持たない。`pass` が 1 回入れば連続回数は 0 に戻る。
3. **閾値以上なら伝える。** 既定の 3 回以上に達したとき、`additionalContext` で警告する。環境変数
   `GATE_FAILOPEN_THRESHOLD` で上書きできる(数値でない・0 は既定に戻す)。警告には、復旧の手がかり
   (`copilot-plan-review` なら、実行に失敗した原因が systemMessage にあること、モデルの catalog、ログイン、
   `docs/agents/copilot-plan-review.md`)を添える。
4. それ以外(ログが存在しない、読み取れない、閾値未満)は何も出力しない。全経路が fail-open であり、hook 自体は修復処理を実行しない。

## 記録しない素通り

`copilot` CLI がないホストでの無音の素通り(ADR-0005)と、プラン本文を取得できなかった素通りは記録しない。
どちらも依存先の不調ではなく、設計上の素通りであるため。

## 閾値 3 の根拠

`promotion-detect` の `DEMOTION_SKIP_THRESHOLD`(3)にそろえた。タイムアウトなどの一過性の失敗は
1〜2 回で収まり、ExitPlanMode は 1 日に数回なので、壊れていれば当日中に検出できる(従来は約 2 週間半)。

## 既存手段と代替

- 既存手段: 拡張 — `crates/herdr-status-advisory` の型(stdin を使わず、警告があるときだけ `additionalContext`
  を出す SessionStart hook)と、`gate_event.rs` の語彙を拡張した。新しい保存先や daemon は作らない。
  `promotion-detect` は未知の `decision` を読み飛ばすので、語彙を追加しても降格候補の検出は変わらない。
- 対抗馬: bash と jq の SessionStart スクリプト(jsonl は jq で足りる)。ADR-0024 は新規 hook の実装言語を
  Rust としており、型付きの読み取りと `cargo test` による負例テストを持てる。`promotion-detect` への組み込みは、
  日次の集計であり次の SessionStart で警告を出す受け入れ条件を満たさない。
- 先行例: Martin Fowler, "CircuitBreaker"(2014-03-06、https://martinfowler.com/bliki/CircuitBreaker.html)は、
  失敗を数え、成功でゼロに戻し、閾値で trip する。本案は trip(遮断)せず、fail-open の設計を保つため警告のみに留める。
- 軸: 表現不可能 — 現行の語彙は成功を記録しないので「連続」を表現できない。語彙を追加し、ログの末尾から連続回数を導出する。

## ja-polish の課金検討(割合による通知)

連続回数とは別に、`ja-polish` については、**直近 7 日の素通しのうち、混雑・時間切れによるものの割合**でも通知する
(ADR-1387 の D14、`billing_line`)。無料枠が混雑して素通しが増えたときに課金の検討を促すため。

- 参照する記録は `~/.local/state/claude/ja-polish/records.jsonl`。分子は `"transient":true` の素通しのみで、キー
  未設定や照合失敗は分母にのみ含める。期間内の記録が 20 件未満のときは何も出力しない。
- 割合が 5% を超えたら、無料枠の扱い(推論)・有料枠の違い・月の目安(推定)・判断の材料(ADR-1387)を 1 行で出力する。
  閾値は環境変数 `JA_POLISH_TRANSIENT_ADVISORY_PERCENT` で変えられる。
- `ja-polish` の設定ファイル(`~/.config/dotfiles/ja-polish.toml`)がない機体では何も出力しない。

<<<<<<< HEAD
## ja-polish の予備のモデルが答えた割合(質の劣化を知らせる)

課金の検討の知らせは素通しの率しか見ない。無料枠の 1 日の上限のあとは、素通しにならずに予備の軽量モデルへ流れ、
変更なしで返した段落も「通した」と記録されるので、そこには映らない(2026-11-27、直近 7 日の書き直しの 89% が
予備の答え)。そこで、**直近 7 日に書き直した段落のうち、主のモデルでなく予備のモデルが答えた割合**でも知らせる
(ADR-1387 の D17、`fallback_line`)。

- 分母は `"state":"polished"` の記録。記録の `model` が、設定の主のモデルと違えば、予備が答えたと数える。期間内の
  書き直しが 20 件に満たないときは何も言わない。
- 割合が 10% を超えたら、予備の割合と、変更の有無(`changed`)を記録した段落のうち変えずに返した割合を 1 行で出す。
  閾値は環境変数 `JA_POLISH_FALLBACK_ADVISORY_PERCENT` で変えられる。
- `changed` がない古い記録は、変更なしの割合の分母に入らない。

## 他の fail-open hook への適用

`crates/gate-failopen-advisory/src/main.rs` の `GATES` に 1 行(gate 名と復旧の手がかり)を追加し、その gate が
`gate_event::record` で `fail-open` / `pass` を記録するようにする。gate 名は `gate-events.jsonl` に書き込む
名前と一致させる。

## 登録

- `home/modules/agents.nix`: SessionStart hook(Claude)、`home.file` の配備、Codex の SessionStart 登録。
- `pipeline-map.toml`: ノード `gate-failopen-advisory` と、`copilot-plan-review` からのデータの流れ。
- テスト: `crates/gate-failopen-advisory/tests/cli.rs` が、一時的な `XDG_STATE_HOME` に配置した `gate-events.jsonl` で
  実バイナリを動かす(ログがない・閾値未満では無出力、閾値で警告、`pass` でリセット、別の gate は数えない、閾値の上書き)。
  `crates/copilot-plan-review/tests/hook.rs` の `gate_events_record_fail_open_then_pass` が、偽の copilot で
  `fail-open` と `pass` が記録されることを確かめる(テストの `XDG_STATE_HOME` は一時的な HOME に隔離する)。
