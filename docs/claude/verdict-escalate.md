# verdict-escalate — 判定レッジャーの deny/ask を集約し、人間の GO 後に開発元へフィードバックする

設計と根拠: [ADR-478](../adr/478-agent-verdict-ledger.md)

## 動機

`bleep`(tarotene/bleep)のような自作ガードレールに、エージェントが同じ
判定へ何度も弾かれ続けても、その事実を開発元(自分自身)にフィードバック
する経路がこれまで無かった。Claude Code の transcript を実測すると、
同一セッションで 3 回以上同じ判定に弾かれるケースが複数あり、deny 理由文
が「何がマッチしたか」を示さないため、エージェントは bypass 環境変数を
試したり scanner を手で叩いてトークン総当たりしたりする「滑り」を起こして
いた。

判定を下したツール自身(bleep)がローカルの判定レッジャー
(`agent-verdicts/<tool>.jsonl`)に記録するようになった(tarotene/bleep
側の変更)。`verdict-escalate` はこのレッジャーを読み、セッション単位で
集約して wrap-up inbox に流す側を担う。

## 仕組み

- `crates/verdict-escalate` (Rust)。`config/claude/hooks/wrapup-stop-gate.sh`
  の Stop 本体が、inbox を読む**前に**逐次呼ぶ(`~/.claude/hooks/verdict-escalate`
  を絶対パスで実行、`WRAPUP_VERDICT_ESCALATE_BIN` で上書き可能)。**hook として
  register はしない** — 判定(allow/deny/ask)を一切返さないため、他の hook と
  同列に並列実行させると inbox 読み取りとの順序が非決定になる。
- stdin で Stop の入力 JSON をそのまま受け取り、`session_id` を読む
  (`hook_io::input::HookInput`、Claude/Codex/Copilot 共通)。
- `$AGENT_VERDICTS_DIR`(既定 `$XDG_STATE_HOME/agent-verdicts`)配下の
  `*.jsonl` すべてから、その `session_id` に一致し `verdict` が `deny`/`ask`
  のレコードを読む。壊れた行・未知のスキーマの行は無視する(fail-open)。
- fingerprint = `(tool, reason_id, match_class, term_hash)` でグループ化し、
  件数が閾値(`THRESHOLD = 3`)以上のものだけを昇格候補にする。**平文の
  マッチ語では群化しない** — bleep が守ろうとしている値そのものだから。
- 候補ごとに、セッション×fingerprint の stamp 台帳
  (`~/.claude/verdict-escalate/state/<session_id>.stamped`、`hook_io::SessionLedger`)
  に無ければ、`wrapup-stop-gate.sh --add <inbox> <json>`(サブプロセス)を
  呼んで inbox に 1 行追記し、成功したら stamp する。**inbox への追記ロジック
  (flock・`jq -ce` での compact 化)は再実装しない** — 唯一の実装
  (`wrapup-stop-gate.sh`)を呼ぶだけ。
- 追記する行は `{"ts","title","detail","repo","go":"ask"}`。`repo` はレコードの
  `repo` フィールド(既定の起票先)、`go:"ask"` は「起票前に人間の明示的な GO
  が要る」印。`detail` には fingerprint・件数・`session_id`・ローカルレッジャー
  への grep 手順を書くが、**平文のマッチ語・コマンド本文は書かない**
  (`term_hash` は短い辞書語なら総当たり可能なので本文にも出さない)。

## 先行例との差分

- Sentry の Issue Grouping(fingerprint → stack → exception → message の順で
  群化、`message` は最後の手段)と同じ考え方で、理由文でなく閉語彙タプルで
  群化する。
- Claude Code の SendFeedback(「A tool or command keeps failing」で下書きを
  作り、送信は人間の `/feedback` のみ)と同じ「ローカルに溜めて、送信は人間
  の GO 後」モデル。ここでは「送信」を「Issue 起票」に、「/feedback」を
  「Stop 指示文 + AskUserQuestion」に対応させている。
- 既存の wrap-up inbox 配管(`docs/claude/wrapup-inbox.md`)をそのまま再利用
  し、新しい消費経路(集計 CLI・週次レビュー等)は作らない。

## 限界

- 閾値(3 回)はセッション内のみを見る。セッションを跨いで同じ fingerprint
  が繰り返される「恒常的な偽陽性」(bleep#15 のような設定起因のもの)は
  この仕組みでは検出しない — 将来、fingerprint をセッション横断で集計する
  拡張の余地として残す(ADR-478 の「後続」節)。
- `go:"ask"` による GO 必須化は Stop 指示文への LLM の追従に依る。hook が
  「AskUserQuestion を実際に呼んだか」を検証する手段はない。ただし本文は
  平文の私的情報を持たないため、GO が守っているのは秘匿ではなく「起票先を
  人間が最終判断する」という運用上の一線である。
- bleep 以外のツールがこのレッジャーの書き手になるには、
  `docs/schemas/agent-verdict.schema.json` の契約に従って
  `agent-verdicts/<tool>.jsonl` に書くだけでよい(登録リストは無い、
  ADR-0025 の「存在すら書かない」原則と同じ理由でツール一覧を repo に持たない)。

## 縮退

- `WRAPUP_VERDICT_ESCALATE_BIN`(または既定解決先)にバイナリが無い →
  `wrapup-stop-gate.sh` 側が何もせずスキップ。
- レッジャー dir が読めない・stamp 台帳が作れない・`wrapup-stop-gate.sh`
  が見つからない → `verdict_escalate::run()` は何もせず `0` を返す。
- `wrapup-stop-gate.sh --add` が失敗した候補は stamp しない(次回 Stop で
  再試行する)。

## 検証

- `cargo test -p verdict-escalate`(fingerprint・閾値・inbox 行組み立ての
  単体テスト、レッジャー dir → inbox 追記 → stamp までの統合テスト、CLI 表面の
  trycmd テスト)。
- `cargo test -p verdict-escalate --test schema`(`docs/schemas/agent-verdict.schema.json`
  が `record::VerdictRecord` からの生成物と一致していること)。
- `bash config/claude/hooks/wrapup-stop-gate.sh --selftest`(Stop 本体が
  inbox 読み取り前に呼ぶ配線・`--check-dup` の repo 引数)。
