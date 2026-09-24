# Cutover runbook

Per-host migration from the procedural dotfiles to the home-manager environment.
Tracked by #217.

## Prerequisites

- Nix installed (the `bootstrap.sh` script handles this for greenfield hosts).
- The flake builds cleanly: `nix flake check` passes.
- You know your machine's hostname (`hostname` — it must match a key in
  `homeConfigurations` in `flake.nix`), or the logical name a
  `~/.config/dotfiles/host` marker resolves to (ADR-0019).

## Order of hosts

| Stage | Host (hostname) | Purpose |
|-------|------|---------|
| 1 | personal-pop (this PC) — rehearsal only | Validate the procedure in a VM (greenfield) before touching real hosts. No cutover on this PC at Stage 1. |
| 2 | company-pop-old | First real cutover. Existing on-disk [S] subkey is reused as-is. |
| 3 | company-pop-new | Greenfield — provision from bare metal via `bootstrap.sh`. New [S] subkey is cut on this host. |
| 4 | personal-pop | In-place cutover of this PC, after the procedure is proven on company hosts. |

Historical record of this table's own cutover — the names above are as they
were at the time. Current names (#214): `personal-pop` → `vega`,
`company-pop-new` → `arcturus`; `company-pop-old` was retired outright (see
"Renaming an existing host to a star codename" below for how the rename
itself is done).

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

If the host has no native Claude Code install yet, install one — see
[Installing Claude Code (native installer)](#installing-claude-code-native-installer)
below. This step is idempotent and safe to re-run on already-cut-over hosts.

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

## Renaming an existing host to a star codename

Following up after a host module gets renamed to a star codename (ADR-0019,
e.g. `personal-pop` → `vega`, #214) — for a Linux host that is already
provisioned and only needs to pick up its new name. macOS is out of scope
for this section: `altair` was greenfield-provisioned directly under its
star name (`docs/setup-macos.md`), and ADR-0019's Context 2 is exactly why
macOS never keys off `hostname` in the first place.

**The OS hostname changes too, as part of this same procedure.** ADR-0019's
"Alternatives considered"
(`docs/adr/0019-star-codename-hosts-and-marker-resolution.md`) rejected
*relying on* `scutil --set HostName` as the resolution mechanism on macOS —
not renaming the OS hostname on Linux. `resolve_host()` in `scripts/hms.sh` /
`bootstrap.sh` still reads the `dotfiles/host` marker first, but on Linux the
simplest way to make that marker resolve correctly *and* end up with a
`hostname` that matches is to change the OS hostname itself, then let the
marker fall out of that:

```bash
sudo hostnamectl hostname <star-name>   # e.g. vega — sets static, transient,
                                         # and pretty hostname together; no reboot needed
hostname                                # expect: <star-name>
grep -n '127\.0\.1\.1' /etc/hosts       # if this line exists, update it to <star-name> too
                                         # (Pop!_OS 24.04 has no such line by default —
                                         # NSS resolves the local hostname via `myhostname`)
hms
```

With no `dotfiles/host` marker yet, `resolve_host()` falls back to `hostname`
— now `<star-name>` — and selects `.#<star-name>`. `home/hosts/<star-name>.nix`'s
own `xdg.configFile."dotfiles/host"` declaration then deploys the marker on
this same switch, so from here on the marker is the resolution's source of
truth and the OS hostname is a (now-matching) display name kept in sync by
this procedure, not by any ongoing mechanism.

If a marker was already hand-placed on this host before this procedure
existed (true for `vega` and `arcturus` as of #214/#422 — the marker already
resolves to the new name, only the OS hostname is still the old one), only
the `hostnamectl` step above is needed; `hms` will pick up the OS hostname
change but the marker (already home-manager-managed) does not change.

Verify:

```bash
hostname                               # expect: <star-name>
dotfiles-doctor                        # expect: OK   host: <star-name>
readlink -f ~/.config/dotfiles/host    # expect: a /nix/store/... path
```

Clean up: if this host had a *hand-placed* marker file predating this switch
(the greenfield-style bootstrap, not the `hostnamectl` step above), `-b
backup` moves it aside to `~/.config/dotfiles/host.backup` rather than
failing the switch (same `.backup` clobber mechanics as
[`docs/operations.md`](operations.md#hms-fails-at-checklinktargets-with-a-backup-clobber-error));
delete it once the `dotfiles-doctor` check above passes.

Side effects of the OS hostname change to expect, not to troubleshoot: the
starship/herdr hostname segments pick up the new name on the next shell or
herdr restart; the host's mDNS name becomes `<star-name>.local`; and on
`arcturus`, the terminal name shown in the Falcon Console changes (mention
this to IT if they track terminals by name). `crates/detect-drift` reads
`/etc/hostname` directly, so its next drift-report Issue title also switches
to the new name.

`scripts/install-falcon-sensor.sh`'s `TARGET_HOST` and the rest of
`docs/falcon-sensor.md` are kept in sync with the current name (both track
`arcturus` as of this rename) — the installer's own OS-hostname check is
*not* relaxed, since it is exactly what tells you to run this procedure
first if you try to run it on a not-yet-renamed host.

Once every host's OS hostname has been renamed via this procedure, the
migration-era `homeConfigurations` aliases for the old names (`flake.nix`,
currently `personal-pop` → `vega` and `company-pop-new` → `arcturus`) can be
removed — they exist only as a safety net for a host that has not yet
renamed its OS hostname (so `hms`'s `hostname` fallback still resolves under
the old key).

See also [`docs/setup-macos.md`](setup-macos.md#4-clone-the-repo-and-set-the-host-marker)
for the greenfield marker-then-switch sequence used on a brand-new macOS
host, which does not apply here.

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

On Pop!_OS + COSMIC hosts, `home-manager switch` used to print a wall of
red-flag-looking output during the `Activating reloadSystemd` step:

```
The user systemd session is degraded:
● app-hidpi\x2ddaemon@autostart.service          loaded failed failed
● app-nvidia\x2dsettings\x2dautostart@autostart… loaded failed failed
...
Attempting to reload services anyway...
```

The failing units were per-user XDG autostart wrappers generated by
`systemd-xdg-autostart-generator(8)` from `/etc/xdg/autostart/` entries
shipped by Pop!_OS packages (`system76-hidpi-daemon`, `nvidia-settings`)
that require a GNOME/X11 session or a loaded NVIDIA driver — none of which
apply on any of the three hosts. `home/modules/desktop.nix` now ships a
`Hidden=true` override for each of the three entries
(`hidpi-daemon`/`hidpi-frontend`/`nvidia-settings-autostart`) under
`~/.config/autostart/`, per the XDG Autostart Specification's own
mechanism for disabling a system-wide entry — the generator produces no
unit for a hidden entry at all, so the units no longer exist to fail.

**One-time step after the first switch that ships this override** (not
needed again — the override is a permanent home-manager-managed file):

```bash
systemctl --user daemon-reload     # hms already runs this
systemctl --user reset-failed      # clear the still-remembered failed state
systemctl --user is-system-running # → running
```

A normal logout/login also clears the remembered failed state, without
running the commands above.

This was harmless even before the override: no home-manager-managed unit
was ever involved. home-manager's `reloadSystemd` step lists **all**
failed user units unconditionally, without filtering to the ones it
manages (upstream: [home-manager#7557](https://github.com/nix-community/home-manager/issues/7557)),
so a *different* failing autostart entry — host-side or newly added —
would reproduce the same "degraded" listing. Judge a switch by its own
exit status, not by this listing.

## Post-cutover

All hosts are now cut over. Phase 4 (#218) retired the legacy procedural
installers (the symlink installer, the dev-tool installer, and their config
file — see git history) and replaced the old script-centric CI with nix-centric
checks (`nix flake check` + per-host activation build in `nix.yml`, plus a slim
shellcheck/dry-run pass in `ci.yml`).

### Granting `altair`'s `~/Downloads` cleanup access to Full Disk Access

`home/modules/downloads.nix` deploys a `launchd` agent
(`downloads-clean`) that runs `/usr/bin/find` against `~/Downloads` daily.
macOS's TCC (Transparency, Consent and Control) subsystem blocks a
`launchd` agent from touching a protected user folder even though the same
command works fine from Terminal — Terminal already holds the grant, and a
`launchd`-spawned process does not inherit it. There is no way to declare
this grant from Nix; it must be added by hand once per machine:

1. **System Settings → Privacy & Security → Full Disk Access.**
2. Click **+**, press **⌘⇧G**, and enter `/usr/bin/find` to add it.
3. Toggle it on.

Before this grant, `launchctl kickstart -k
gui/$(id -u)/org.nix-community.home.downloads-clean` (or the timer firing on
its own) fails silently with `Operation not permitted` and nothing under
`~/Downloads` gets cleaned up.

## Installing Claude Code (native installer)

Claude Code is **not** installed via `home/modules/packages.nix` — nixpkgs'
`claude-code` trails upstream by dozens of patch releases, which does not fit
a tool whose model catalog changes underneath it (ADR-457). The native
installer is the source of truth on every host:

```bash
curl -fsSL https://claude.ai/install.sh | bash
```

This places `~/.local/bin/claude` as a symlink into
`~/.local/share/claude/versions/<version>`. `~/.local/bin` precedes
`~/.nix-profile/bin` on PATH (see `config/zsh/modules/10-path.zsh` and
`config/shell/profile`) — this is the intended shadow, not drift
(`docs/operations.md`'s Ad-hoc installers section).

Background auto-update is disabled by declaration
(`home.sessionVariables.DISABLE_AUTOUPDATER`, `home/modules/claude.nix`) — the
only update path is running `claude update` yourself. The zsh function in
`config/zsh/modules/53-tools-claude.zsh` wraps `update`/`upgrade` and runs
`claude-plan-model sync` right after, so the Opus Plan Mode model pin never
trails the installed binary's catalog (ADR-457).

If `which claude` resolves into `~/.nix-profile/bin/` instead, that host
still has the retired nixpkgs `claude-code` in its generation — run `hms`
to pick up its removal, then re-open the shell.

## Removing an ad-hoc native herdr install

`herdr` is installed declaratively via `home/modules/herdr.nix` (from a
`nixpkgs-unstable` overlay — ADR-0001 Amendment 2026-08, #42), unlike claude
above (ADR-457's scoped exception): herdr's Nix package is meant to win over
any ad-hoc install, not lose to one. Same `~/.local/bin` precedes
`~/.nix-profile/bin` PATH position as the claude case, but here
`home.activation.quarantineSelfInstalledHerdr` handles the opposite direction
automatically on every switch — it renames a stray real file at
`~/.local/bin/herdr` to `~/.local/bin/herdr.pre-nix` (a store symlink is left
alone; only a genuine self-installed binary is quarantined). No manual step
should be necessary; if `which herdr` still resolves into `~/.local/bin/herdr`
after a switch, check that the activation actually ran
(`home-manager generations` for the current one) rather than removing the file
by hand.

Unlike claude, herdr's own updater does not fight this: it detects a Nix
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
