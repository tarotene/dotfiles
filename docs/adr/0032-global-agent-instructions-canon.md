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
