# ADR-625 — LLM 向け hook 出力を `<hook-directive>` 外枠 + 英語本文で書く(試験導入)

- Status: Accepted(wrapup 系 4 出力に範囲を確定。他 hook へは展開しない — Amendment 2026-10-02、#624)
- Date: 2026-10-01
- Issue: No-Issue(セッション内の会話から直接起票)

## Context

`wrapup-stop-gate.sh` は inbox が非空だと、約 25 行の起票手順書を毎回 stderr
に出して exit 2 していた。ユーザーから「ノイジー」と指摘を受けた。調べると、
Claude Code の Stop hook 出力は経路を問わずユーザーの transcript に表示される:

| 経路 | Claude に届く | ユーザーに見える |
|---|---|---|
| Stop の exit 2 stderr | ○ | ○(hook error として) |
| Stop の `decision:"block"` + `reason` | ○ | ○ |
| Stop の `additionalContext` | ○ | ○(「Stop hook feedback」) |
| SessionStart / UserPromptSubmit の `additionalContext` | ○ | ×(チャットに出ない) |

(Anthropic, "Hooks reference", <https://code.claude.com/docs/en/hooks>、
2026-09-30 取得)。つまり Stop で「LLM にだけ長文を渡し、人には隠す」手段は
仕様上存在せず、量を減らすしかない。

あわせて、「LLM とマシンの間でだけやり取りするメッセージは専用の言語で書けないか」
という問いが出た。このリポジトリの hook が LLM 向けに出す文言は約 30 箇所あり、
ほぼすべて日本語で、出力言語を定めた ADR/docs は無かった。調査結果:

- 言語: Claude の日本語性能は英語比 96.8%(Sonnet 4.5、Anthropic,
  "Multilingual support",
  <https://platform.claude.com/docs/en/build-with-claude/multilingual-support>、
  2026-09-30 取得)。tokenizer による言語間のトークン数格差は Petrov, La Malfa,
  Torr, Bibi, "Language Model Tokenizers Introduce Unfairness Between
  Languages", NeurIPS 2023(<https://arxiv.org/abs/2305.15425>、2026-09-30
  取得)が示すが、現行 Claude tokenizer での日英比は二次情報(約 1.2 倍)しか
  見つからなかった。英語化の効果は小さく、決定的ではない。
- 形式: Anthropic は指示・文脈を XML タグで区切ること、強い強調語(MUST 等)を
  避けることを推奨する("Prompting best practices",
  <https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/claude-prompting-best-practices>、
  2026-09-30 取得)。tool/hook→LLM メッセージの確立した DSL は見つからなかった
  (MCP spec 2025-06-18 Tools 章
  <https://modelcontextprotocol.io/specification/2025-06-18/server/tools> も
  外枠のみを規定)。
- 英語の reminder が応答言語をドリフトさせるかを測った研究は見つからなかった。

## Decision

LLM 向けの hook 出力(人間が読むことを主目的としない出力)は次の書式で書く。
目的は精度・コストの改善ではなく、**人向け/LLM 向けの区別を形式で表すこと**に
置く。

1. `<hook-directive source="<hook 名>" event="<イベント名>">` または
   `kind="<種別>"` の XML 外枠で囲む。
2. 本文は英語で、「何が足りないか・次に何をするか・なぜか」を短く書く。強い
   強調語は使わない。
3. 照合語(gate が機械照合する見出し・タグ・JSON キー・コマンド名・
   AskUserQuestion の選択肢・フッター文言・規範文書の節名)は原文のまま埋め込む。
4. Stop のように人にも見える経路では、本文を短いポインタにし、静的な詳細は
   エージェントが自分で取りに行くサブコマンド(例: `--procedure`)に置く。
5. 応答言語を固定する一文("Reply in Japanese" 等)は入れない。Claude Code の
   言語設定が system prompt で効いているため重ねない。

試験導入の範囲は wrapup 系の 4 出力(Stop の inbox ポインタ、`--procedure`
の手順書、Stop の feedback-memory ブロック、SessionStart の inbox 案内)に限る。
他 hook(`pr-gate.sh`、PreToolUse の deny 理由群、各 SessionStart)には、
試験導入を評価してから展開するかを判断する。

## Alternatives considered

- **手順書を skill に移す**: `${self}`・`${inbox}`・Codex 用フッター env を
  展開できず、Codex からは Claude の skill が読めない。不採用。
- **手順書を SessionStart `additionalContext` に常時注入する**: ユーザーには
  見えないが、inbox を使わないセッションでもコンテキストを毎回消費する。不採用。
- **全 hook を一括で英語化する**: selftest の文言依存が約 70 箇所あり、効果の
  証拠が弱いまま移行コストだけを払うことになる。不採用(試験導入で評価してから)。
- **日本語のまま外枠だけ付ける**: 区別の形式化は満たすが、英語の方が効果の方向
  としては有利(差は小さい)なので、試験導入では両方を入れて評価する。

## Consequences

- inbox 非空時の Stop 出力は外枠込み 4 行になる。エージェントは `--procedure`
  を 1 回余分に実行する。
- Codex adapter はフッターを env で差し替えるため、Stop 出力が示す
  `--procedure` コマンドには env を明示的に前置する(エージェントの shell には
  hook 起動時の env が引き継がれない)。
- 英語と日本語の reminder が混在する期間が生じる。

## 執行点

- config/claude/hooks/wrapup-stop-gate.sh
- config/claude/hooks/wrapup-session-start.sh

## Verification

- `bash config/claude/hooks/wrapup-stop-gate.sh --selftest` — Stop 出力が
  外枠 4 行に収まること、示された `--procedure` コマンドの出力に go:ask の
  扱いと(差し替え後の)フッターが含まれることを検査する。

## Amendment (2026-10-02 — 試験導入の評価と範囲の確定, #624)

試験導入の評価観点(#624)を、実績に当てて評価した。**評価に足る実績が無かった**
ので、その事実を残したうえで、展開しない側に倒して範囲を確定する。

### 評価

調べた範囲: この開発機の Claude Code のセッション記録(`~/.claude/projects/`)と
gate-events(`~/.local/state/claude/gate-events.jsonl`)。取得日 2026-10-02。

- **応答言語のドリフト**: 観測できなかった。`<hook-directive source=…>` を hook 出力として
  実際に受け取ったセッションは 0 件(記録に現れるのは、この ADR や Issue を読んだ
  セッションだけ)。この機体は試験導入の書式が入った世代を `hms` で適用する前だった。
- **手順の取りこぼし**: 観測できなかった(同上)。`"go":"ask"` 行を AskUserQuestion なしで
  起票した、`--procedure` を取りに行かずに起票した、といった事例は記録に無い。
- **体感ノイズ**: 観測できなかった(同上)。設計上は、inbox 非空時の Stop 出力は外枠込み
  4 行。

### 決定

- 試験導入の範囲(wrapup 系の 4 出力: Stop の inbox ポインタ、`--procedure` の手順書、
  Stop の feedback-memory ブロック、SessionStart の inbox 案内)を、そのまま恒久の範囲に
  確定する(試験の注記は外す)。
- **他 hook(`pr-gate`、PreToolUse の deny 理由群、各 SessionStart)へは展開しない。**
  理由は元の Decision と同じ — 英語化の効果は小さく決定的でないこと、文言依存の
  selftest(約 70 箇所)の移行コストに対して証拠が弱いこと。評価で効果を示す実績が
  得られなかったので、この判断は覆らない。
- 再評価の条件: wrapup 系で、応答言語のドリフトまたは手順の取りこぼしが実際に観測された
  ら、Issue を立てて書式を見直す(ADR を Superseded にするかを判断する)。逆に、展開を
  望む具体的な根拠(精度・トークンの測定)が出たら、展開の Issue を立てる。

### 執行点

- `crates/wrapup-stop-gate/tests/directive_scope.rs`

## Amendment (2026-10-02 — レトロの出力を範囲に加える, ADR-0000)

作業終了時のレトロ(ADR-0000)が、wrapup 系に次の 2 つの出力を足す: Stop の
`kind="retro"` ポインタと、`--retro-procedure` の手順書(`kind="retro-procedure"`)。
どちらも `crates/wrapup-stop-gate` の中にあり、書式は元の Decision のとおり
(`<hook-directive>` 外枠 + 英語本文、Stop 出力は短いポインタ、詳細は
サブコマンドで取りに行く)。**範囲の境界は変わらない** — 他の crate には展開せず、
`directive_scope.rs` が引き続きそれを検査する。

### 執行点

- `crates/wrapup-stop-gate/src/retro.rs`
