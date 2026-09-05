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

Also work through
[Legacy artifact cleanup](#legacy-artifact-cleanup) below. Those are artifacts
the retired installers *already deployed*, which removing the installers did not
undo — a stale `~/.gitconfig` that overrides the declared credential helper, and
stale systemd user units. Both are silent, so check even on hosts that were cut
over long ago.

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

**`--rollback` re-executes the activation script baked into the target
generation, not the current one.** For most modules that is invisible — the
target generation's `home.file` / `home.packages` are exactly what you get.
But for the imperative `~/.claude/settings.json` merge (`registerHooks` /
`registerPermissions` / `syncStatusLine` in `home/modules/claude.nix`), it
means a rollback to a generation that predates a hook's declarative retirement
cannot retire it — the old activation never learned about the retirement.
`home.file` still removes the now-unmanaged script, so you can end up with a
`settings.json` entry pointing at a path that no longer exists (this is what
happened with the herdr-sidebar-metadata hooks; see issue #44 and
[`operations.md`](operations.md#checking-for-orphaned-hook-statusline-entries-after-a---rollback)
for the check to run afterward). Treat `--rollback` as an emergency measure to
undo a *recent* switch, not as a way to permanently retire a feature — retiring
permanently means adding to the retired list in Nix and doing a forward `hms`.

## Known noise: `reloadSystemd` and host-side XDG autostart failures

On Pop!_OS + COSMIC hosts, `home-manager switch` may print a wall of
red-flag-looking output during the `Activating reloadSystemd` step:

```
The user systemd session is degraded:
● app-hidpi\x2ddaemon@autostart.service          loaded failed failed
● app-nvidia\x2dsettings\x2dautostart@autostart… loaded failed failed
...
Attempting to reload services anyway...
```

This is harmless. The failing units are per-user XDG autostart wrappers
generated by `systemd-xdg-autostart-generator(8)` from `/etc/xdg/autostart/`
entries shipped by Pop!_OS packages (`system76-hidpi-daemon` etc.) that
require a GNOME session and exit 1 under COSMIC. No home-manager-managed
unit is involved — home-manager's `reloadSystemd` step lists **all** failed
user units unconditionally, without filtering to the ones it manages
(upstream: [home-manager#7557](https://github.com/nix-community/home-manager/issues/7557)).
Activation success/failure is unaffected; judge the switch by its own exit
status, not by this listing.

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

## Removing an ad-hoc native herdr install

`herdr` is installed declaratively via `home/modules/herdr.nix` (from a
`nixpkgs-unstable` overlay — ADR-0001 Amendment 2026-08, #42). Same PATH
shadowing trap as claude-code above (`~/.local/bin` precedes
`~/.nix-profile/bin`), but here `home.activation.quarantineSelfInstalledHerdr`
handles it automatically on every switch — it renames a stray real file at
`~/.local/bin/herdr` to `~/.local/bin/herdr.pre-nix` (a store symlink is left
alone; only a genuine self-installed binary is quarantined). No manual step
should be necessary; if `which herdr` still resolves into `~/.local/bin/herdr`
after a switch, check that the activation actually ran
(`home-manager generations` for the current one) rather than removing the file
by hand.

Unlike claude-code, herdr's own updater does not fight this: it detects a Nix
install and disables its self-update path (`herdr channel show` / `herdr
update` refuse with a message pointing at `nix profile upgrade` / the flake
input). There is no re-populating background process to race.

After the binary changes (a version bump or the first cutover to Nix), the
running `herdr server` and its TUI client still hold the old binary in memory
— restart from outside herdr per
[`operations.md`](operations.md#restarting-herdr-after-a-switch-that-changes-its-binary-or-hooks),
since it will otherwise drop the pane you are running the switch from.

### Claude integration hook (agent session restore)

`home.activation.installHerdrClaudeIntegration` runs `herdr integration install
claude` automatically on every switch when `~/.claude/hooks/herdr-agent-state.sh`
is missing — no manual step needed on a fresh host. This hook is what lets
herdr's native agent session restore (`[session] resume_agents_on_restore`,
on by default) reattach a `claude` pane to its prior conversation after
`herdr server` restarts; without it, restore only recreates the pane's layout
and cwd, spawning a plain shell instead. This repo ships `onboarding = false`
in `config/herdr/config.toml`, which also skips the onboarding flow that would
otherwise install the integration — the activation step exists specifically to
cover that gap. Verify with `herdr integration status` (`claude: installed`).

## Legacy artifact cleanup

Removing the legacy procedural installers (#218) did not remove what they had
already deployed. Every host that was managed by them before the migration still
carries debris that home-manager never placed and therefore never cleans up.
Two kinds have been found so far; both are silent, so check for them explicitly
rather than waiting to be told.

Run these on each migrated host. Both checks are idempotent and safe to re-run.

### Stale `~/.gitconfig` (#34)

The old layer wrote a `~/.gitconfig` pointing the GitHub credential helper at
the **apt** `gh`:

```
[credential "https://github.com"]
	helper =
	helper = !/usr/bin/gh auth git-credential
```

Git reads `~/.config/git/config` (XDG) *before* `~/.gitconfig`, and the later
file wins, so this quietly overrides the helper that `home/modules/git.nix`
declares — the Nix `gh` never gets asked. That is a direct ADR-0001 violation:
home-manager is supposed to be the source of truth for this value.

The second effect is nastier because it is invisible. Once `~/.gitconfig`
exists, `git config --global` addresses *that* file and stops seeing the XDG
config entirely, so any script reading a home-manager-declared value with
`--global` silently gets nothing. The `sign-prewarm` hook hit exactly this
during development and had to drop to a scope-less `git config --get`.

```bash
# Check
ls -l ~/.gitconfig                       # absent → nothing to do

# Back up, then remove
cp ~/.gitconfig ~/.gitconfig.pre-hm.bak  # only if it exists
rm ~/.gitconfig

# Verify: the helper must now resolve to the Nix gh, via the XDG config
git config --get credential."https://github.com".helper   # → !gh auth git-credential
git config --show-origin --get user.email                 # → …/.config/git/config
```

Do not use `git config --global --get` to verify — that is the scope this whole
step is about, and it will keep reading a file you just deleted (or claim the
value is unset). Use the scope-less form.

### Stale systemd user units (#10)

The symlink installer enabled a `fcitx5.service` user unit pointing into the old
repo layout (`config/systemd/user/`). That path no longer exists — fcitx5 is now
wired via XDG autostart in `home/modules/desktop.nix` (ADR-0001 Amendment) — so
the symlinks dangle:

```
~/.config/systemd/user/fcitx5.service
  -> ~/dotfiles/config/systemd/user/fcitx5.service          (dangling)
~/.config/systemd/user/graphical-session.target.wants/fcitx5.service
  -> ~/dotfiles/config/systemd/user/fcitx5.service          (dangling)
```

systemd reports the unit as `bad` yet `enabled`, and logs a failure on **every**
`home-manager switch` during `reloadSystemd`. Two reasons to clean it up rather
than tolerate it: the noise lands right next to the documented-harmless
`reloadSystemd` output (see "Known noise" above), which makes a real activation
failure easy to miss; and the unit is still *enabled*, so if a checkout at
`~/dotfiles` ever regains that path, systemd would start a second fcitx5 daemon
racing the XDG-autostart one (`app-fcitx5@autostart.service`).

```bash
# Check — a clean host lists only home-manager symlinks into /nix/store
ls -l ~/.config/systemd/user/
systemctl --user list-units --state=failed

# Clean up
systemctl --user disable fcitx5.service
rm -f ~/.config/systemd/user/fcitx5.service \
      ~/.config/systemd/user/graphical-session.target.wants/fcitx5.service
systemctl --user daemon-reload

# Verify: exactly one fcitx5 process, started from the XDG autostart unit
systemctl --user status app-fcitx5@autostart.service --no-pager
pgrep -c fcitx5    # → 1
```

### Status per host

| Host | `~/.gitconfig` | stale systemd units |
|---|---|---|
| `company-pop-new` | cleaned 2026-08-31 | already clean |
| `personal-pop` | unverified | present as of #10 |
| `company-pop-old` | unverified | unverified |
