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

## Architecture Decision Records ([`adr/`](adr/))

- [ADR-0001](adr/0001-home-manager-as-source-of-truth.md) — home-manager is
  the source of truth; apt + per-project runtimes are escape hatches.
- [ADR-0002](adr/0002-runtimes-and-hybrid-translation.md) — runtime
  consolidation + hybrid config translation.
- [ADR-0003](adr/0003-secrets-and-identity.md) — secrets & identity
  (YubiKey-rooted, runtime SOPS). See the Amendment for the deployed model.
- [ADR-0004](adr/0004-repo-identity-and-relocation.md) — repo identity &
  relocation.
- [ADR-0005](adr/0005-shell-extension-init-no-auth-gate.md) — shell-extension
  init gates on binary existence, not auth.
- [ADR-0006](adr/0006-gl-for-nix-gui-apps.md) — nix GUI apps carry their own
  GL stack (nixGL); the system graphics stack stays apt.

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
- [`claude-permissions.md`](claude/claude-permissions.md) —
  `permissions.allow` under nix: declarative, idempotent jq merge + retirement.
- [`opusplan-model-aliases.md`](claude/opusplan-model-aliases.md) — Opus Plan
  Mode は *エイリアス* のペア: `opus` を Fable 5 に差し替えて「Plan 中は Fable
  5(1M)、実行中は Sonnet 5」にする + `fallbackModel`。
- [`herdr-sidebar-metadata.md`](claude/herdr-sidebar-metadata.md) — Herdr
  sidebar: per-agent Claude mode/model/metrics via pane metadata (2-channel:
  hook for permission mode, statusline for model/ctx/cost/effort).
- [`claude-usage.md`](claude/claude-usage.md) — Herdr tab bar:
  Claude rate-limit usage (5h session window / weekly per-model cap) with
  burn-rate prediction, from the undocumented `/usage` API (fail-soft: the
  segment just disappears).
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

## Investigation records

- [`ime-chrome-diagnosis.md`](ime-chrome-diagnosis.md) — fcitx5 trigger-key
  investigation (#14): methodology, traces, and the recovery path.

## Miscellaneous

- [`falcon-sensor.md`](falcon-sensor.md) — company EDR agent notes.
- [`nixification-roadmap.md`](nixification-roadmap.md) — literal configs
  worth translating to Nix DSL later, per ADR-0002.
