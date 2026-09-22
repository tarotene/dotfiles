# global-agents-md — グローバル agent 指示の正本を共有 AGENTS.md に一元化する

実装は `config/agents/AGENTS.md`、配備は `home/modules/claude.nix` の
`home.file` で 3 箇所(`~/.agents/AGENTS.md`・`~/.codex/AGENTS.md`・
`~/.copilot/copilot-instructions.md`)に同一ソースをマウントする。決定の
経緯・根拠は ADR-0032。

## なぜ Claude 専用のまま成長していたか

`global-claude-md.md` が記録するとおり、`~/.claude/CLAUDE.md` は「Claude
Code が全セッションに常時読み込む唯一の経路」という理由で作られた。この
時点(#252 系)では Codex CLI・Copilot CLI にグローバル指示ファイルを
配備する仕組みが無く、比較対象が存在しなかったため「グローバル指示 =
Claude 専用ファイル」という前提が疑われないまま定着した。

一方でリポジトリ単位の指示ファイルは ADR-0016 で「AGENTS.md が AI 向け
正本、CLAUDE.md は `@AGENTS.md` import + Claude 固有差分の router」という
二層構造に既に整理されていた。hook 層でも attribution-guard(#192)・
pr-title-guard(ADR-0031)が「判定エンジンを共有し、agent ごとに薄い
adapter を被せる」型を確立していた。グローバル指示ファイルだけがこの
どちらのパターンにも倣わず、Claude 専用のまま取り残されていた。

PR #350(「トピックブランチ更新は merge でなく rebase を使う」)は
git 操作という agent に依存しない規範を、Claude 専用ファイルにだけ
追記する形で発効しようとしていた。この非対称にユーザーが気づいたのが
本改修の直接のきっかけ(grill-me セッション、2026-09-22)。

## 分割線: 規範(共有 AGENTS.md)と施行(CLAUDE.md)

グリルで確定した分割方針は「原則は agent 非依存、施行手段は Claude
Code 固有」。例えば stacked PR(ADR-0027)は「同一セッションの複数 PR は
単一チェーンに積む」という原則自体はどの agent が作業していても成立する
規範だが、`stack-base-guard.sh`(PreToolUse deny)・`pr-gate.sh` の
`G_stack`(Stop block)という具体的な強制手段は Claude Code の hook
イベントに紐づく実装であり、Codex/Copilot に対応する adapter は存在
しない。同様に Plan mode の `## 先行例との対比` 節の書式や
`plan-precedent-gate.sh` の自己検査は ExitPlanMode という Claude Code
専用ツールに紐づく。

この分割線に従い、以下は共有 AGENTS.md 側に移した:

- 検証可能な仮定を放置しない
- 発明する前に先行例を確認する(優先順位の原則。Plan 特有の書式は
  CLAUDE.md に残す)
- 調べ方の規律
- セッション内 PR は単一チェーンに積む(原則。gate 配線は CLAUDE.md)
- 複数項目の依頼は要求インベントリで受ける(原則。`Rn` 書式・gate 配線は
  CLAUDE.md)
- 実装タスクの完了定義
- トピックブランチの rebase 規約(#350)
- GitHub 投稿の生成元明示(フッター文言はエージェント自身の名前・URL に
  一般化。具体文面は各 CLAUDE.md 側)

## 各 CLI のマウント先を一次情報から決めた理由

「グローバル AGENTS.md」自体は agents.md 仕様(https://agents.md、
2026-09-22 取得)では未規定(リポジトリ内配置のみを規定)で、各 CLI の
独自拡張である。一次情報を確認した結果:

- **Codex CLI**: `~/.codex/AGENTS.md` をネイティブに読む。global → repo
  root → cwd の順で連結し後勝ち、合計 32 KiB 上限(OpenAI 公式 "Custom
  instructions with AGENTS.md"
  https://developers.openai.com/codex/guides/agents-md、2026-09-22 取得)。
- **Copilot CLI**: グローバル AGENTS.md には非対応。ユーザー単位の指示
  ファイルは `~/.copilot/copilot-instructions.md`(GitHub Docs "Adding
  custom instructions for GitHub Copilot CLI"
  https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-custom-instructions、
  2026-09-22 取得)。
- **Claude Code**: グローバルメモリは `~/.claude/CLAUDE.md` のみ。`@path`
  import で任意の絶対パス(`~` 展開含む)を import でき、user 層の import
  は承認ダイアログなしで信頼される(Claude Code 公式 "How Claude
  remembers your project" https://code.claude.com/docs/en/memory、
  2026-09-22 取得)。

したがって配備先は Codex/Copilot はそれぞれのネイティブパスへの直接
マウント、Claude だけ `~/.claude/CLAUDE.md` から `@~/.agents/AGENTS.md`
を import する形になり、3 者で統一的な単一パスには揃わない。

## 配備機構: `~/.agents/skills/` と同型

正本 1 ファイルを `home.file` で複数パスにマウントする方式は、本リポジトリ
が `~/.agents/skills/` で既に採用しているクロスツール共有パターン
(`home/modules/claude.nix`、ADR-0016)をそのまま踏襲した。新しい配備機構を
発明していない。

## 効果の確かめ方

`hms .` 適用後、`readlink ~/.agents/AGENTS.md ~/.codex/AGENTS.md
~/.copilot/copilot-instructions.md` が同一 store path を指すことを確認する。
Claude Code は新規セッションで共有規範(例: rebase 規約)が常時コンテキスト
に含まれるかで確認する。Codex CLI / Copilot CLI は実際に起動して規範の
認識を確認する(手動)。
