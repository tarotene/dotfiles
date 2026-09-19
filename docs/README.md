# docs/ index

## Operations

- [`operations.md`](operations.md) — the canonical apply (`hms`), routine
  flake update, and the tool-layer decision flow for new tools.
- [`cutover-runbook.md`](cutover-runbook.md) — per-host provisioning /
  migration procedure, including rollback.
- [`git-sync.md`](git-sync.md) — machine-wide git config + hooks that
  guard herdr's parallel-worktree workflow (stale base, protected-branch
  commits, stale `[gone]` branches).
- [`worktree-lifecycle.md`](worktree-lifecycle.md) — reject unmanaged worktree
  creation, detect stale registrations, and notify through Herdr.
- [`github-audit-rulesets.md`](github-audit-rulesets.md) — read-only
  cross-repository GitHub ruleset drift audit (#130): why it lives here
  instead of a dedicated inventory repo, and why judgement is by rule-type
  union rather than ruleset name/count.
- [`github-audit-charters.md`](github-audit-charters.md) — read-only
  cross-repository README charter drift audit: the purpose-sentence /
  Scope / Issue-litmus / topics schema, and why judgement is by literal
  presence/match rather than an LLM call.

## Architecture Decision Records ([`adr/`](adr/))

- [ADR-0001](adr/0001-home-manager-as-source-of-truth.md) — home-manager is
  the source of truth; apt + per-project runtimes are escape hatches.
- [ADR-0002](adr/0002-runtimes-and-hybrid-translation.md) — runtime
  consolidation + hybrid config translation.
- [ADR-0003](adr/0003-secrets-and-identity.md) — secrets & identity
  (YubiKey-rooted key model). See the Amendment for the deployed model; the
  runtime-SOPS Decision item is retired by ADR-0010.
- [ADR-0004](adr/0004-repo-identity-and-relocation.md) — repo identity &
  relocation.
- [ADR-0005](adr/0005-shell-extension-init-no-auth-gate.md) — shell-extension
  init gates on binary existence, not auth.
- [ADR-0006](adr/0006-gl-for-nix-gui-apps.md) — nix GUI apps carry their own
  GL stack (nixGL); the system graphics stack stays apt.
- [ADR-0007](adr/0007-naming-and-layout-conventions.md) — 命名・配置規約
  (拡張子・shebang・hook 語彙・`config/claude/hooks/` の純度・docs 対応原則・
  環境変数接尾辞・`scripts/` の位置づけ・モジュール分割の軸)。
- [ADR-0008](adr/0008-documentation-artifact-selection.md) — 記録の器の選択
  規約: 新しい判断・調査を ADR / `docs/claude/*.md` / Investigation record の
  どれに書くか、腐る事実と腐らない決定を分離する理由。
- [ADR-0009](adr/0009-publish-guard-upstream-split.md) — 公開面ガード
  (public-publish-guard)を別リポジトリ `tarotene/publish-guard` へ切り出し、
  flake input で逆消費する決定。「No semver releases」と plugin 配布の
  commit-SHA/tag pin が両立しないことが理由。
- [ADR-0010](adr/0010-retire-sops-runtime-secrets.md) — SOPS ランタイム
  復号チャネル(シェル起動時の自動シークレットロード)の全撤去。棚卸しで
  全消費者(MCP-gdrive・brave-search・Falcon Sensor 含む)が代替済みまたは
  消滅済みと判明したため。ADR-0003 の該当 Decision 項目を supersede。
- [ADR-0011](adr/0011-local-activity-log-capture.md) — ローカル活動ログの
  採取(atuin + Claude Code ターンログ)。オフライン専用の atuin
  history.db と `agent-events.jsonl` の 2 採取点、出力パス・JSON 行の形式
  という消費側(別リポジトリ)向けの契約を固定する。
- [ADR-0012](adr/0012-precedent-grounding-over-prompted-adversarial-review.md)
  — プロンプトでの「敵対的レビュー」「文献調査」の都度指示を、常設
  システムプロンプトへの抽象指示にはせず、著者が先行例へ接地し文脈を
  切った批評者(既存 copilot-plan-review の lens A)が監査する形に
  機構化する決定。文献調査(自己批評の非収束性)を根拠に、抽象指示への
  変換を明示的に棄却した。
- [ADR-0013](adr/0013-repo-charter-schema.md) — 全自作リポジトリの README
  に machine-checkable な charter(目的1文 = description のミラー /
  `## Scope` / `## Issue litmus` / topics)を強制する決定。作成時
  (`repo-charter` スキル)と事後(`github-audit-charters`)の二点で強制し、
  Issue 起票時の意味照合は第 2 弾に送る。README スキーマ自体は
  ADR-0016 が部分 supersede。
- [ADR-0014](adr/0014-repository-naming-classes.md) — リポジトリ命名クラス
  体系(codename / descriptive / pj / site の 4 種)を定め、正本を GitHub
  topics に置く決定。形式一致は機械判定、クラス帰属の意味判断は
  `github-audit-triage` 経由で人間が裁定する。
- [ADR-0015](adr/0015-unified-github-audit-and-triage-loop.md) — 診断を
  `github-audit` 統合 CLI に再編し、決定論ノード(監査)と LLM ノード
  (`github-audit-triage`)を分離した判断ループとして定義する決定。
  charter-sweep(#180)の「merge まで自動」を廃止し、完了定義を PR 作成
  までに固定する。
- [ADR-0016](adr/0016-repository-document-canon.md) — README 全節固定
  スキーマ・Issue litmus の CONTRIBUTING.md 移設・ルート文書 allowlist・
  markdown 単位の言語混在禁止・人間文書/AI 文書の完全分離
  (AGENTS.md 正本化・CLAUDE.md ルータ化・skills の `.agents/skills/`
  ルーティング)を、一次情報(standard-readme・GitHub 公式・Art of
  README・Google style guide 等)に接地して定める決定。ADR-0013 の
  README スキーマと AGENTS.md 扱いを部分 supersede。

## Claude Code tooling ([`claude/`](claude/))

Design and rationale for the hooks and commands deployed from
`config/claude/` by `home/modules/claude.nix`:

- [`copilot-plan-review.md`](claude/copilot-plan-review.md) — ExitPlanMode gate:
  a read-only GitHub Copilot CLI custom agent reviews the plan; the gate is on
  severity, not on a verdict.
- [`pr-gate.md`](claude/pr-gate.md) — Stop hook: PR completion barrier (CI 待ち・push 忘れ・Issue リンク忘れ・視覚証跡忘れ)
  (CI/push, not review/base).
- [`pr-description.md`](claude/pr-description.md) — PR 本文の標準スケルトンと
  Before/After 視覚証跡の判断知識(スキル)+ `G_visual` による強制(ゲート)の
  二層構成。`gh --attach` (>= 2.99.0) の事実と charm-freeze 選定理由も記録。
- [`issue-index.md`](claude/issue-index.md) — SessionStart hook: inject an
  Issue index, not a full crawl.
- [`sign-prewarm.md`](claude/sign-prewarm.md) — SessionStart hook: pre-warm
  the git-signing passphrase cache.
- [`plan-view.md`](claude/plan-view.md) — `/plan-view`: render the
  in-progress plan to HTML in Chrome.
- [`wrapup-inbox.md`](claude/wrapup-inbox.md) — Stop hook: out-of-scope
  findings land in an issue-filing inbox.
- [`git-worktree-allow.md`](claude/git-worktree-allow.md) — PreToolUse hook:
  validated programmatic allow for `git -C <worktree>`, replacing unsafe
  mid-pattern wildcard rules.
- [`git-stash-guard.md`](claude/git-stash-guard.md) — PreToolUse hook: deny
  bare `git stash` (the stack is shared across herdr's parallel worktrees).
- [`public-publish-guard.md`](claude/public-publish-guard.md) — dotfiles-side
  wiring for the PreToolUse hook (deny/ask on `git push` / `gh pr|issue
  create|edit|comment` / MCP GitHub tool calls that would leak a company or
  private repository name); the design and denylist mechanism now live
  upstream in [tarotene/publish-guard](https://github.com/tarotene/publish-guard)
  (ADR-0009).
- [`attribution-guard.md`](claude/attribution-guard.md) — PreToolUse hook: deny
  a `gh pr|issue create|edit|comment` / `gh pr review` whose body carries no
  Claude-Code attribution footer (escape hatch: `No-Attribution: <reason>`).
  Covers the two holes left by the harness-supplied footer: comments never got
  one, and the PR/Issue body side had no repo-side enforcement at all.
- [`claude-permissions.md`](claude/claude-permissions.md) —
  `permissions.allow` under nix: declarative, idempotent jq merge + retirement.
- [`opusplan-model-aliases.md`](claude/opusplan-model-aliases.md) — Opus Plan
  Mode は *エイリアス* のペア: `opus`(Plan 側)と `sonnet`(実行側)を乗っ取り、
  モードを **(Plan 側, 実行側) のペア 3 種**(`fable/sonnet` / `opus/sonnet` /
  `fable/opus`)として `claude-plan-model` で巡回する。モードは実行時状態、
  具体モデル ID は宣言が `latest_per_family` から毎回引き直す(pin ゼロ)。
  `.model` が `opusplan` でなければ書き込む前に落ちる。`fallbackModel` は
  Usage limit では発火しない。
- [`herdr-sidebar-metadata.md`](claude/herdr-sidebar-metadata.md) — Herdr
  sidebar: per-agent mode/model/metrics via pane metadata. Claude is 2-channel
  (hook for permission mode, statusline for model/ctx/cost/effort); Codex and
  Copilot get a leaner branch+model-only reporter each, plus the research
  notes on why tab-bar usage was deferred (#117).
- [`claude-usage.md`](claude/claude-usage.md) — Herdr tab bar:
  Claude rate-limit usage (5h session window / weekly per-model cap) with a
  pace-at-reset projection (average pace since window start → projected % at
  reset), from the undocumented `/usage` API (fail-soft: the segment just
  disappears).
- [`worktree-fresh-base.md`](claude/worktree-fresh-base.md) — SessionStart
  hook: pristine な herdr worktree だけを origin/`<base>` へ黙って
  fast-forward する。
- [`global-claude-md.md`](claude/global-claude-md.md) — グローバル
  `~/.claude/CLAUDE.md`: 検証可能な仮定は情報源(Slack/Drive/GitHub/公式ドキュメント/
  文献)を参照するか明示判断し、発明する前に先行例を確認する調査規律を全セッション
  常時注入する(read-only 配布、`#` 追記は skill-gardening の PR フローへ)。
- [`diagramming.md`](claude/diagramming.md) — 個人スキル: 作図時に内容の型に
  合うジャンル・技術を選ぶ処方と、手書き SVG の技術非依存の不変条件。
- [`skill-gardening.md`](claude/skill-gardening.md) — 個人スキル: 知見を
  この公開リポジトリにスキル化するときのメタスキル(器の判断・配線チェックリスト・
  公開リポジトリ向けサニタイズ規則の正本)。
- [`living-description.md`](claude/living-description.md) — 個人スキル:
  Issue/PR の本文を「起票時点のスナップショット」ではなく「現在の合意状態を表す
  正本」として運用し、コメントで裁定が確定した時点で本文を編集し続ける習慣。
- [`test-grounding.md`](claude/test-grounding.md) — 個人スキル: 複数の実
  コンポーネントが絡む検証項目・試験手順を書く前に、facts 文書+層別モデルで
  一次資料に当たることを強制する。
- [`copilot-model-bump.md`](claude/copilot-model-bump.md) — 個人スキル: 外部
  AI CLI に固定 pin した具体モデル ID を GA・廃止サイクルに追従して更新する
  定型手順(pin 箇所の棚卸し・上流確認・スラッグ実機確認・完了条件)。
- [`stacked-pr.md`](claude/stacked-pr.md) — 個人スキル: PR 同士に依存関係が
  あるとき main 起点で並行させず base を親ブランチにした stacked PR として
  積む手順。なぜ素の `--base` + `gh stack link` を選び `init/submit/sync` を
  避けたか、なぜ pr-gate.sh を触らなかったかの裁定を記録。
- [`tracking-issue.md`](claude/tracking-issue.md) — 個人スキル: 複数の子作業
  を束ねる親 Issue(Tracking Issue)を起票・更新するときの書式規約。地の文と
  sub-issues の二重管理を避け、更新すべき箇所を最小化する。事後の棚卸し・清算は
  `issue-hygiene` が担う。
- [`issue-hygiene.md`](claude/issue-hygiene.md) — 個人スキル: open Issue が
  出自ごとに束ねられず積み上がったとき、GitHub の sub-issues 機能で親子構造を
  明示し直し、腐った tracking Issue を清算する定期衛生管理の手順。起票・更新
  する側の規約は `tracking-issue` が担う。
- [`scope-inventory.md`](claude/scope-inventory.md) — グローバル CLAUDE.md
  ルール + 個人スキル: Tracking Issue や複数項目の依頼を計画に起こすとき、
  子タスクを黙って落とさせないための要求インベントリ(`R1..Rn`)の作り方。
  gate: `plan-scope-gate.sh`。
- [`precedent-grounding.md`](claude/precedent-grounding.md) — グローバル
  CLAUDE.md ルール + 個人スキル: Plan の非自明な設計判断ごとに先行例との
  対比(`D1..Dn`)を成果物に残す書き方。プロンプトでの「敵対的レビュー」
  「文献調査」の都度指示を機構化した経緯は ADR-0012。批評は既存
  copilot-plan-review の lens A、形式検査は `plan-precedent-gate.sh`(gh/LLM
  を呼ばない決定論的 judge)。
- [`repo-charter.md`](claude/repo-charter.md) — 個人スキル: 自作リポジトリの
  README に machine-checkable な charter(目的1文・`## Scope`・
  `## Issue litmus`・topics)を播く/適合化する手順。事後の横断監査は
  `github-audit-charters.md` が担う。パイロット適合(命名と責務の乖離が
  判明した経緯)も記録。

## Investigation records

- [`ime-chrome-diagnosis.md`](ime-chrome-diagnosis.md) — fcitx5 trigger-key
  investigation (#14): methodology, traces, and the recovery path.
- [`stacked-pr-github-native.md`](stacked-pr-github-native.md) — GitHub
  ネイティブ Stacked pull requests 機能の実測(preview ステータス・API
  サーフェス・`gh-stack` 拡張の既知 issue)。時間で腐る事実を
  `claude/stacked-pr.md` の裁定から分離するための器(ADR-0008)。

## Miscellaneous

- [`falcon-sensor.md`](falcon-sensor.md) — company EDR agent notes.
- [`nixification-roadmap.md`](nixification-roadmap.md) — literal configs
  worth translating to Nix DSL later, per ADR-0002.
