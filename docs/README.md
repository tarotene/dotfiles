# docs/ index

## Operations

- [`operations.md`](operations.md) — the canonical apply (`hms`), routine
  flake update, and the tool-layer decision flow for new tools.
- [`cutover-runbook.md`](cutover-runbook.md) — per-host provisioning /
  migration procedure, including rollback.
- [`git-sync.md`](git-sync.md) — machine-wide git config + hooks that
  guard herdr's parallel-worktree workflow (stale base, protected-branch
  commits, stale `[gone]` branches).

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

- [`codex-plan-review.md`](claude/codex-plan-review.md) — ExitPlanMode gate:
  Codex reviews the plan; the gate is on severity, not on a verdict.
- [`pr-gate.md`](claude/pr-gate.md) — Stop hook: PR completion barrier (CI 待ち・push 忘れ・Issue リンク忘れ)
  (CI/push, not review/base).
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

## Investigation records

- [`ime-chrome-diagnosis.md`](ime-chrome-diagnosis.md) — fcitx5 trigger-key
  investigation (#14): methodology, traces, and the recovery path.

## Miscellaneous

- [`falcon-sensor.md`](falcon-sensor.md) — company EDR agent notes.
- [`nixification-roadmap.md`](nixification-roadmap.md) — literal configs
  worth translating to Nix DSL later, per ADR-0002.
