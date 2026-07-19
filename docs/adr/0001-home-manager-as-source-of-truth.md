# ADR-0001 — home-manager is the source of truth; apt and per-project runtimes are escape hatches

- Status: Accepted
- Date: 2025
- Epic: #207

## Context

The current dotfiles are procedural shell scripts that symlink configs and shell
out to `apt` and `cargo`. There is no single source of truth, no pinning, and no
clean rollback. We want a fresh machine — personal or work — provisioned to an
identical user environment with near-zero manual steps.

We need to decide *what* owns the user environment, and where the boundaries
with the operating system sit.

## Decision

- **home-manager (standalone, flake-based) is the source of truth for the user
  environment.** Shell, git, terminal, user-space CLIs, fonts, prompts, and
  per-user services are declared in the flake.
- Two layers stay **outside** home-manager as deliberate escape hatches:
  - **System layer (`apt`)** — anything requiring root or a system service:
    kernel, drivers, display manager, input-method daemons (`fcitx5`).
    Slimmed to a system-only set in Phase 2 (#216).
  - **Per-project runtimes (`mise` / `direnv` / `rustup`)** — language
    toolchains and project-local versions. home-manager installs the launchers;
    toolchains stay project-scoped (see ADR-0002).
- **Host model:** two Pop!_OS machines. We do **not** assume root: the
  multi-user Nix install (daemon) is the default, with the single-user install
  documented as a fallback.
- **Infra:** pin to the latest **stable release** channel; install Nix with the
  Determinate Systems installer; nix-centric CI; rollback via home-manager
  generations (`home-manager switch -b backup`, `home-manager generations`).

## Consequences

- One declarative source builds the same user environment on every host.
- The OS install stays thin and replaceable; we never try to manage drivers or
  the display stack through Nix.
- Project toolchains remain fast to switch and do not bloat the global closure.
- Rollback is a generation switch, not a hand-written snapshot system.
