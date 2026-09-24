# dotfiles agent instructions

See [README.md](README.md) for what this repository is and does, and
[CONTRIBUTING.md](CONTRIBUTING.md) for what Issues and pull requests are
accepted.
This file is for agent operating instructions only (ADR-0016) — it does not
duplicate README/CONTRIBUTING content.

## Project Overview

Declarative, flake-based **standalone home-manager** configuration for one
person's user environment across two Pop!_OS hosts (`vega`, `arcturus` —
star codenames, ADR-0019; a third, retired `company-pop-old`, was
decommissioned rather than renamed, #214) plus one darwin host (`altair`),
and two identities (personal, company).

**Core purpose**: home-manager is the single source of truth for the user
environment, so a fresh machine is provisioned to an identical setup with
near-zero manual steps. Migrated from the old procedural shell-script installer
(epic #207). See `README.md` and `docs/adr/` for the full charter.

## Three-layer model

| Layer | Owns | Managed by | ADR |
|-------|------|------------|-----|
| **User environment** (source of truth) | shell, git, terminal, user-space CLIs, fonts, prompts, per-user services, GPG agent, GUI apps, **fcitx5 daemon + mozc**, **herdr (binary + sidebar config)** | home-manager | ADR-0001 (+ Amendment) |
| **System layer** (escape hatch) | root, a system service, kernel/driver integration, **or code loaded into an apt-installed process**: build toolchain, cross C toolchain, `scdaemon`, fcitx5 *immodules* (`fcitx5-frontend-all`), login-shell fallback | apt (`scripts/install-packages.sh`) | ADR-0001 (+ Amendment) |
| **Per-project runtimes** (escape hatch) | language toolchains, project-local versions | `mise` / `direnv` / `rustup` launchers (installed by home-manager; toolchains stay project-scoped) | ADR-0002 |

Note on graphics: the driver stack itself is root-owned and stays in the system
layer, but nix GUI apps cannot use it — they load **nix's own mesa** through a
per-package `nixGL` wrapper (the shared wrapper module, ADR-0006), applied to
Linux GUI packages in the desktop module and identity-scoped ones like
`warp-terminal` (the personal identity module, #9).

Note on `herdr`: not yet in the pinned stable nixpkgs channel. Comes from a
single-package `nixpkgs-unstable` overlay in `flake.nix` (ADR-0001 Amendment,
#42) — drop the overlay entry once stable catches up. Unlike `nixgl`, this
overlay does not need `inputs.nixpkgs.follows`: herdr is only ever `exec`'d,
never `dlopen`'d into another package's process, so a second glibc in its
closure is harmless. (`gh`'s own version-capped escape hatch on this same
overlay was dropped once stable shipped >= 2.99.0, see #91.)

Note on `claude`: not managed via nix at all — the native installer
(`~/.local/bin/claude`) is the source of truth on every host (ADR-0000, a
scoped exception to ADR-0001). Background auto-update is disabled by
declaration; updates go through `claude update`, which the zsh wrapper
(`config/zsh/modules/53-tools-claude.zsh`) follows with `claude-plan-model
sync` so the Opus Plan Mode model pin never trails the installed binary.

## Project Structure

Top-level layout, roughly: the flake + home-manager modules (the source of
truth), literal config deployed verbatim, escape-hatch and home-manager-
deployed scripts, the apt package list, architecture/operating docs,
source patches applied via the flake's package overlay, committed public
keys, and the greenfield installer. Browse the tree directly (GitHub's
file view, or a local checkout) for the full breakdown — see
[the docs index](docs/README.md) for a curated, by-category list of the
design-rationale docs.

## Architecture Decision Records

Full list with one-line summaries: [`docs/README.md`](docs/README.md#architecture-decision-records)
(kept as the single index — do not duplicate it here, ADR-0033). Browse
`docs/adr/` directly for the full text of any decision.

## Development Rules

### Nix modules
- Modules are the source of truth. A host module imports the shared module
  plus exactly one identity module; the shared module imports everything
  under the per-topic modules directory.
- `homeConfigurations.<hostname>` in the flake is keyed by hostname so
  `home-manager switch --flake .#"$(hostname)"` auto-selects per machine.
- Identity-scoped values (git `user.name`/`email`, browser default) go in
  the identities directory; per-machine values (signing key bound to the
  host's YubiKey/[S] subkey) go in the hosts directory.
- Format with `nix fmt` (nixfmt-tree — a treefmt wrapper that feeds nixfmt
  every Nix file, so no arguments are needed).

### Private machine-state values vs. public rules (ADR-0034)
- This repo is PUBLIC. Write the **rule, schema, or derivation procedure** for
  a machine-state decision here, with a placeholder standing in for any real
  value (see the operations doc's B2 endpoint example). Do **not**
  write the value itself — a real bucket name, GCP project ID, Healthchecks
  ping URL, backup-identity UUID, PRIVATE repository name, owned domain, or a
  private wrapper flake's own path/name — anywhere in this repo's source,
  Issues, or PRs. Those go in the private wrapper flake instead (ADR-0034);
  this repo never takes that flake as an input (`flake.lock` would record its
  `{owner, repo}` in the clear) and never names it — resolve it only through
  the private-hub marker (see the `hms` apply wrapper's resolver).
- ADR-0025's "don't write the target's existence at all" class (pre-release
  tool names bound for the update-own-tools registry) overrides everything
  else in this section unconditionally — no placeholder, no schema entry,
  nothing.
- Grandfathered exceptions (ADR-0007 precedent — no retroactive bulk fix):
  the committed public keys, the company identity module's work email, the
  three Linux hosts' literal hostnames, the herdr sidebar's fan-mark lookup
  table. Do not use these as precedent for adding new real values elsewhere.

### Hybrid translation (ADR-0002)
- **Keep working config files literal** and deploy them via `xdg.configFile` /
  `home.file` (the zsh modules, starship, alacritty, sheldon configs, ...).
  Do not rewrite battle-tested config into Nix DSL wholesale.
- **Use Nix DSL only where interpolation pays** — per-host/identity values, or
  where a `programs.*` module removes real boilerplate.
- Track literal configs worth nixifying later in the nixification roadmap doc.

### Shell-extension init (ADR-0005)
- `eval "$(tool init …)"` / `source <(tool …)` MUST gate only on binary
  existence (`command -v`), never on auth credentials (e.g. `GITHUB_TOKEN`).
  Suppress tool warnings with `2>/dev/null`, not by skipping the loader — a
  token-gated loader breaks in the token-less home-manager session.

### Git sync guards (herdr's parallel worktrees)
- The git module sets `pull.ff=only` / `fetch.prune` / `push.autoSetupRemote` /
  `rerere.enabled` / `merge.conflictStyle=zdiff3` machine-wide — herdr creates a
  worktree from the parent checkout's HEAD without fetching or setting an upstream.
- The repo-local `pre-commit` git hook blocks a direct commit on `main`/`master`.
  Bypass with `GIT_ALLOW_MAIN_COMMIT=1`, **not** `--no-verify` — `--no-verify`
  would also skip the chained repo-local pre-commit (other repos' ruff/mypy).
- `git prune-branches` (deployed to the user's local bin directory, resolved
  via git's `git-<subcommand>` mechanism, no alias) deletes local branches
  whose upstream is `[gone]`, after listing them and asking once. Full
  rationale: the git-sync operations doc.

### Escape-hatch scripts
- Keep the surviving scripts small and POSIX/bash-lint clean (shellcheck
  severity=error in CI). Support `--dry-run` where it makes sense.
- `install-packages.sh` installs the **system layer only** — user-space CLIs
  belong in `home/modules/packages.nix`, not apt.

### CI (nix-centric)
- The nix workflow runs `nix flake check` + a per-host activation build
  matrix, plus a rust job: crane package/clippy/rustfmt checks and
  `cargo test --workspace` (fixture oracles + the migration-allowlist audit,
  ADR-0024/#389).
- The general CI workflow is a slim shell pass: shellcheck the surviving
  scripts, the installer scripts' `--dry-run` paths, a zsh module syntax
  check, every script's `--selftest`, and a full-history gitleaks scan.

### Scope inventory (Claude Code only)
- A request with multiple items (a Tracking Issue with sub-issues, a bulleted
  ask) gets a `## 要求インベントリ` (requirement inventory) at the top of its
  plan: every item, verbatim, with an `Rn` id, before design starts.
- Every `Rn` gets a disposition: which stage implements it, or one of the
  closed tags `Blocked-Upstream:` / `Obsolete:` / `User-Excluded:`. Size,
  effort, or session length are never valid reasons to drop an item.
  A referenced Issue that isn't an implementation target gets
  `Reference-Only: #N — <reason>` instead of being silently ignored.
- The `PreToolUse`/`ExitPlanMode` hook (`plan-scope-gate.sh`) enforces this:
  it cross-checks referenced Issues' sub-issues (or unchecked task-list items
  as a fallback) against the plan's inventory, and separately checks the
  inventory section's internal consistency (every `Rn` has a disposition,
  tags are from the closed set, no duplicates). It calls no LLM. Full
  rationale + how-to: `docs/claude/scope-inventory.md`, skill:
  `config/claude/skills/scope-inventory/`.

### Pull request descriptions
- A coding task is **not done** until the PR exists: commit → push →
  `gh pr create` in one motion, without pausing to ask. The Stop hook
  (`G_pr` in pr-gate.sh) enforces this.
- Every PR body **must** either close an issue or say why there is none:
  - `Closes #<n>` — one line per issue (`Fixes`/`Resolves` and
    `owner/repo#<n>` work too). Without this, merging does not touch the issue
    and a finished piece of work sits open until someone re-triages it by hand —
    which is exactly how #28 and #29 survived months after being solved.
  - `No-Issue: <reason>` — when the work genuinely has no issue behind it
    (feature work born mid-session is the common case). This is a real escape
    hatch, not a formality: do not file a throwaway issue just to have a number.
- Keep the keyword **out of code spans and fences**. GitHub ignores
  `` `Closes #1` `` and anything inside ``` fences, so a quoted example does not
  link anything — and the gate deliberately reads the body the same way GitHub
  does.
- The `G_link` judgement in the PR-completion Stop hook blocks the Stop hook
  when neither is present. Rationale: the hook's design doc under
  `docs/claude/`.
- Closing keywords only fire when the PR targets the **default branch**. On a
  stacked PR the gate says so, but it will not stop you — carry the keyword on
  the stage that actually closes that issue (the `stacked-pr` skill, §6): each
  stage's `Closes #N` / `No-Issue:` line reflects what *that stage* completes,
  not the stack as a whole. GitHub auto-retargets a stage's base to the
  default branch once the stage below it merges, so a stage's own keyword
  fires once it lands — you don't need to close by hand unless a stage never
  merges on its own.
- **Dependent PRs go into a stacked PR**, not parallel PRs off `main`: when a
  later change references an earlier PR's output, or edits the same section
  of the same file, base it on the parent branch instead. Full rationale +
  how-to: the `stacked-pr` skill's design doc and the skill itself.
  A `Stack: <n>/<total> (base: #<parent>)` line goes next to `Closes #N` /
  `No-Issue:` when stacking; the PR-completion gate does not check it.
- Every PR body follows a 5-section skeleton (full rationale + how-to: the
  `pr-description` skill's design doc and the skill itself):
  ```
  Closes #N / No-Issue: <reason>

  ## 課題
  ## 解決策
  ## Before / After
  ## 検証
  ## 要確認   (omit the whole section if there is nothing)
  ```
- `## Before / After` needs one of: an uploaded image (`gh pr create|edit
  --attach` with an alt-text-tagged path, gh >= 2.99.0), a fenced code block
  under that heading for text-only diffs, or `No-Visual: <reason>` when the
  change has no visible effect (GUI/Web **and** terminal/TUI appearance both
  count as visible). The `G_visual` judgement in the same PR-completion gate
  blocks the Stop hook when none of the three is present — it rides the same
  terminal block as `G_link`, so both body fixes cost one round trip.

## Verification / Testing
- `nix flake check` — evaluates every host's activation package.
- `nix build .#homeConfigurations.<host>.activationPackage --no-link` — build a host.
- `hms` — canonical apply (pushed main); `hms .` applies the current
  checkout/worktree for pre-push verification (wraps switch + daemon-reload +
  fcitx5 restart; see `docs/operations.md`).
- `home-manager switch --flake .#"$(hostname)" -b backup` — the raw switch
  `hms` wraps (see runbook).
- Rollback via generations: `home-manager generations`, then `--rollback`.
- Provisioning procedures: `docs/cutover-runbook.md`.
- Routine flake update + which layer a new tool goes in: `docs/operations.md`.

## Scope boundaries
- Do **not** manage drivers, the display stack, or anything root-owned through
  Nix — that stays in the thin apt system layer.
- Do **not** re-introduce the procedural symlink/dev-tool/keybase installers;
  they were retired in Phase 4 (#218). Git history preserves them.
- Do **not** add a root-level `*.md` file outside README/CONTRIBUTING/CHANGELOG/
  AGENTS.md/CLAUDE.md/LICENSE* — deep documentation goes in `docs/` (ADR-0016).
