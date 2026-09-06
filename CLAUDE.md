# dotfiles - Claude Context

## Project Overview

Declarative, flake-based **standalone home-manager** configuration for one
person's user environment across three Pop!_OS hosts (`personal-pop`,
`company-pop-old`, `company-pop-new`) and two identities (personal, company).

**Core purpose**: home-manager is the single source of truth for the user
environment, so a fresh machine is provisioned to an identical setup with
near-zero manual steps. Migrated from the old procedural shell-script installer
(epic #207). See `CONTEXT.md` and `docs/adr/` for the full charter.

## Three-layer model

| Layer | Owns | Managed by | ADR |
|-------|------|------------|-----|
| **User environment** (source of truth) | shell, git, terminal, user-space CLIs, fonts, prompts, per-user services, GPG agent, SOPS loader, GUI apps, **fcitx5 daemon + mozc**, **herdr (binary + sidebar config)** | home-manager (`flake.nix` + `home/`) | ADR-0001 (+ Amendment) |
| **System layer** (escape hatch) | root, a system service, kernel/driver integration, **or code loaded into an apt-installed process**: build toolchain, cross C toolchain, `scdaemon`, fcitx5 *immodules* (`fcitx5-frontend-all`), login-shell fallback | `apt` via `scripts/install-packages.sh` + `packages/declarative/apt-packages.txt` | ADR-0001 (+ Amendment) |
| **Per-project runtimes** (escape hatch) | language toolchains, project-local versions | `mise` / `direnv` / `rustup` launchers (installed by home-manager; toolchains stay project-scoped) | ADR-0002 |

Note on graphics: the driver stack itself is root-owned and stays in the system
layer, but nix GUI apps cannot use it — they load **nix's own mesa** through a
per-package `nixGL` wrapper in `home/modules/desktop.nix` (ADR-0006).

Note on `herdr` / `gh`: `herdr` is not yet in the pinned stable nixpkgs channel;
`gh` is present but version-capped (stable ships 2.96.0, `--attach` needs
>= 2.99.0). Both come from a single-package `nixpkgs-unstable` overlay in
`flake.nix` (ADR-0001 Amendment, #42 for herdr) — drop each package's overlay
entry once stable catches up. Unlike `nixgl`, this overlay does not need
`inputs.nixpkgs.follows`: both are only ever `exec`'d, never `dlopen`'d into
another package's process, so a second glibc in their closure is harmless.

## Project Structure

```
dotfiles/
├── flake.nix                 # inputs (nixpkgs + home-manager, pinned; nixpkgs-unstable
│                             #   is a herdr + gh escape hatch, ADR-0001 Amendment) +
│                             #   homeConfigurations.<hostname>
├── flake.lock
├── home/                     # home-manager modules (Identity / Instance two-layer)
│   ├── common.nix            # shared across every host; imports all modules/
│   ├── identities/           # identity-scoped (git identity, browser default)
│   │   ├── personal.nix
│   │   └── company.nix
│   ├── hosts/                # instance-scoped; imports common + one identity + per-host signing key
│   │   ├── personal-pop.nix
│   │   ├── company-pop-old.nix
│   │   └── company-pop-new.nix
│   └── modules/              # shell, git, gpg, secrets, packages, desktop, runtimes, herdr
├── config/                   # literal config files, deployed verbatim via xdg.configFile / home.file
│   ├── zsh/                  # zsh modules (loaded in numeric order)
│   ├── claude/               # Claude Code hooks: plan-review gate, wrap-up inbox,
│   │                         #   plan-view, pr-gate, issue-index, sign-prewarm,
│   │                         #   git-worktree-allow, git-stash-guard,
│   │                         #   herdr-sidebar-metadata + statusline + output
│   │                         #   schema + slash commands, claude-usage (herdr
│   │                         #   tab-bar command, not a hook)
│   ├── git/hooks/            # core.hooksPath targets: pre-push (worktree push guard),
│   │                         #   pre-commit (protected-branch guard, then chains to
│   │                         #   the repo-local hook), prune-branches.sh (helper for
│   │                         #   the `git prune-branches` alias)
│   ├── herdr/                # Herdr config.toml (theme + sidebar rows), fully managed —
│   │                         #   xdg.configFile deploys it verbatim (store symlink,
│   │                         #   read-only; in-app settings writes fail by design)
│   ├── git/, alacritty/, sheldon/, shell/, fcitx5/, environment.d/, ...
│   └── starship.toml
├── packages/declarative/
│   └── apt-packages.txt      # system-layer packages ONLY
├── scripts/                  # escape-hatch and diagnostic scripts
│   ├── hms.sh                # canonical apply wrapper (deployed to ~/.local/bin/hms):
│   │                         #   switch + daemon-reload + fcitx5 restart + verification
│   ├── install-packages.sh   # thin system-layer apt installer (#216)
│   ├── install-falcon-sensor.sh # company EDR agent installer
│   ├── fix-ssh-permissions.sh
│   ├── setup-sops-secrets.sh # host-local SOPS/.sops.yaml setup
│   ├── sops-secrets-env.sh   # SOPS runtime env helper (deployed to
│   │                         #   ~/.local/bin/sops-secrets-env, no .sh)
│   ├── detach-open.sh        # deployed as ~/.local/bin/open AND ~/.local/bin/xdg-open
│   │                         #   (shadows the system xdg-open, which blocks in the
│   │                         #   foreground on COSMIC), and as the $BROWSER target
│   ├── git-shelve             # worktree-tagged `git stash push` wrapper
│   │                         #   (deployed to ~/.local/bin/git-shelve, called as
│   │                         #   `git shelve` via git's subcommand resolution)
│   ├── git-unshelve           # resolves + applies + drops this worktree's own
│   │                         #   shelve entry (SHA-based, TOCTOU-safe drop)
│   └── fcitx5-key-trace.pl   # fcitx5 trace redactor + trigger-key detector (#14)
├── keys/                     # committed public keys (non-secret), imported at activation
├── bootstrap.sh              # greenfield: Nix install → apt → home-manager switch
├── docs/
│   ├── README.md             # index of everything below, by category
│   ├── adr/0001..0006        # architecture decision records
│   ├── operations.md         # the canonical apply (hms) + routine flake update + tool-layer decision flow
│   ├── cutover-runbook.md    # per-host migration procedure
│   ├── git-sync.md           # machine-wide git config + hooks guarding herdr's parallel worktrees
│   ├── ime-chrome-diagnosis.md  # fcitx5 trigger-key investigation record (#14)
│   ├── claude/               # Claude Code tooling docs (design + rationale per hook)
│   │   ├── copilot-plan-review.md  # Copilot plan-review gate: read-only custom agent, why it gates on severity, not on a verdict
│   │   ├── git-worktree-allow.md # PreToolUse hook: validated programmatic allow for `git -C <worktree>`
│   │   ├── git-stash-guard.md    # PreToolUse hook: deny bare `git stash` (shared stack across worktrees)
│   │   ├── issue-index.md        # SessionStart hook: inject an Issue index, not a full crawl
│   │   ├── pr-gate.md            # Stop hook: PR completion barrier (CI/push/issue-link/visual-evidence)
│   │   ├── pr-description.md     # PR body skeleton + mandatory Before/After
│   │   │                     #   visual evidence (gate: G_visual, skill: pr-description)
│   │   ├── sign-prewarm.md       # SessionStart hook: pre-warm the git-signing passphrase cache
│   │   ├── plan-view.md          # /plan-view: render the in-progress plan to HTML in Chrome
│   │   ├── wrapup-inbox.md       # Stop hook: out-of-scope findings → issue-filing inbox
│   │   ├── herdr-sidebar-metadata.md # Herdr sidebar: per-agent Claude mode/model/metrics via pane metadata
│   │   ├── claude-permissions.md # permissions.allow: declarative, idempotent jq merge like registerHooks
│   │   ├── opusplan-model-aliases.md # Opus Plan Mode はエイリアスのペア: opus を
│   │   │                     #   Fable 5 に差し替え、Plan 中だけ別モデルにする
│   │   └── claude-usage.md   # Herdr tab bar: 5h/weekly rate-limit usage +
│   │                         #   burn-rate prediction, from the undocumented /usage API
│   ├── falcon-sensor.md      # EDR agent notes
│   └── nixification-roadmap.md
└── .github/workflows/        # nix.yml (flake check + per-host build) + ci.yml (slim shellcheck)
```

## Architecture Decision Records

- **ADR-0001** — home-manager is the source of truth; apt + per-project runtimes are escape hatches.
- **ADR-0002** — runtime consolidation (Java/Go → mise; rustup/uv kept) + hybrid config translation.
- **ADR-0003** — secrets & identity: YubiKey-rooted, runtime-decrypted SOPS (no sops-nix). **See the Amendment** for the deployed model ([S] subkey on-disk per-machine, two identities, host-local `.sops.yaml`, migration ⊆ rotation).
- **ADR-0004** — repo identity & relocation (keep the `dotfiles` name; publish to public `tarotene/dotfiles` via clean orphan history; no semver releases).
- **ADR-0005** — shell-extension init gates on binary existence, never on auth credentials.
- **ADR-0006** — nix GUI apps carry their own GL stack: `/run/opengl-driver` is NixOS-only and the system mesa cannot be loaded into a nix process, so GL-using GUI packages are wrapped per-package with `nixGL` (nix's mesa). The system graphics stack stays untouched in apt.

## Development Rules

### Nix modules (`home/`)
- Modules are the source of truth. A host module imports `home/common.nix` plus
  exactly one identity module; `common.nix` imports everything under `modules/`.
- `homeConfigurations.<hostname>` in `flake.nix` is keyed by hostname so
  `home-manager switch --flake .#"$(hostname)"` auto-selects per machine.
- Identity-scoped values (git `user.name`/`email`, browser default) go in
  `identities/`; per-machine values (signing key bound to the host's YubiKey/[S]
  subkey) go in `hosts/`.
- Format with `nix fmt` (nixfmt-tree — a treefmt wrapper that feeds nixfmt only
  the `*.nix` files, so no arguments are needed).

### Hybrid translation (ADR-0002)
- **Keep working config files literal** and deploy them via `xdg.configFile` /
  `home.file` (the `config/zsh/modules/*.zsh`, `starship.toml`, `alacritty.toml`,
  `sheldon/plugins.toml`, ...). Do not rewrite battle-tested config into Nix DSL
  wholesale.
- **Use Nix DSL only where interpolation pays** — per-host/identity values, or
  where a `programs.*` module removes real boilerplate.
- Track literal configs worth nixifying later in `docs/nixification-roadmap.md`.

### Shell-extension init (ADR-0005)
- `eval "$(tool init …)"` / `source <(tool …)` MUST gate only on binary
  existence (`command -v`), never on auth credentials (e.g. `GITHUB_TOKEN`).
  Suppress tool warnings with `2>/dev/null`, not by skipping the loader — a
  token-gated loader breaks in the token-less home-manager session.

### Git sync guards (herdr's parallel worktrees)
- `home/modules/git.nix` sets `pull.ff=only` / `fetch.prune` / `push.autoSetupRemote` /
  `rerere.enabled` / `merge.conflictStyle=zdiff3` machine-wide — herdr creates a
  worktree from the parent checkout's HEAD without fetching or setting an upstream.
- `config/git/hooks/pre-commit` blocks a direct commit on `main`/`master`.
  Bypass with `GIT_ALLOW_MAIN_COMMIT=1`, **not** `--no-verify` — `--no-verify`
  would also skip the chained repo-local pre-commit (other repos' ruff/mypy).
- `git prune-branches` (alias → `config/git/hooks/prune-branches.sh`) deletes
  local branches whose upstream is `[gone]`, after listing them and asking once.
  Full rationale: `docs/git-sync.md`.

### Escape-hatch scripts
- Keep the surviving scripts small and POSIX/bash-lint clean (shellcheck
  severity=error in CI). Support `--dry-run` where it makes sense.
- `install-packages.sh` installs the **system layer only** — user-space CLIs
  belong in `home/modules/packages.nix`, not apt.

### CI (nix-centric)
- `nix.yml` runs `nix flake check` + a per-host activation build matrix.
- `ci.yml` is a slim shell pass: shellcheck the surviving scripts, `bootstrap.sh`
  + `install-packages.sh` `--dry-run`, and a zsh module syntax check.

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
- The `G_link` judgement in `config/claude/hooks/pr-gate.sh` blocks the Stop hook
  when neither is present. Rationale: `docs/claude/pr-gate.md`.
- Closing keywords only fire when the PR targets the **default branch**. On a
  stacked PR the gate says so, but it will not stop you — close the issue by
  hand, or carry the keyword on the PR that lands on `main`.
- Every PR body follows a 5-section skeleton (full rationale + how-to:
  `docs/claude/pr-description.md`, skill: `config/claude/skills/pr-description/`):
  ```
  Closes #N / No-Issue: <reason>

  ## 課題
  ## 解決策
  ## Before / After
  ## 検証
  ## 要確認   (omit the whole section if there is nothing)
  ```
- `## Before / After` needs one of: an uploaded image (`gh pr create|edit
  --attach './after.png#Alt'`, gh >= 2.99.0), a fenced code block under that
  heading for text-only diffs, or `No-Visual: <reason>` when the change has no
  visible effect (GUI/Web **and** terminal/TUI appearance both count as
  visible). The `G_visual` judgement in `pr-gate.sh` blocks the Stop hook when
  none of the three is present — it rides the same terminal block as `G_link`,
  so both body fixes cost one round trip.

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
