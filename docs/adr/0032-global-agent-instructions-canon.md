# ADR-0032 — グローバル agent 指示ファイルの正本は共有 AGENTS.md、CLAUDE.md は router

- Status: Accepted
- Date: 2026-09-22
- Issue: No-Issue(grill-me セッション中にユーザー指摘から発見・裁定)

## Context

PR #350(「トピックブランチ更新は merge でなく rebase を使う」規約を
`config/claude/CLAUDE.md` に追加)を見たユーザーから、「なぜグローバル指示が
AGENTS.md に一元化されていないのか」という指摘を受けた。

調査の結果、次の事実が判明した:

- ADR-0016 の「AGENTS.md = AI canon、CLAUDE.md = router」は**リポジトリ単位**
  の決定で、グローバル階層(`~/.claude/CLAUDE.md`、`home/modules/claude.nix`
  の `home.file` で配備)には適用されておらず、Claude 専用ファイルとして
  そのまま成長してきた。
- Codex CLI・Copilot CLI にはグローバル指示ファイルを**一切配備していない**
  (配っているのは hooks と `~/.agents/skills/` の skills のみ)。
- 一方 hook 層では attribution-guard(#192)が「Codex/Copilot も GitHub に
  投稿できる」という理由で 3 エージェント共有の decision engine + per-agent
  adapter に分割済みで、pr-title-guard(ADR-0031)も同じ型を踏襲している。
  つまり**フック層は既にクロスツール化した規範を、指示ファイル層は Claude
  専用に留めている非対称**があり、#350 の rebase 規約(git 操作の一般規範で
  agent に依存しない)はまさにこの非対称に落ちる例だった。

一次情報を確認したところ、「グローバル AGENTS.md」自体は agents.md 仕様
(https://agents.md、2026-09-22 取得)では規定されておらず(リポジトリ内配置
のみを規定)、各 CLI の独自拡張である:

- Codex CLI: `~/.codex/AGENTS.md` をネイティブに読む。global → repo root →
  cwd の順で連結し後勝ち、合計 32 KiB 上限(OpenAI 公式 "Custom instructions
  with AGENTS.md" https://developers.openai.com/codex/guides/agents-md、
  2026-09-22 取得)。
- Copilot CLI: グローバル AGENTS.md には非対応。ユーザー単位の指示ファイルは
  `~/.copilot/copilot-instructions.md`(GitHub Docs "Adding custom
  instructions for GitHub Copilot CLI"
  https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-custom-instructions、
  2026-09-22 取得)。
- Claude Code: グローバルメモリは `~/.claude/CLAUDE.md` のみ。`@path` import
  構文で任意の絶対パス(`~` 展開含む)のファイルを import でき、user 層の
  import は承認ダイアログなしで信頼される。再帰深さ上限 4(Claude Code
  公式 "How Claude remembers your project" https://code.claude.com/docs/en/memory、
  2026-09-22 取得)。

## Decision

1. **正本を `config/agents/AGENTS.md`(1 ソース)に新設**し、agent 非依存の
   規範(調査規律・先行例確認・stacked PR の原則・要求インベントリの原則・
   実装タスクの完了定義・rebase 規約・GitHub 投稿の生成元明示)を集約する。
   フッター文言は「Claude Code」固定から「自分自身のエージェント名と URL」
   に一般化する(attribution-guard の Codex/Copilot adapter が各自の
   エージェント名で照合するため、固定文面のままでは Codex/Copilot からの
   投稿が自 gate に矛盾する)。
2. **`home.file` で 3 箇所に同一ソースをマウントする**: `~/.agents/AGENTS.md`
   (Claude Code の import 先)・`~/.codex/AGENTS.md`(Codex CLI ネイティブ)・
   `~/.copilot/copilot-instructions.md`(Copilot CLI ネイティブ)。`~/.agents/skills/`
   のクロスツール共有(ADR-0016 で確立済み)と同型のパターン。
3. **`~/.claude/CLAUDE.md` は router に縮約**する。冒頭を
   `@~/.agents/AGENTS.md` の import 1 行にし、残りは Claude Code 固有の
   施行配線(各 gate スクリプトの自己検査手順・ExitPlanMode・AskUserQuestion・
   wrap-up inbox との接続)のみを持つ。リポジトリルートの CLAUDE.md
   (`@AGENTS.md`)と同型の router 構造。
4. Plan の `## 先行例との対比` 節の書式、`plan-precedent-gate.sh` /
   `plan-scope-gate.sh` の自己検査手順、`stack-base-guard.sh` / `pr-gate.sh`
   の形式検査は Claude Code の Plan Mode / hook に固有の運用のため
   `~/.claude/CLAUDE.md` 側に残す(Codex/Copilot にこれらの adapter は
   存在しない)。

ADR-0016(リポジトリ単位の AGENTS.md/CLAUDE.md 二層)を supersede せず、
同型構造をグローバル階層に拡張するものとして扱う。

## Consequences

- グローバル規範の追記先が 1 箇所(`config/agents/AGENTS.md`)に統一され、
  以後 #350 のような agent 非依存の規範追加が Claude 専用ファイルにだけ
  書かれて Codex/Copilot に届かない、という非対称は再発しない。
- Copilot CLI はグローバル AGENTS.md をネイティブに読まないため、
  ファイル名だけ `copilot-instructions.md` に変わる非対称は残る(仕様上の
  制約であり本決定では解消できない)。
- Codex CLI の合計サイズ上限(32 KiB)に対し、現行の共有 AGENTS.md は
  約 8 KB — 当面の余裕はあるが、将来的な追記の際は上限を意識する必要がある。
- Copilot の read-only カスタムエージェント(plan-reviewer)にも
  git 操作規範等が注入されるが、実害はないと判断(問題が出たら Copilot 側
  マウントのみ個別に見直す)。

## Verification

- `home-manager switch` 後、`readlink ~/.agents/AGENTS.md ~/.codex/AGENTS.md
  ~/.copilot/copilot-instructions.md` が同一 store path を指す。
- `~/.claude/CLAUDE.md` の 1 行目が `@~/.agents/AGENTS.md` である。
- 新規 Claude Code セッションで共有 AGENTS.md の内容(例: rebase 規約)が
  常時コンテキストに含まれることを確認する。

## Amendment (2026-09-28 — Codex CLI にも analogous 機構を展開する, #531)

Decision 4 は「Codex/Copilot にこれらの adapter は存在しない」と書いたが、
Claude 高額利用者が 1 週間 Codex CLI 代替で仕事をする社内検証への備えとして、
この非対称の一部を解除する(#531、4段の stacked PR で段階的に実施)。

- `stack-base-guard.sh` / `pr-gate.sh` / `wrapup-stop-gate.sh` /
  `agent-turn-log.sh` は、attribution-guard/pr-title-guard と同じ「判定
  エンジン1本 + 薄い adapter」の型で Codex CLI にも展開する
  (`config/codex/hooks/`、段2〜3)。判定エンジン側は環境変数
  (`AGENT_NAME` 等)で agent 名・footer 文言を差し替え可能にし、Claude
  からの既定呼び出しは変えない。
- Plan Mode の承認点(ExitPlanMode)は Codex CLI に存在しないため、
  Codex の Stop hook として `codex-plan-gate.sh` を新設する(段4)。これは
  Codex の応答に含まれる `<proposed_plan>` ブロックを検出し、既存の
  `plan-scope-gate.sh --check-plan` / `plan-precedent-gate.sh --check` を
  そのまま呼ぶ薄い adapter である(新しい判定ロジックは増やさない)。
  無限 block 対策は `pr-gate.sh` と同じ「`stop_hook_active` を見ず独自
  カウンタで上限到達時に1回 escalate する」型(docs/claude/copilot-plan-
  review.md の「第二次の非収束」を回避するため)。詳細:
  docs/claude/codex-plan-gate.md(段4)。
- Plan の `## 先行例との対比` 節の書式そのもの、および ExitPlanMode 直前の
  自己検査を「呼び出せ」という指示文は、依然 Claude Code 固有の
  `~/.claude/CLAUDE.md` 側に残る(Codex には ExitPlanMode という UI 概念が
  無く、`<proposed_plan>` を出す判断自体はモデルの応答に委ねられるため)。
  Codex 側の analogous な指示は `config/codex/AGENTS.codex.md`
  (ADR-0032 Decision 2 の3箇所マウントとは別に、Codex 専用節として
  共有 AGENTS.md に build 時結合する新規ファイル、本段(段1)で新設済み)
  に置く。
- decision-colocation-guard・external-send-guard・git-worktree-allow・
  gh-edit-allow・routines-write-guard・plan-view・copilot-plan-review・
  plan-fresh-gate・`atuin hook claude-code` は対象外のまま残す
  (decision-colocation-guard は ADR-396 により CI backstop、他は
  Claude Code の runtime API・UI 概念に依存するため Codex に移植不能)。

### 執行点

- home/modules/claude.nix
- config/codex/AGENTS.codex.md

## Amendment (2026-10-01 — agent-turn-log を Copilot CLI にも展開する)

daily-report の `agent_events`(ターン単位の作業証拠)は、Codex と Copilot の
ターンを取れていなかった。Codex の hook は上の Amendment で宣言済みだが、Copilot は
未配線で、Codex も初回の信頼(`/hooks`)が未了のため一行も書かれていない。

- **`agent-turn-log.sh` を Copilot の `UserPromptSubmit`/`Stop` にも登録する**
  (`AGENT_NAME=copilot`、adapter は新設しない)。Copilot は大文字始まりのイベント名で
  登録すると Claude と同じ snake_case の payload(`hook_event_name` あり)を渡す。小文字
  始まり(`userPromptSubmitted`/`agentStop`)は別形の payload になり、このスクリプトは
  読めない。出典: GitHub Docs, "GitHub Copilot Hooks Reference"
  (https://docs.github.com/en/copilot/reference/hooks-configuration, 取得 2026-10-01)。
- **Codex の識別子**: payload に `prompt_id` は無く `turn_id` がある。`prompt_id // turn_id`
  で読み、`date+pid` の合成 ID に落ちるのを避ける。出典: OpenAI, "Codex Hooks"
  (https://learn.chatgpt.com/docs/hooks, 取得 2026-10-01)。
- **機械起動のセッションを除く**: Copilot のセッションのうち約 8 割は `copilot-plan-review.sh`
  が Claude の hook から起動するもので、本人の作業ではない。`AGENT_TURN_LOG=0` なら
  `agent-turn-log.sh` は何も書かずに終わり、`copilot-plan-review.sh` は Copilot を起動する
  ときにこれを立てる。環境変数による opt-out は機械起動を「表現不可能」にするものではなく
  「検出して外す」ものである。Copilot が hook に呼び出し元の環境変数を渡すかは文書化されて
  いないので、実機で確かめる(下の検証)。
- **棄却**: 各エージェントのセッションログの事後取り込み(形式が文書化されていない)、
  Codex の `notify`(ターン完了だけで開始時刻が取れない)、atuin のコマンド span だけで代替
  (話して読むだけのターンが見えない)。
- daily-report 側は変更不要(`agent` をそのまま通す)。対の文書は
  sugimoto-kentaro-sandbox の `daily-report/docs/adr/0020` Amendment。

### 執行点

- config/claude/hooks/agent-turn-log.sh
- config/claude/hooks/copilot-plan-review.sh
- home/modules/claude.nix
