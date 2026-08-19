# dotfiles

Declarative, reproducible **user environment** for Pop!_OS, built as a
flake-based **standalone home-manager** configuration. One flake is the source
of truth across every host and identity, so a fresh machine comes up identically
with near-zero manual steps.

Migrated from a procedural shell-script installer to home-manager — see
[`CONTEXT.md`](CONTEXT.md) and the [ADRs](docs/adr/) for the charter.

## Quick start

### Greenfield (fresh Pop!_OS install)

```bash
# Bootstrap: install Nix (Determinate Systems), then run home-manager.
curl -fsSL https://raw.githubusercontent.com/tarotene/dotfiles/main/bootstrap.sh | bash

# Or clone first and run locally:
git clone https://github.com/tarotene/dotfiles.git ~/dotfiles
cd ~/dotfiles && ./bootstrap.sh
```

`bootstrap.sh` installs Nix, installs the system-layer apt packages, runs
`home-manager switch --flake .#"$(hostname)"`, and registers the Nix-provided
zsh in `/etc/shells`. It then prints the manual steps (YubiKey `gpg
--card-status`, `chsh` to the Nix zsh). See [`SETUP.md`](SETUP.md) for the
step-by-step guide and [`docs/cutover-runbook.md`](docs/cutover-runbook.md) for
migrating an existing host.

### Existing host

Follow the **Existing host cutover** procedure in
[`docs/cutover-runbook.md`](docs/cutover-runbook.md) (system packages →
`home-manager switch -b backup` → switch login shell → verify).

## Architecture: three layers

home-manager owns the user environment; two layers stay outside it as
deliberate escape hatches.

| Layer | Owns | Managed by |
|-------|------|------------|
| **User environment** (source of truth) | shell, git, terminal, user-space CLIs, fonts, prompt, GPG agent, SOPS loader, GUI apps | home-manager (`flake.nix` + `home/`) |
| **System layer** (escape hatch) | build/cross toolchain, `scdaemon`, fcitx5 *immodules*, login-shell fallback — anything needing root, a system service, or to be loaded into an apt-installed process | `apt` (`scripts/install-packages.sh` + `packages/declarative/apt-packages.txt`) |
| **Per-project runtimes** (escape hatch) | language toolchains, project-local versions | `mise` / `direnv` / `rustup` launchers (home-manager installs them; toolchains stay project-scoped) |

See [ADR-0001](docs/adr/0001-home-manager-as-source-of-truth.md) and
[ADR-0002](docs/adr/0002-runtimes-and-hybrid-translation.md) for the boundary
rationale.

## What's managed by home-manager

- **Shell** — Nix-provided zsh (primary login shell), Starship prompt, Sheldon
  plugin manager. Literal `config/zsh/modules/*.zsh` deployed via `xdg.configFile`
  (hybrid translation, ADR-0002). `$SHELL` pinned via both
  `home.sessionVariables` and `systemd.user.sessionVariables` so GUI terminals
  pick up the right shell.
- **Git & identity** — `programs.git`; `user.name`/`user.email` per identity
  (personal / company), signing key per host (bound to the machine's YubiKey /
  on-disk [S] subkey). Commit signing on by default; `gh` credential helper.
- **GPG / YubiKey** — `programs.gpg` + `services.gpg-agent` (GNOME pinentry, SSH
  agent support, `scdaemon` CCID, no pcscd). Public keys committed under `keys/`,
  imported at activation.
- **Secrets** — SOPS **runtime**-decrypted in the interactive shell only (no
  sops-nix). Host-local `~/.sops/.sops.yaml` + `~/.sops/.env`, set up by
  `scripts/setup-sops-secrets.sh`. See
  [ADR-0003](docs/adr/0003-secrets-and-identity.md) (and its Amendment).
- **Terminal & desktop** — Alacritty, FiraCode Nerd Font (`pkgs.nerd-fonts.fira-code`
  + `fonts.fontconfig`).
- **Input method** — fcitx5 + mozc from nixpkgs
  (`qt6Packages.fcitx5-with-addons`), plus the env/autostart/profile wiring. apt
  caps fcitx5 at 5.1.7, which predates the fix for the trigger-key defect in
  [`docs/ime-chrome-diagnosis.md`](docs/ime-chrome-diagnosis.md); only the
  client-side immodules stay apt. See ADR-0001's Amendment.
- **GUI apps** — Google Chrome, Slack, Zoom via home-manager. Chrome is set as
  the default browser on company hosts (`xdg.mimeApps`).
- **GL / EGL** — nix-built GUI apps get their driver from **nix's own mesa** via
  a per-package `nixGL` wrapper, because `/run/opengl-driver` is NixOS-only and
  the system mesa cannot be loaded into a nix process. Without it Alacritty does
  not start at all and Chrome/Slack silently fall back to software rendering.
  See [ADR-0006](docs/adr/0006-gl-for-nix-gui-apps.md).
- **AI tooling** — `claude-code` via nixpkgs (`home/modules/packages.nix`).
- **User-space CLIs** — neovim, ripgrep, fd, bat, zellij, gh, siketyan-ghr,
  git-interactive-rebase-tool, and more, all from nixpkgs.
- **Dev-runtime launchers** — `mise`, `direnv` (+ nix-direnv), `uv`, `deno`,
  `rustup`. Java/Go consolidate into mise; rustup stays the global Rust default
  for cross-compilation and C↔Rust FFI (ADR-0002).

## Repository layout

```
dotfiles/
├── flake.nix / flake.lock    # pinned nixpkgs + home-manager; homeConfigurations.<hostname>
├── home/                     # Identity / Instance two-layer modules
│   ├── common.nix
│   ├── identities/{personal,company}.nix
│   ├── hosts/{personal-pop,company-pop-old,company-pop-new}.nix
│   └── modules/{shell,git,gpg,secrets,packages,desktop,runtimes}.nix
├── config/                   # literal config, deployed verbatim
├── packages/declarative/apt-packages.txt   # system layer only
├── scripts/                  # escape-hatch + diagnostic scripts (install-packages, sops, ssh, fcitx5 trace)
├── keys/                     # committed public keys
├── bootstrap.sh              # greenfield entrypoint
└── docs/{adr,operations.md,cutover-runbook.md,nixification-roadmap.md,
         ime-chrome-diagnosis.md,falcon-sensor.md}
```

## Architecture Decision Records

- [ADR-0001](docs/adr/0001-home-manager-as-source-of-truth.md) — home-manager is the source of truth; apt + runtimes are escape hatches.
- [ADR-0002](docs/adr/0002-runtimes-and-hybrid-translation.md) — runtime consolidation + hybrid config translation.
- [ADR-0003](docs/adr/0003-secrets-and-identity.md) — secrets & identity (YubiKey-rooted, runtime SOPS). See the Amendment for the deployed model.
- [ADR-0004](docs/adr/0004-repo-identity-and-relocation.md) — repo identity & relocation.
- [ADR-0005](docs/adr/0005-shell-extension-init-no-auth-gate.md) — shell-extension init gates on binary existence, not auth.
- [ADR-0006](docs/adr/0006-gl-for-nix-gui-apps.md) — nix GUI apps carry their own GL stack (nixGL); the system graphics stack stays apt.

## Routine operations

Refresh the pinned inputs on a cadence (weekly is enough), and consult the
tool-layer decision flow before installing anything new — both in
[`docs/operations.md`](docs/operations.md):

```bash
nix flake update
nix flake check
home-manager switch --flake .#"$(hostname)" -b backup
```

## Rollback

home-manager keeps every activation as a generation:

```bash
home-manager generations                               # list
home-manager switch --flake .#"$(hostname)" --rollback # previous generation
```

## Limitations

- **One manual step per host**: identity is hardware-rooted, so inserting the
  YubiKey, importing/trusting keys (`gpg --card-status`, `gpg --import
  keys/*.pub`), and setting the per-host signing key cannot be declarative
  (ADR-0003).
- **`chsh` stays manual**: `bootstrap.sh` registers the Nix zsh in `/etc/shells`
  but does not change your login shell (interactive auth; can fail under
  `curl | bash`).
- **System layer is not reproducible**: apt packages are installed
  imperatively; only the *list* is version-controlled.
- **Per-project toolchains are out of scope**: home-manager installs the
  launchers (mise/direnv/rustup); the actual toolchain versions live per project.
- **Pop!_OS only**: hosts are Pop!_OS 24.04; other distros/macOS are not
  targeted.

## License

See [LICENSE](LICENSE) for details.
