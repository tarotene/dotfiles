# dotfiles

Flake-based home-manager config that reproduces one user environment identically across Pop!_OS hosts.

## Background

home-manager is the single source of truth for the user environment; a thin apt layer and per-project runtime launchers (`mise`/`direnv`/`rustup`) stay outside it as deliberate escape hatches (ADR-0001, ADR-0002). The full architecture record lives in [`docs/adr/`](docs/adr/), indexed at [`docs/README.md`](docs/README.md).

Further reading: the [home-manager manual](https://nix-community.github.io/home-manager/) and its [standalone-flakes section](https://nix-community.github.io/home-manager/index.xhtml#sec-flakes-standalone); this repo's YubiKey + GPG operating model follows <https://fuwa.dev/posts/yubikey/>.

## Install

Greenfield (fresh Pop!_OS install):

```bash
curl -fsSL https://raw.githubusercontent.com/tarotene/dotfiles/main/bootstrap.sh | bash
```

or clone first and run locally:

```bash
git clone https://github.com/tarotene/dotfiles.git ~/dotfiles
cd ~/dotfiles && ./bootstrap.sh
```

`bootstrap.sh` installs Nix (Determinate Systems), the system-layer apt packages, runs `home-manager switch --flake .#"$(hostname)"`, and registers the Nix-provided zsh as a login shell option. It prints the remaining manual steps (YubiKey card verification, changing the shell). See [the step-by-step setup guide](docs/setup.md) and [the migration runbook](docs/cutover-runbook.md) for migrating an existing host.

## Usage

The canonical apply is `hms` (deployed to `~/.local/bin`): no arguments applies pushed `main`, `hms .` applies the current checkout for pre-push verification. It wraps the switch, the user `daemon-reload`, and the fcitx5 unit restart in one command.

```bash
nix flake update   # refresh pinned inputs (weekly cadence is enough)
nix flake check    # evaluate every host's activation package
hms .               # apply this checkout; commit + push once it proves out
```

Roll back to a previous generation at any time:

```bash
home-manager generations
home-manager switch --flake .#"$(hostname)" --rollback
```

Routine operations and the tool-layer decision flow (new CLI → home-manager package vs. apt vs. per-project runtime) are in [`docs/operations.md`](docs/operations.md).

## Scope

home-manager owns the user environment: shell, git, GPG, terminal, input method, GUI apps, and the Claude Code tooling under `config/claude/`. Two layers stay outside it as escape hatches — a thin apt system layer for anything needing root or to be loaded into an apt-installed process, and per-project language toolchains (`mise`/`direnv`/`rustup` launchers; the actual toolchain versions stay project-scoped).

This repository does not manage per-project language toolchains, the judgement engine behind `publish-guard` (lives in [tarotene/publish-guard](https://github.com/tarotene/publish-guard); this repo only wires it in), or feature development on the upstream tools it merely consumes (e.g. `herdr` — those live in their own repos).

Caveats: identity is hardware-rooted, so inserting the YubiKey and trusting keys cannot be declarative (ADR-0003). `chsh` stays manual (`bootstrap.sh` cannot reliably change the login shell under `curl | bash`). The apt system layer is not reproducible — only the package *list* is version-controlled. Hosts are Pop!_OS 24.04 (three machines) plus one macOS host, `altair` (ADR-0018); other Linux distros are not targeted.

## Development

```bash
nix fmt            # format all *.nix files (nixfmt-tree)
nix flake check    # evaluate every host's activation package
hms .               # build + activate this checkout for end-to-end verification
```

CI (`.github/workflows/`) runs a per-host activation build matrix plus a slim shellcheck pass over the surviving escape-hatch scripts.

## License

See [LICENSE](LICENSE) for details.
