# ADR-478 — 自作ツールの deny/ask をローカルの判定レッジャーに集約し、人間の GO 後に開発元へフィードバックする

- Status: Accepted
- Date: 2026-09-25
- Issue: No-Issue(`/grill-me` セッション中に発見・裁定)

## Context

`bleep`(tarotene/bleep、public)は Claude Code / Codex / Copilot の
PreToolUse hook と `git push` の pre-push hook から呼ばれ、private
リポジトリ名・org 名の漏洩を deny/ask するガードレールである。ユーザーの
観察「エージェントが何度も弾かれている様子を見ると使いにくいのかなと
思う」を、このマシンの Claude Code transcript(`~/.claude/projects/`
全 146 ディレクトリ)で実測した:

| 指標 | 値 |
|---|---|
| 純粋な PreToolUse deny(bleep 由来) | 84 件 / 34 セッション |
| 同一セッションで ≥3 deny | 9 セッション(最大 19) |
| 連続 deny ペアのうち逐語再試行 | 2 / 118(残り 116 はコマンドを変形して探る) |

deny 理由文は定数 1 種(「denylist に登録された company/private リポジトリ
の参照と一致しました。」)で、何がマッチしたかを示さない。結果、エージェント
は (1) pre-push が案内する bypass 環境変数を試す、(2) scanner を手で叩いて
トークン総当たりする、(3) private 名を `gh issue create` のタイトルに書いて
再度 deny される、という「滑り」を起こしていた。

一方、bleep は判定を一切記録せず、dotfiles 側にもそれを観測する hook が
無い。Claude Code / Codex / Copilot のいずれも、PreToolUse hook が deny した
事実を*他の* hook に通知するイベントを持たない(Anthropic, Hooks reference,
<https://code.claude.com/docs/en/hooks>、取得 2026-09-25)。「弾かれ続けて
いる」という事実は、判定を下したその場所以外に単一正本を作れない。

この ADR は、判定を下した bleep 自身がローカルにその事実を記録し、
dotfiles の Stop 時にセッション単位で集約して、既存の wrap-up inbox 経由で
**人間の明示的な GO を得てから** 開発元(tarotene/bleep)へ Issue を起票
できるようにする配管を定める。配管そのものはツール非依存に設計し、
herdr・telepath・dotfiles 自身の guard 群も後から書き手として乗れるように
する。

## Decision

### D1: 捕捉点は判定を下すツール自身のローカルレッジャー書き込みにする

本命: なし — 3 候補を同じ 3 軸で比較して選んだ
対抗馬: Stop 時の Claude Code transcript 走査(`is_error` +
`toolDenialKind` を読む; Claude 限定・非公開形式・理由文以上の情報が
無い); claude.nix で `bleep.sh` をラップして stdout JSON を記録
(ホストプロトコル知識の二重正本)
外した候補: Claude Code の OTel `claude_code.tool_decision{source=hook}`
を collector で受ける — 感触で外した(自分 1 人のために collector を
常駐させたくなかった)。分析ではない。
先行例: Anthropic, Claude Code Hooks reference
<https://code.claude.com/docs/en/hooks> (取得 2026-09-25)
差分: 一致 — 同 reference は「matching hooks は並列実行」「PreToolUse
hook の deny を他 hook に通知するイベントは無い」ので、判定側が記録する
以外に単一正本を作れない
軸: 表現不可能 — 単一正本(判定した場所で記録)> 複写+同期(派生物の
パース)

### D2: 昇格信号は「同一セッション・同一 fingerprint で ≥3」

先行例: Anthropic, Tools reference(SendFeedback のトリガー「A tool or
command keeps failing」) <https://code.claude.com/docs/en/tools-reference>
(取得 2026-09-25)
差分: 異なる — 先行例は「同じコマンドが失敗し続ける」を見るが、実測では
連続 deny 118 ペア中 116 がコマンドを変形しているため、コマンド同一性
ではなく判定の fingerprint 同一性で数える
軸: 検出のみ — 閾値判定は実行時にレッジャーを数える以外にない(記録
時点では「3 回目になるか」を表現できない)

### D3: fingerprint は閉語彙タプル、自由記述の理由文は使わない

fingerprint = `(tool, reason_id, match_class, term_hash)`。
先行例: Sentry, Issue Grouping(fingerprint → stack → exception →
message の順で群化) <https://docs.sentry.io/concepts/data-management/event-grouping/>
(取得 2026-09-25)
差分: 一致 — 「message は最後の手段」に従い、理由文でなく安定 ID で
群化する
軸: 表現不可能 — 閉語彙(`reason_id` enum)> 自由記述+事後 lint

### D4: レコードは平文のマッチ語・コマンド本文・理由文を持たず、語はランダム鍵付きハッシュのみ

先行例: Astro, Telemetry(「file paths, contents of files, git remote
information」を除外) <https://astro.build/telemetry/> (取得
2026-09-25); Microsoft, .NET SDK telemetry(cwd・引数を SHA256 ハッシュ)
<https://learn.microsoft.com/en-us/dotnet/core/tools/telemetry> (取得
2026-09-25); [ADR-0034](0034-machine-state-wrapper-flake.md) D5
差分: 一致 — 加えて bleep 固有に「マッチ語 = 漏らしてはいけない値
そのもの」なので平文フィールドをスキーマから除く
軸: 表現不可能 — 書けないフィールドはスキーマに存在しない(事後
サニタイズではない)

### D5: 記録は既定で有効、送信機構を持たず、`DO_NOT_TRACK`/`<TOOL>_NO_LEDGER` で無効化する

先行例: Esteban Kuber, No telemetry in the Rust compiler: metrics
without betraying user privacy, 2023-08-01
<https://internals.rust-lang.org/t/no-telemetry-in-the-rust-compiler-metrics-without-betraying-user-privacy/19275>
(取得 2026-09-25); Console Do Not Track <https://donottrack.sh/> (取得
2026-09-25)
差分: 一致 — 「記録と送信を分離し、送信を持たない」モデル。GitHub CLI
の opt-out 送信(2026-04)への反発は送信があるから起きたもので、ローカル
限定なら同意 UI を要しない
軸: 還元 — 送信を持たないことで同意フロー・匿名化基盤という機構を丸ごと
不要にする

### D6: 消費経路は既存 wrap-up inbox を再利用し、`repo`/`go:"ask"` フィールドを足す

`go: "ask"` の行は Stop 指示文が AskUserQuestion による明示 GO を要求し、
GO 前に起票させない。`gh-edit-allow` は同セッションで作成実績のある
リポジトリへの `gh issue create` を自動 allow するため、permission prompt
は人間 GO の境界にならない。
先行例: `docs/claude/wrapup-inbox.md`(採取 → Stop で起票を促す配管)
(取得 2026-09-25); Anthropic, Tools reference(SendFeedback は下書きを
ローカルに置き、送信は人間の `/feedback` のみ)
<https://code.claude.com/docs/en/tools-reference> (取得 2026-09-25);
[ADR-387](387-wrapup-chores-adjudication-first.md)(wrapup-chores の裁定は
AskUserQuestion で先に決着させる)
差分: 異なる — 既存 inbox は重複確認後に直ちに起票させ、人間 GO の段階
が無い。起票先固定も「このプロジェクト」のみ。`repo` で向け先を、`go` で
GO 必須を行単位に表現する
軸: 検出のみ — GO の実施は Stop 指示文への LLM の追従に依る(hook で
AskUserQuestion の実施を検証できない)。GO を経ずに起票されても本文は
D4 により平文の私的情報を持たないため、GO が守るのは秘匿ではなく振り
直しの判断

### D7: 昇格判定は独立 Stop hook として登録せず、wrapup-stop-gate.sh が inbox を読む前に逐次呼ぶ

先行例: Anthropic, Claude Code Hooks reference(matching hooks は並列
実行) <https://code.claude.com/docs/en/hooks> (取得 2026-09-25);
`gh-edit-allow` の「1 バイナリを既存 hook から呼ぶ」形
(`home/modules/claude.nix` L764-772)(取得 2026-09-25)
差分: 異なる — 独立登録だと escalate と inbox 読みの順序が非決定になり、
最終 Stop で追記が次セッションまで見えない。逐次呼び出しで順序を固定する
軸: 表現不可能 — 順序競合という不正状態を構造的に排除する

### D8: レコード契約の正本は Rust の型(serde + schemars)、JSON Schema は生成物

本命: なし — 手書き Schema 案と同じ軸で比較した
対抗馬: 手書き JSON Schema を正本にして `jsonschema` crate で fixture を
検証(struct と Schema の二重正本になる)
外した候補: 契約を別 crate / 別リポに切り出す — 感触で外した(ツール
1 つの時点で過重に感じた)。分析ではない。
先行例: [ADR-0024](0024-hook-cli-scripts-target-rust.md) Amendment 2 の
`hook-io`(共有ロジックを型として 1 箇所に寄せる)(取得 2026-09-25)
差分: 一致 — 型を正本にし、文書はそこから導出する
軸: 表現不可能 — 単一正本(型)> 複写+同期(struct と Schema を手で揃える)

### D9: マッチ語のハッシュは `bleep-hook`(Rust)の `hash` サブコマンドに委ねる

本命: なし
対抗馬: `sha256sum` → `shasum -a 256` フォールバックを bash に書く
(darwin 差を実行時に吸収)
外した候補: `openssl dgst -hmac` — 感触で外した(bleep の依存に openssl
CLI を増やしたくなかった)。分析ではない。
先行例なし: bleep リポ内(bash 本体・CONTRIBUTING)と coreutils/macOS の
`sha256sum` 可搬性を一次情報の範囲で確認したが、bash ツールがハッシュを
外部化する先行例は見つけていない
軸: 表現不可能 — pin 固定された単一実装(`bleep-hook` は既に `lex` で
必須依存)> ホストごとに実装が変わる coreutils

### D10: 命名は `agent-verdicts/`・`verdict-escalate` とし、既存の `-gate`/`-guard`/`-allow` 接尾辞は使わない

先行例: `config/claude/hooks/` の命名(`*-gate.sh` は deny/block を
返す、`*-guard.sh` は deny、`*-allow.sh` は allow)(取得 2026-09-25)
差分: 異なる — 本 hook は判定を返さず inbox へ書くだけなので、判定を
含意する接尾辞を避ける
軸: 還元 — 既存語彙で表せない仕事にだけ新語を足す

### D11: pre-push の deny 文から bypass 案内を外す(段2、隣接負債)

先行例: tarotene/bleep CONTRIBUTING.md・README(「deny 理由に bypass 手段
を書かない」) <https://github.com/tarotene/bleep> (取得 2026-09-25);
実測: transcript で pre-push の文言を見たエージェントが bypass を 4 回
試行
差分: 一致 — dotfiles 側 pre-push だけが upstream 方針から外れていたのを
揃える
軸: 表現不可能 — エージェントが読む面に bypass 情報が存在しない状態に
する(「使うな」と書く検出型ではない)

## レッジャー契約

- 場所: `${XDG_STATE_HOME:-~/.local/state}/agent-verdicts/<tool>.jsonl`
  (dir 0700、ファイル追記のみ、1 行 1 レコード)。
- ハッシュ鍵: `${XDG_STATE_HOME}/agent-verdicts/hmac-key`(32 byte
  ランダム、0600、最初の書き手が生成)。ツール横断で同じ鍵を使うので
  同一語は同一ハッシュになる。
- レコード(閉語彙。フル定義は `crates/verdict-escalate/src/record.rs` と
  生成物 `docs/schemas/agent-verdict.schema.json`):
  ```json
  {"v":1,"ts":"2026-09-25T12:34:56Z","tool":"bleep","tool_version":"0.2.0",
   "repo":"tarotene/bleep","host":"claude","session_id":"<uuid or null>",
   "verdict":"deny","reason_id":"repo-ref","match_class":"plain",
   "term_hash":"<hex64 or null>","tool_name":"Bash"}
  ```
- 無効化: `DO_NOT_TRACK`(非空)または `<TOOL>_NO_LEDGER=1` で書き込みを
  止める。書き込み失敗は判定結果に影響させない(fail-open)。

## Alternatives considered

各 `Dn` の「対抗馬」「外した候補」に列挙した個別の代替案に加え、配管
全体としての代替案:

- **bleep を含む各ツールが直接 GitHub Issue を起票する。** 棄却。
  「収集・集約は自動、起票は人間の GO 後」という確定要件(#463 と同じ
  方向)に反し、private 名の混入を人間が確認する機会が失われる。
- **専用の集計 CLI + 週次レビュー(weekly-backlog-review)に流す。** 棄却
  ではなく後続で検討可能だが、v1 では既存の wrap-up inbox という 1 本の
  経路で十分(D6)。セッション直後の文脈が失われるコストの方が大きい。

## Consequences

- `crates/verdict-escalate/`(新設)が `agent-verdicts/*.jsonl` を
  読み、閾値を超えた fingerprint を wrap-up inbox へ追記する。
- `config/claude/hooks/wrapup-stop-gate.sh` が inbox 読み取り前に
  `verdict-escalate` を逐次呼ぶようになり、`--check-dup` が `repo`
  引数を取れるようになる。Stop の指示文に `go:"ask"` 行の GO 手順が
  追加される。
- `home/modules/claude.nix` が `verdict-escalate` バイナリを配備する
  (hook 登録はしない)。
- tarotene/bleep 側は別 PR(<https://github.com/tarotene/bleep/pull/32>)
  で判定レッジャーへの書き込みを実装する。この PR のマージ後、
  `flake.nix` の bleep pin を上げる作業が後続として残る(このセッションの
  範囲外 — マージ待ちのため)。
- `config/git/hooks/pre-push` の bypass 案内撤去は stacked PR の段2で
  別途行う(D11)。

## Verification

- `nix flake check`
- `nix develop --command cargo test --workspace --locked`
  (`crates/verdict-escalate` の schema 一致テスト・trycmd を含む)
- `bash config/claude/hooks/wrapup-stop-gate.sh --selftest`
- 手動 E2E: fixture レッジャー(同一 session_id で deny 3 行)を置き、
  Stop 入力 JSON を `wrapup-stop-gate.sh` に流して stderr に
  `[wrapup-inbox]` と `repo`/`go` 付き行が出ることを確認する。

## 執行点

- `crates/verdict-escalate/` — 新規クレート、レッジャー集約 + inbox 追記
- `docs/schemas/agent-verdict.schema.json` — 生成物、schema 一致テストで検証
- `config/claude/hooks/wrapup-stop-gate.sh` — 逐次呼び出し・`repo`/`go` 対応
- `home/modules/claude.nix` — `verdict-escalate` バイナリの配備
