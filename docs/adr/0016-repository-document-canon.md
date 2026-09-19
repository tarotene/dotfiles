# ADR-0016 — リポジトリ文書正典: README 正形・litmus 移設・ルート閉集合・人間/AI 文書分離

- Status: Accepted
- Date: 2026-09-18
- Issue: No-Issue(grill-me セッション中に発見・裁定)
- Supersedes: ADR-0013 の Decision 1(README charter スキーマの形)・
  Decision 3(AGENTS.md の扱い)を部分的に置き換える。Decision 2(強制点は
  作成時 skill + 事後監査の 2 つ)、Alternatives、他の Consequences は
  そのまま有効。

## Context

別セッションが並行して開発した charter-sweep スキル(#180)が ADR-0013 の
スキーマを機械的に一括適用した結果(private リポジトリの 1 件で観測 —
具体名は `docs/claude/public-publish-guard.md` の方針により省く)、README に
改名の経緯・日付・「follow-up 待ち」という時限状態記述、個別 Issue 番号の
焼き込み、Issue litmus の長文弁明が現れた。これを受けて README の
確立された規範を一次情報で調査した結果、**ADR-0013 のスキーマ自体が
確立標準から外れていた**ことが判明した — `In:`/`Out:` ラベル付き Scope や
README 内 Issue litmus は standard-readme・GitHub 公式・Art of README の
いずれにも先例がない。

加えて、リポジトリ間で「同じように読めて同じように開発できる」ことを
求めると、README の節構成だけでなく、ルートに置いてよい文書ファイルの
集合、言語の統一、そして「人間が読む文書」と「AI が読む文書」の分離
までを正典として固定する必要がある。dotfiles 自身の `CONTEXT.md`
(94 行、Phase 0–5 の移行史や決定表の複製)がこの問題の実例であり、
`CLAUDE.md` にプロジェクト指示の実体を置く現在の構成も、AGENTS.md という
クロスツール標準(Codex CLI・Copilot CLI が読む)と競合する。

## 一次情報(取得日 2026-09-18、全て個別調査で確認)

- README の必要最小限: GitHub 公式 "About the repository README file"
  <https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes>
  — "A README should only contain information necessary for developers to
  get started"。
- README の節構成・簡潔性: standard-readme spec (Richard Littauer)
  <https://github.com/RichardLitt/standard-readme/blob/main/spec.md> —
  節の種類・順序を仕様で固定、短い説明は 120 字以内、history 節は仕様に
  存在しない。
- Cognitive funneling・永続性: Art of README (Kira/hackergrrl, Wayback
  2023-12-31 — 原リポは 404)
  <https://web.archive.org/web/20231231175007/https://github.com/hackergrrl/art-of-readme>
  — 深い文脈は最下部のみ、"will outlive your repository host"。
- 時限記述の禁止: Google developer documentation style guide "Timeless
  documentation"(Last updated 2024-10-15)
  <https://developers.google.com/style/timeless-documentation> —
  currently/now/does not yet/in the future 等を禁止語として明示。
- 履歴の責務分離: Keep a Changelog 1.1.0 (Olivier Lacan, 2019-02-15)
  <https://keepachangelog.com/en/1.1.0/>。
- Contribution ガイドラインの置き場: GitHub 公式 "Setting guidelines for
  repository contributors"
  <https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/setting-guidelines-for-repository-contributors>
  — CONTRIBUTING.md は起票/PR 作成画面で自動リンク表示される。
- 正本言語(英語 1 本): standard-readme(複数言語時は README.md を英語に
  予約)、W3C "Policy for Authorized W3C Translations" (2005)
  <https://www.w3.org/2005/02/TranslationPolicy.html>、Kubernetes
  "Localizing Kubernetes documentation"
  <https://kubernetes.io/docs/contribute/localization/>、MDN localization
  strategy (Chris Mills, Mozilla, 2020-12-08)
  <https://hacks.mozilla.org/2020/12/an-update-on-mdn-web-docs-localization-strategy/>、
  Google "Write for a global audience"(更新 2026-08-25)
  <https://developers.google.com/style/translation>。実リポ構造確認:
  facebook/react・vuejs/core は README 英語 1 本のみ。
- AGENTS.md ルーティング: Claude Code 公式 memory ドキュメント
  <https://code.claude.com/docs/en/memory.md> — AGENTS.md は直接読まれず
  `@AGENTS.md` import(推奨)または symlink で共存させる。agents.md 標準
  <https://agents.md/>。
- skills のクロスツールルーティング: Agent Skills 公式実装ガイド
  <https://agentskills.io/client-implementation/adding-skills-support> —
  配置は仕様非規定、`.agents/skills/` は「広く採用されたクロスクライアント
  慣習」。Codex CLI <https://developers.openai.com/codex/skills> は
  `.agents/skills`(repo)・`~/.agents/skills`(user)をネイティブ読取。
  Copilot CLI <https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-skills>
  は `.claude/skills` と `.agents/skills` の両方を読取。Claude Code 公式
  skills ドキュメント <https://code.claude.com/docs/en/skills> は
  `.claude/skills/` 系のみで `.agents/skills/` 非対応(対応要望
  <https://github.com/anthropics/claude-code/issues/31005> は open)。

## Decision

1. **README は全節固定スキーマにする。**
   `# <name>` → 目的 1 文(≤120 字、GitHub description と一致)→
   `## Background`(任意、知的出自のみ)→ `## Install` → `## Usage` →
   `## Scope`(地の文で境界と caveats、`In:`/`Out:` ラベル廃止)→
   `## Development` → `## License`。許可外見出し・順序違反・時限語・
   Issue 番号焼き込み・長文弁明は drift とする。
2. **Issue litmus は CONTRIBUTING.md へ移設する。** 判定問 1 文 + 採用/
   棄却例各 1–2 行。GitHub が起票・PR 作成画面で自動リンク表示する標準
   機構に乗る(ADR-0013 の「README 内 Issue litmus」を置き換える)。
3. **ルート文書ファイルを閉集合(allowlist)にする。** README.md /
   CONTRIBUTING.md / CHANGELOG.md / LICENSE* / AGENTS.md / CLAUDE.md の
   6 種のみ許可。CONTEXT.md・NOTES.md のような野良ルート文書は drift。
   深い文書は `docs/` 配下に置く。
4. **markdown はファイル単位で言語混在を禁止し、正本言語を固定する。**
   README・CONTRIBUTING は全リポジトリ英語 1 本とし、翻訳ファイルは
   持たない(必要になったリポジトリのみ、standard-readme 準拠の
   `README.ja.md` を後付けしてよいが監査対象外とする)。`docs/` 配下は
   1 ファイル 1 言語であれば日本語可。横断監査(`github-audit`)は
   README+CONTRIBUTING の言語混在のみ判定し、リポジトリ内の全 markdown
   の言語混在検査は各リポジトリの per-repo CI に置く。
5. **人間文書と AI 文書を完全分離する。** README/CONTRIBUTING/CHANGELOG
   にエージェント向け指示を書かない。AI 向け正本はルートの `AGENTS.md`
   1 本とし、README/CONTRIBUTING を参照する側に置く(内容を複製しない
   — ADR-0013 の「AGENTS.md への参照 1 行(任意)」は方向が逆転するため
   ここで supersede する)。`CLAUDE.md` は `@AGENTS.md` import + Claude
   固有差分のみのルータに徹する。CLAUDE.md に実内容が正本化されている
   構成は drift。
6. **skills も同型のルーティングにする。** 正本はツール中立の
   `.agents/skills/`(Codex CLI・Copilot CLI がネイティブ読取)。
   `.claude/skills/` はルーティング専用とする — user スコープは
   home-manager が同一ソースを `~/.agents/skills/` と `~/.claude/skills/`
   の両方へデプロイし、repo スコープは committed 相対 symlink
   (`.claude/skills/<name>` → `../.agents/skills/<name>`)にする。Claude
   Code が `.agents/skills/` をネイティブに読むようになったら、この
   ルーティング層は撤去する。

## Alternatives considered

- **ADR-0013 のスキーマ(`In:`/`Out:` ラベル・README 内 Issue litmus)を
  維持する** — 一次情報のいずれにも先例がなく、charter-sweep(#180)の
  機械適用で品質劣化が実証された。棄却。
- **「3C」(Clear/Concise/Correct)のような通俗的な名前を規範根拠として
  掲げる** — 確立された単一出典が存在しない(遠祖は Cutlip & Center 1952
  の 7 Cs の通俗的縮約)。個別の一次出典を引く方が正確。採らない。
- **翻訳ファイル(`README.ja.md`)を全リポジトリに持たせる** —
  Kubernetes・MDN の先行例が「自給できない翻訳は受け入れない/腐った翻訳は
  アーカイブする」と示す。個人アカウント規模で全リポジトリの翻訳を保守
  する主体はいないため、英語正本 1 本を既定にする。
- **1 ファイル内に日英を混在させる** — 支配的慣行に先例がなく
  (standard-readme は構造的に 1 ファイル 1 言語前提)、節順序・見出し
  リテラル照合とも非両立。棄却。
- **CLAUDE.md を正本のままにし AGENTS.md を追加参照するだけにとどめる**
  — Claude Code 公式ドキュメントが `@AGENTS.md` import を推奨する共存
  手段として提示しており、Codex/Copilot 側は AGENTS.md をネイティブに
  読むため、正本を AGENTS.md に一本化する方が二重管理を避けられる。
- **skills を `.claude/skills/` に置いたままにする** — Codex CLI・
  Copilot CLI は `.agents/skills/` を読み、Claude Code は
  `.agents/skills/` を読まない非対称がある。ツール中立の正本 +
  ルーティングで両立させる。

## Consequences

- `repo-charter` スキルは README 正形テンプレ・CONTRIBUTING.md テンプレ・
  AGENTS.md/CLAUDE.md ルーティングの播種手順・skills 配置手順へ全面
  改訂する。
- `github-audit` の charters ドメインは、目的文一致に加えて README 見出し
  の順序・許可外見出し・CONTRIBUTING.md の litmus・ルート allowlist・
  言語混在・CLAUDE.md ルータ・skills ルーティングを判定する
  (ADR-0015 実装)。
- dotfiles 自身がこの正典の最初の適合例になる(exempt にしない) —
  README の正形化、CONTRIBUTING.md 新設、CONTEXT.md の解体、
  CLAUDE.md → AGENTS.md への正本移行を行う。
- 本 ADR の時点では、charter スキーマが新しい判定項目を含むため、全
  リポジトリが再び `drifted` と報告される。これは想定内の再初期化であり、
  適合化は別セッションの棚卸しで順次行う。
- 文書正典を変更する場合は、この ADR を supersede する新しい ADR を
  起こす(ADR-0008 の規約)。

## Verification

- `github-audit charters --selftest` — README 順序違反・野良見出し・
  litmus 未移設・ルート allowlist 違反・言語混在・CLAUDE.md ルータ違反・
  skills ルーティング違反の各分岐を fixture で確認。
- dotfiles 自身への適用後、`github-audit charters` が dotfiles を `ok` と
  報告することを確認(段5、自己検証)。
- `hms .` 適用後、Claude Code が `~/.claude/skills/` 経由、Codex/Copilot
  が `~/.agents/skills/` 経由で同一 skill を認識することを確認。
