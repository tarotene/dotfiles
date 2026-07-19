# Cutover runbook

Per-host migration from the procedural dotfiles to the home-manager environment.
Tracked by #217.

## Prerequisites

- Nix installed (the `bootstrap.sh` script handles this for greenfield hosts).
- The flake builds cleanly: `nix flake check` passes.
- You know your machine's hostname (`hostname` — it must match a key in
  `homeConfigurations` in `flake.nix`).

## Order of hosts

| Stage | Host (hostname) | Purpose |
|-------|------|---------|
| 1 | personal-pop (this PC) — rehearsal only | Validate the procedure in a VM (greenfield) before touching real hosts. No cutover on this PC at Stage 1. |
| 2 | company-pop-old | First real cutover. Existing on-disk [S] subkey is reused as-is. |
| 3 | company-pop-new | Greenfield — provision from bare metal via `bootstrap.sh`. New [S] subkey is cut on this host. |
| 4 | personal-pop | In-place cutover of this PC, after the procedure is proven on company hosts. |

Stages 2 and 4 follow the "Existing host cutover" procedure below. Stage 3
follows the "Greenfield host" procedure. Stage 1 is the VM rehearsal of Stage
3's bootstrap path.

## Existing host cutover (Stages 2 and 4)

### 1. Remove old procedural symlinks (historical)

> **Historical:** the legacy procedural layer (per-file symlinks driven by a
> config file, plus its symlink installer) was removed by the home-manager
> migration in Phase 4 (#218). See git history for the original script.
>
> On a host that was symlinked by the old procedural installer *before* the
> migration, remove the stale symlinks under `$HOME` that point back into the
> repo so home-manager can place its own links without collisions. On a
> greenfield host, or one already cut over, there is nothing to remove — skip to
> Step 2. (The Step 3 `-b backup` switch also backs up any leftover collisions.)

If the host has an ad-hoc native Claude Code install, remove it — see
[Removing an ad-hoc native Claude Code install](#removing-an-ad-hoc-native-claude-code-install)
below. This step is idempotent and safe to re-run on already-cut-over hosts,
because the native installer's self-update mechanism can recreate the shadow
long after the initial cutover.

### 2. Install system-layer packages

```bash
./scripts/install-packages.sh
```

### 3. First home-manager switch

Use `-b backup` so any remaining file collisions are backed up (renamed to
`*.bak`) rather than causing a failure:

```bash
nix run home-manager -- switch --flake .#"$(hostname)" -b backup
```

Check the output for `backing up` messages — each one is a file that existed
before home-manager tried to place its own version.

### 4. Switch to the Nix-provided zsh

Register `~/.nix-profile/bin/zsh` in `/etc/shells` if not already present,
then set it as your login shell (#245):

```bash
NIX_ZSH="$HOME/.nix-profile/bin/zsh"
grep -qxF "$NIX_ZSH" /etc/shells || echo "$NIX_ZSH" | sudo tee -a /etc/shells >/dev/null
chsh -s "$NIX_ZSH"
```

Log out and back in for the new login shell to take effect.

### 5. Verify

```bash
# Shell
zsh --version
starship --version
sheldon --version

# Git
git config user.name
git config user.signingkey

# GPG
gpg --card-status          # requires YubiKey inserted
git log --show-signature -1

# Packages
which bat ripgrep fd nvim claude
```

### 6. Clean up backups

After verifying everything works, remove the `.bak` files:

```bash
find "$HOME" -name '*.bak' -newer /tmp -print    # review first
find "$HOME" -name '*.bak' -newer /tmp -delete    # then delete
```

## Greenfield host (Stage 3)

A brand-new machine with no prior dotfiles:

```bash
# From a fresh Pop!_OS install:
curl -fsSL https://raw.githubusercontent.com/tarotene/dotfiles/main/bootstrap.sh | bash

# Then follow the printed "Next steps" (YubiKey / gpg --card-status, and the
# chsh command to switch to the Nix-provided zsh).
```

Or clone first and run locally:

```bash
git clone https://github.com/tarotene/dotfiles.git ~/dotfiles
cd ~/dotfiles
./bootstrap.sh
```

## Rollback

home-manager keeps every activation as a generation. To roll back:

```bash
# List generations
home-manager generations

# Switch to the previous generation
home-manager switch --flake .#"$(hostname)" --rollback
```

Or, to switch to a specific older generation:

```bash
/nix/var/nix/profiles/per-user/$USER/home-manager-<gen>-link/activate
```

## Post-cutover

All hosts are now cut over. Phase 4 (#218) retired the legacy procedural
installers (the symlink installer, the dev-tool installer, and their config
file — see git history) and replaced the old script-centric CI with nix-centric
checks (`nix flake check` + per-host activation build in `nix.yml`, plus a slim
shellcheck/dry-run pass in `ci.yml`).

## Removing an ad-hoc native Claude Code install

`claude-code` is installed declaratively via `home/modules/packages.nix`
(#265). A native install from Anthropic's official installer lives at
`~/.local/bin/claude` → `~/.local/share/claude/versions/<version>`, and
`~/.local/bin` precedes `~/.nix-profile/bin` on PATH (see
`config/zsh/modules/10-path.zsh`) — so a leftover native binary silently
shadows the Nix-managed one.

Note that Claude Code's own self-update mechanism will re-download the
latest native binary into `~/.local/share/claude/versions/` on startup as
long as either the `claude` symlink under `~/.local/bin/` or the
`~/.local/share/claude/` directory still exists. Removing just the symlink
is not enough; the versions directory must go too.

Keep `~/.claude` — that holds settings, memory, and history, which are
independent of the binary location:

```bash
rm -f ~/.local/bin/claude
rm -rf ~/.local/share/claude
hash -r   # or open a new shell
which claude                    # → ~/.nix-profile/bin/claude
readlink -f "$(which claude)"   # → /nix/store/…-claude-code-<ver>/bin/claude
```

If `which claude` still points into `~/.local/`, the native installer has
already re-populated it; re-run the removal and check whether a background
process or a shell alias is re-invoking the native installer.
