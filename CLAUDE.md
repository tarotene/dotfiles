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
| **User environment** (source of truth) | shell, git, terminal, user-space CLIs, fonts, prompts, per-user services, GPG agent, SOPS loader, GUI apps, **fcitx5 daemon + mozc** | home-manager (`flake.nix` + `home/`) | ADR-0001 (+ Amendment) |
| **System layer** (escape hatch) | root, a system service, kernel/driver integration, **or code loaded into an apt-installed process**: build toolchain, cross C toolchain, `scdaemon`, fcitx5 *immodules* (`fcitx5-frontend-all`), login-shell fallback | `apt` via `scripts/install-packages.sh` + `packages/declarative/apt-packages.txt` | ADR-0001 (+ Amendment) |
| **Per-project runtimes** (escape hatch) | language toolchains, project-local versions | `mise` / `direnv` / `rustup` launchers (installed by home-manager; toolchains stay project-scoped) | ADR-0002 |

Note on graphics: the driver stack itself is root-owned and stays in the system
layer, but nix GUI apps cannot use it — they load **nix's own mesa** through a
per-package `nixGL` wrapper in `home/modules/desktop.nix` (ADR-0006).

## Project Structure

```
dotfiles/
├── flake.nix                 # inputs (nixpkgs + home-manager, pinned) + homeConfigurations.<hostname>
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
│   └── modules/              # shell, git, gpg, secrets, packages, desktop, runtimes
├── config/                   # literal config files, deployed verbatim via xdg.configFile / home.file
│   ├── zsh/                  # zsh modules (loaded in numeric order)
│   ├── claude/               # Claude Code hooks: plan-review gate, wrap-up inbox,
│   │                         #   plan-view, pr-gate, issue-index, sign-prewarm,
│   │                         #   git-worktree-allow + output schema + slash commands
│   ├── git/hooks/            # core.hooksPath targets: pre-push (worktree push guard),
│   │                         #   pre-commit (chains to the repo-local hook)
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
│   ├── sops-secrets-env.sh   # SOPS runtime env helper (deployed to ~/.local/bin)
│   └── fcitx5-key-trace.pl   # fcitx5 trace redactor + trigger-key detector (#14)
├── keys/                     # committed public keys (non-secret), imported at activation
├── bootstrap.sh              # greenfield: Nix install → apt → home-manager switch
├── docs/
│   ├── README.md             # index of everything below, by category
│   ├── adr/0001..0006        # architecture decision records
│   ├── operations.md         # the canonical apply (hms) + routine flake update + tool-layer decision flow
│   ├── cutover-runbook.md    # per-host migration procedure
│   ├── ime-chrome-diagnosis.md  # fcitx5 trigger-key investigation record (#14)
│   ├── claude/               # Claude Code tooling docs (design + rationale per hook)
│   │   ├── codex-plan-review.md  # Codex plan-review gate: why it gates on severity, not on a verdict
│   │   ├── git-worktree-allow.md # PreToolUse hook: validated programmatic allow for `git -C <worktree>`
│   │   ├── issue-index.md        # SessionStart hook: inject an Issue index, not a full crawl
│   │   ├── pr-gate.md            # Stop hook: PR completion barrier (CI/push/issue-link)
│   │   ├── sign-prewarm.md       # SessionStart hook: pre-warm the git-signing passphrase cache
│   │   ├── plan-view.md          # /plan-view: render the in-progress plan to HTML in Chrome
│   │   ├── wrapup-inbox.md       # Stop hook: out-of-scope findings → issue-filing inbox
│   │   └── claude-permissions.md # permissions.allow: declarative, idempotent jq merge like registerHooks
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
