# macOS Setup Guide (altair)

Bring a macOS host (currently: **altair**, a 2022 M2 MacBook Air,
aarch64-darwin) up to the declarative home-manager environment. See
[`setup.md`](setup.md) for the Pop!_OS procedure and
[ADR-0018](adr/0018-darwin-host-and-homebrew-layer.md) /
[ADR-0019](adr/0019-star-codename-hosts-and-marker-resolution.md) for the
design rationale (Homebrew system layer, star-codename host resolution).

This machine ships with light day-to-day use, so the setup starts from a
factory reset rather than a careful migration — a real backup pass is not
required. If you do keep anything off-repo on this Mac, move it somewhere
else first; the next step erases it.

## 1. Prerequisites

- The M2 MacBook Air itself, plugged in and connected to the internet.
- Apple ID credentials (to sign back in after the erase).
- Access to an existing host that already has the master GPG key, for the
  signing-subkey provisioning in step 6.

## 2. Erase and update macOS

1. **System Settings → General → Transfer or Reset → Erase All Content and
   Settings.** Confirm and let the machine reboot into Setup Assistant.
2. Complete Setup Assistant (Wi-Fi, Apple ID, skip anything you don't need —
   Migration Assistant, iCloud features, etc. are all optional for this
   machine's role).
3. **System Settings → General → Software Update** → install the latest
   available macOS. Reboot if prompted, and re-check until there is nothing
   left to install.

## 3. Xcode Command Line Tools

Homebrew's installer (next step, via bootstrap.sh) triggers this
automatically, but installing it explicitly first avoids an interactive
GUI prompt interrupting an otherwise unattended bootstrap:

```bash
xcode-select --install
```

Follow the GUI prompt to completion before continuing.

## 4. Clone the repo and set the host marker

```bash
git clone https://github.com/tarotene/dotfiles.git ~/dotfiles
cd ~/dotfiles
mkdir -p ~/.config/dotfiles
echo altair > ~/.config/dotfiles/host
```

The marker is what `resolve_host()` (in `scripts/hms.sh` and
`bootstrap.sh`) reads to select the `altair` homeConfiguration — macOS's own
`hostname` is never consulted (ADR-0019). `home/hosts/altair.nix` declares
this same marker via `xdg.configFile`, so after the first successful
activation the file becomes home-manager-managed; the hand-placed copy above
only needs to exist long enough for that first activation to happen.

## 5. Bootstrap

```bash
./bootstrap.sh
```

This installs Nix (Determinate Systems installer), then Homebrew +
`packages/declarative/Brewfile` (`scripts/install-packages-darwin.sh`), then
builds and activates the `altair` homeConfiguration. The activation script
runs with `HOME_MANAGER_BACKUP_EXT=backup`, so the marker file you hand-placed
in step 4 gets backed up to `~/.config/dotfiles/host.backup` rather than
blocking activation on a file-already-exists conflict — you can delete that
`.backup` file once you've confirmed `~/.config/dotfiles/host` now reads
`altair` again (it will, via `xdg.configFile`).

Use `--dry-run` first if you want to preview the steps without changing
anything.

## 6. Git signing subkey (ADR-0003 model, ADR-0018 applies the same
   per-machine pattern to darwin)

On a host that already has the master key (e.g. an existing Pop!_OS host):

```bash
gpg --edit-key 1DCDC49510DCC9BF58C89751B7D596E9AA6F36E8
gpg> addkey
# Choose: (4) RSA (sign only), key size 4096, expires in 1y
gpg> save

# Export the new subkey (note its keygrip/fingerprint from `gpg -K` first).
# umask 077 keeps the file unreadable by other local users regardless of the
# shell's ambient umask; chmod 600 covers the case where a stale
# altair-sign.key with looser permissions already exists at this path.
(umask 077 && gpg --export-secret-subkeys <NEW_SUBKEY_FPR>! > altair-sign.key)
chmod 600 altair-sign.key
```

Move `altair-sign.key` to the Mac over a channel you trust (a USB drive is
fine — this is not a "real backup," it is a one-time key transfer). On
altair:

```bash
gpg --import altair-sign.key
gpg --edit-key <NEW_SUBKEY_FPR>   # trust → 5 (ultimate) → quit
```

Then:

1. Delete `altair-sign.key` from both machines once the import is confirmed
   (`gpg -K` on altair shows the new subkey).
2. Re-export the updated public key set from the machine that ran `addkey`
   and commit it under `keys/*.pub` (so a future fresh host can import it at
   activation time, same as the existing three hosts).
3. Edit `home/hosts/altair.nix`, replacing
   `programs.git.signing.key = "REPLACE_WITH_ALTAIR_SIGNING_SUBKEY_FINGERPRINT"`
   with the new subkey's fingerprint.
4. `hms .` from the `~/dotfiles` checkout to apply it.

## 7. Verify

```bash
hms                                      # resolves "altair" via the marker, applies cleanly
brew bundle check --file=packages/declarative/Brewfile
git log --show-signature -1
which bat rg fd nvim claude alacritty
alacritty --version                      # launches; FiraCode NF renders (Font Book → search "FiraCode Nerd Font")
launchctl list | grep git-audit-worktrees   # the launchd agent (ADR-0018) is loaded
./bootstrap.sh --dry-run                 # re-running bootstrap is a no-op, nothing destructive
```

A Claude Code hook that shells out to `flock` (e.g. the wrap-up inbox gate)
resolving without a "command not found" confirms `pkgs.flock` landed on PATH
(worktree.nix, darwin-only addition — ADR-0018).

`/plan-view` opens a dedicated Chrome window from Plan mode (#230 — the
darwin branch in `config/claude/hooks/plan-view.sh` was added without an
altair run to verify it):

```bash
plan-view --no-open --out /tmp/p.html ~/.claude/plans/<any-existing-plan>.md
open -a "Google Chrome" --args --app="file:///tmp/p.html"   # sanity-check
                                                              # the launch args
                                                              # in isolation
/plan-view                                                   # from a Claude
                                                              # Code session
                                                              # in Plan mode
```

Expect a new Chrome window (app mode, no tabs/toolbar) to open with the
rendered plan.

## 8. Known differences from the Linux hosts

- No IME daemon (fcitx5/mozc) — this machine uses macOS's own input method
  switching. If Japanese input is ever needed here, add it as a system
  Input Source, not through home-manager.
- `open`/`xdg-open` are **not** shadowed — the OS's own `/usr/bin/open`
  already returns immediately, so `scripts/detach-open.sh`'s COSMIC-specific
  foreground-blocking workaround does not apply and is not deployed here.
- Alacritty runs unwrapped (no nixGL) — macOS provides its own native GL
  stack.
- The stale-worktree audit runs as a `launchd` agent, not a `systemd --user`
  timer (same cadence: every 60s, `StartInterval`).
- System-layer packages are Homebrew (`packages/declarative/Brewfile`), not
  apt. `brew bundle check` is the drift-detection step; there is no
  automatic enforcement, matching the same non-automatic posture apt has on
  the Linux hosts.
