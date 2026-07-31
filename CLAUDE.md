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
| **User environment** (source of truth) | shell, git, terminal, user-space CLIs, fonts, prompts, per-user services, GPG agent, SOPS loader, GUI apps | home-manager (`flake.nix` + `home/`) | ADR-0001 |
| **System layer** (escape hatch) | anything needing root or a system service: build toolchain, cross C toolchain, `fcitx5`, `scdaemon`, login-shell fallback | `apt` via `scripts/install-packages.sh` + `packages/declarative/apt-packages.txt` | ADR-0001 |
| **Per-project runtimes** (escape hatch) | language toolchains, project-local versions | `mise` / `direnv` / `rustup` launchers (installed by home-manager; toolchains stay project-scoped) | ADR-0002 |

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
│   ├── git/, alacritty/, sheldon/, shell/, fcitx5/, environment.d/, ...
│   └── starship.toml
├── packages/declarative/
│   └── apt-packages.txt      # system-layer packages ONLY
├── scripts/                  # escape-hatch and diagnostic scripts
│   ├── install-packages.sh   # thin system-layer apt installer (#216)
│   ├── install-falcon-sensor.sh # company EDR agent installer
│   ├── fix-ssh-permissions.sh
│   ├── setup-sops-secrets.sh # host-local SOPS/.sops.yaml setup
│   ├── sops-secrets-env.sh   # SOPS runtime env helper (deployed to ~/.local/bin)
│   └── fcitx5-key-trace.pl   # fcitx5 trace redactor + trigger-key detector (#14)
├── keys/                     # committed public keys (non-secret), imported at activation
├── bootstrap.sh              # greenfield: Nix install → apt → home-manager switch
├── docs/
│   ├── adr/0001..0005        # architecture decision records
│   ├── operations.md         # routine flake update + tool-layer decision flow
│   ├── cutover-runbook.md    # per-host migration procedure
│   ├── ime-chrome-diagnosis.md  # fcitx5 trigger-key investigation record (#14)
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

## Development Rules

### Nix modules (`home/`)
- Modules are the source of truth. A host module imports `home/common.nix` plus
  exactly one identity module; `common.nix` imports everything under `modules/`.
- `homeConfigurations.<hostname>` in `flake.nix` is keyed by hostname so
  `home-manager switch --flake .#"$(hostname)"` auto-selects per machine.
- Identity-scoped values (git `user.name`/`email`, browser default) go in
  `identities/`; per-machine values (signing key bound to the host's YubiKey/[S]
  subkey) go in `hosts/`.
- Format with `nix fmt` (nixfmt-rfc-style).

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

## Verification / Testing
- `nix flake check` — evaluates every host's activation package.
- `nix build .#homeConfigurations.<host>.activationPackage --no-link` — build a host.
- `home-manager switch --flake .#"$(hostname)" -b backup` — apply (see runbook).
- Rollback via generations: `home-manager generations`, then `--rollback`.
- Provisioning procedures: `docs/cutover-runbook.md`.
- Routine flake update + which layer a new tool goes in: `docs/operations.md`.

## Scope boundaries
- Do **not** manage drivers, the display stack, or anything root-owned through
  Nix — that stays in the thin apt system layer.
- Do **not** re-introduce the procedural symlink/dev-tool/keybase installers;
  they were retired in Phase 4 (#218). Git history preserves them.
