# Operations

Routine care of the declarative environment. For provisioning and migration,
see [`cutover-runbook.md`](cutover-runbook.md).

## Applying the configuration

The canonical apply is `hms` (from `scripts/hms.sh`, deployed to
`~/.local/bin` by `home/modules/packages.nix`), runnable from any directory:

```bash
hms          # apply pushed main (github:tarotene/dotfiles) — the default
hms .        # apply the current checkout/worktree (pre-push verification)
hms <path>   # apply an arbitrary local checkout
```

One command covers the whole apply runbook: the switch itself (with
`-b backup`), the user `daemon-reload`, the fcitx5 unit restart, and the
verification that the running fcitx5 matches the new store path (see
[the fcitx5 section](#fcitx5-needs-an-explicit-unit-restart-after-a-switch)
for why that restart is load-bearing).

The default deliberately references the **remote** main, not a local checkout
path: a checkout is whatever branch it happens to be on (the main checkout
regularly sits on a feature branch), so a path reference is an implicit
branch dependency. Applying a worktree or checkout is legitimate for
pre-push verification — but only ever explicitly, as `hms .`.

For a remote ref, `hms` forces `nix flake metadata --refresh` before the
switch and prints the resolved revision (`==> applying revision <rev>`) —
without it, nix's `tarball-ttl` cache (1h by default) can make `hms` silently
apply an hour-old main right after a merge, and still print `Done.` as if
nothing were wrong. `hms .` skips this — a local path always reads the
current tree, so there is nothing to refresh.

For a local ref, `hms` instead passes `--option warn-dirty false` to the
underlying `home-manager switch` — a local checkout's git tree is routinely
dirty mid-session (that's the point of `hms .`), and without this nix repeats
`warning: Git tree '<path>' has uncommitted changes` on every switch. The
suppression is local-path-only, not a machine-wide `nix.conf` setting (#149).

## Host-local marker files

Three `${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/<name>` marker files let
this PUBLIC repository's source stay unaware of host-specific or private
values (ADR-0034 D5: the *schema* is public, the *values* are not).
`private-hub` and `style-hub` are never home-manager-managed — declaring
either there would put the value back into a managed, store-symlinked file,
defeating the indirection — so they stay hand-placed indefinitely. `host` is
different: each star-codename host module declares its own marker via
`xdg.configFile."dotfiles/host"` (ADR-0019 D3), so after a host's first
switch under its new name, that declaration is the marker's source of
truth. On Linux it doesn't even need hand-placing: the rename runbook (see
[`cutover-runbook.md`](cutover-runbook.md#renaming-an-existing-host-to-a-star-codename))
sets the OS hostname to the new name first, so `resolve_host()`'s
`hostname` fallback already resolves correctly on the switch that deploys
the marker. Hand-placing is only needed on a host where the OS hostname
cannot (or should not yet) change — macOS (`altair`'s greenfield bootstrap),
or a Linux host being switched before its OS hostname is renamed:

| marker | consumer | required? | fallback when unset |
|---|---|---|---|
| `host` | `scripts/hms.sh`'s `resolve_host()`, `bootstrap.sh` (ADR-0019) | optional; needs hand-placing only when the OS hostname doesn't already match, home-manager-managed after the first switch under the new name | `hostname` |
| `private-hub` | `scripts/hms.sh`'s `resolve_default_ref()` (ADR-0034) | optional, always hand-placed | `github:tarotene/dotfiles` (public-only apply) |
| `style-hub` | `scripts/writing-style-hub`, for the `writing-style` skill (#115) | required for that skill, always hand-placed | `$WRITING_STYLE_HUB` env var only; otherwise the skill is unusable |

Run `dotfiles-doctor` (deployed to `~/.local/bin` by
`home/modules/packages.nix`) to see the current status of all three at
once — it reports each marker's presence and, for `style-hub`, whether it
actually resolves (via `writing-style-hub`), without ever writing a value
itself (#368). `host`/`private-hub` being unset is reported as `INFO`, not
an error — it's expected before a host's first switch under a star codename,
or for a host that has not been renamed yet (`flake.nix` still carries
migration-era `homeConfigurations` aliases for those, ADR-0019 Amendment).
Only `style-hub` being unresolved is reported as `WARN` (exit 1).

## Routine flake update

Backports to the pinned stable nixpkgs channel are best-effort and batched
upstream, so `flake.lock` drifts silently unless refreshed on a cadence —
weekly is enough:

```bash
nix flake update
nix flake check
hms .        # apply this checkout; commit + push once it proves out
```

To update a single input only:

```bash
nix flake update nixpkgs
```

Notes:

- Nix ≥ 2.19 removed `--update-input` / `--recreate-lock-file`; the positional
  form above is the only syntax. `nix flake lock` no longer updates existing
  inputs — it only creates missing locks.
- If the switch regresses, roll back via generations (see
  [`cutover-runbook.md`](cutover-runbook.md#rollback)).
- **`renovate.json`'s `nix` manager** (beta, opt-in;
  <https://docs.renovatebot.com/modules/manager/nix/>) opens a weekly
  `flake.lock` PR via `lockFileMaintenance`, so drift no longer accumulates
  silently between manual runs of the command above — this manual routine is
  now the fallback for out-of-cadence bumps (a specific input regressing, or
  wanting an update sooner than the weekly PR), not the sole mechanism
  ([#3](https://github.com/tarotene/dotfiles/issues/3)). The GitHub App
  token + `DeterminateSystems/update-flake-lock` route #3 originally
  proposed was dropped in favor of Renovate, which is already installed on
  this account and needs no new App/secrets: see #3's resolution comment.
  Review and merge the Renovate PR the same way as any other — `nix.yml`'s
  CI still gates it.
- **`nixpkgs-unstable` moves faster than the pinned stable channel it sits
  beside** (ADR-0001 Amendment 2026-08 for `herdr`, 2026-09 for `gh`). Bump it
  explicitly and separately when regressions land there — `nix flake update
  nixpkgs-unstable` — rather than assuming the weekly `nix flake update` sweep
  is safe for both channels at once. This bump now moves **both** `herdr` and
  `gh` together (same overlay entry, same input) — a `gh` regression from
  unstable is higher-stakes than it looks, since `pr-gate.sh` / `issue-index.sh`
  / `wrapup-stop-gate.sh` all shell out to `gh` unconditionally. If either
  package regresses after an update, roll back just that input by reverting
  `flake.lock`'s `nixpkgs-unstable` node (or the whole generation, per the
  rollback note above) — there is no way to roll back only one of the two
  packages while keeping the other's update, since they share a single input.
- `herdr`'s overlay entry also carries `patches/herdr-worktree-names.patch`
  (a personal-taste patch renaming generated worktrees after hololive
  talents instead of the built-in adjective-noun list) via `overrideAttrs`.
  This forces `herdr` to build from source locally instead of fetching a
  binary — a `nixpkgs-unstable` bump can shift `src/worktree.rs` enough for
  the patch to stop applying, which fails the build loudly (not silently);
  the fix is to regenerate the patch against the new source. Drop the patch
  once [herdrdev/herdr#4374](https://github.com/herdrdev/herdr/issues/4374)
  (word list configurable via `config.toml`) lands upstream.

### Restarting herdr after a switch that changes its binary or hooks

`herdr` (nix-managed, `home/modules/herdr.nix`) is not restarted by
`home-manager switch` — the running `herdr server` and its TUI client keep the
old binary in memory until you kill and relaunch them. Do this from a plain
terminal, **not from inside herdr itself**: it will drop every pane it is
managing, including the one you are running the switch from.

```bash
hms .                       # or hms, once the change is on main
herdr server stop           # from outside herdr — this ends live agent sessions
herdr                       # relaunch; client/server versions must match (wire
                             # protocol is version-gated)
```

Since the binary comes from a pin, client and server always match after a
switch — there is no risk of relaunching a mismatched pair, unlike an
in-place `herdr update` against a moving install.

`hms` itself warns (never fails) when it detects this: after a successful
switch it compares the store path the running `herdr server` process resolves
to against the store path the new generation's `herdr` points at, and prints
the restart instructions above if they differ (#200).

### `hms` fails at `checkLinkTargets` with a `.backup` clobber error

```
Existing file '/home/tarotene/.config/<something>.backup' would be
clobbered by backing up '/home/tarotene/.config/<something>'
```

This is `home-manager switch -b backup` refusing to activate because a
retreat path from a *previous* switch is still sitting there when the current
one goes to write a fresh one. It fires whenever a file newly taken under
home-manager management (like `herdr/config.toml` in #57) already exists as a
real file on disk with a stale `.backup` next to it — `checkLinkTargets` runs
before `writeBoundary`, so an `entryAfter [ "writeBoundary" ]` quarantine (the
DAG position used elsewhere, e.g. `quarantineStrayFcitx5Autostart`) never gets
a chance to clear the path first. `home/modules/quarantine.nix` (#64) is the
shared fix: add the new file's `$HOME`-relative path to
`dotfiles.quarantine.managedFiles` and its `entryBefore [ "checkLinkTargets" ]`
activation script moves both the real file and its stale `.backup` out of the
way before the check runs — see `home/modules/herdr.nix` for a module using
it.

### Checking for orphaned hook / statusLine entries after a `--rollback`

`registerHooks` / `syncStatusLine`'s declarative retirement
(`retiredHookEntries` / `retiredStatusLineCommands` in `home/modules/claude.nix`)
only runs as part of the activation script baked into a given home-manager
generation. A **forward** `hms` picks it up; a `home-manager switch --rollback`
to a generation that predates the retirement re-executes *that generation's*
(older) activation, which cannot retire anything it does not know about. If you
roll back across a boundary where a hook or `statusLine` command was added and
later retired, `home.file` will remove the now-unmanaged script but the
`~/.claude/settings.json` entry pointing at it can survive — the exact ENOENT /
broken-status-line symptom issue #44 diagnosed (see
[`claude-permissions.md`](claude/claude-permissions.md) and
[`herdr-sidebar-metadata.md`](claude/herdr-sidebar-metadata.md) for the
mechanism). Treat `--rollback` as an emergency measure, not a way to retire a
feature permanently — retiring permanently means adding to the retired list and
doing a forward `hms`, not rolling back.

After any emergency rollback, check for orphans:

```bash
jq -r '.hooks[]?[]? | .hooks[]? | .command' ~/.claude/settings.json |
  sed -n "s/^bash '\([^']*\)'.*/\1/p" |
  while read -r p; do [ -e "$p" ] || echo "orphan hook: $p"; done

p="$(jq -r '.statusLine.command // ""' ~/.claude/settings.json |
  sed -n "s/^bash '\([^']*\)'.*/\1/p")"
[ -z "$p" ] || [ -e "$p" ] || echo "orphan statusLine: $p"
```

If either script reports an orphan, the fix is a forward `hms .` on the
checkout that has the retirement — not another rollback.

### fcitx5 needs an explicit unit restart after a switch

`app-fcitx5@autostart.service` is **generated** from
`~/.config/autostart/fcitx5.desktop` by `systemd-xdg-autostart-generator`, and its
`Exec=` is a store path. `home-manager switch` runs `daemon-reload` but does **not**
restart a generated unit, so after any switch that moves fcitx5 the old binary is
still running. `hms` performs the restart and verification automatically; the
manual sequence it encodes is:

```bash
home-manager switch --flake .#"$(hostname)" -b backup
systemctl --user daemon-reload
systemctl --user restart app-fcitx5@autostart.service
systemctl --user cat app-fcitx5@autostart.service | grep ExecStart   # expect the new store path
readlink /proc/"$(pgrep -x fcitx5)"/exe                              # and the running process
```

Do **not** use `fcitx5 -r` to pick up the change: `-r` makes the unit's ExecStart
process exit, leaving the unit inactive with a daemon outside it. Details and the
recovery path are in [`ime-chrome-diagnosis.md`](ime-chrome-diagnosis.md).

Because the input method is now pinned rather than distro-supplied, it is only as
current as the flake — which is the point (apt was stuck two years behind a fix),
but it makes the update cadence above load-bearing for Japanese input.

## Rotating a machine-local GPG [S] signing subkey

`scripts/gpg-subkey` (deployed to `~/.local/bin/gpg-subkey`) generates and
rotates the on-disk `[S]` subkey each identity's primary card-backed key
signs Git commits with (ADR-0003 Amendment 2). It was absorbed from the
now-archived private predecessor tool, stripped of that tool's
git-config-writing side effect: `programs.git.signing.key` in
`home/hosts/<host>.nix` is the sole declared source of truth for which
subkey Git actually uses, so this tool never touches git config — it only
prints the next manual step. A daily `systemd.user.timer`
(`gpg-subkey-remind`, `home/modules/gpg.nix`) checks every `[S]` subkey's
expiry and notifies through Herdr (same channel as
`git-audit-worktrees` — not a desktop-notification tool, since nothing else
in this repo declares one) once it is within 30 days of expiring.

"Rotate" means generate a new subkey and revoke the old one — never extend
one subkey's own expiry in place (ADR-0003 Amendment 3). The local
passphrase protecting each generated `[S]` may be reused across rotations
unless it is itself suspected of compromise (same Amendment, grounded in
NIST SP 800-63B's guidance against fixed-schedule secret rotation).

Full rotation sequence (needs the identity's YubiKey inserted — the
primary's `[C]` capability signs the new subkey's binding signature):

```bash
gpg-subkey status                                    # see every [S] subkey and its remaining days
gpg-subkey rotate --key <primary-fpr> --revoke-old    # touch/PIN prompt via pinentry
gpg-subkey export --repo <dotfiles-checkout> --identity <personal|company>
```

`export` re-exports `keys/<identity>.pub` from the local keyring and prints
the new subkey ID to put in `programs.git.signing.key`:

1. Edit `home/hosts/<host>.nix`: `programs.git.signing.key = "<new-subkey-id>";`
2. Commit `keys/<identity>.pub` + the host module together, PR, merge.
3. `hms .` (or `hms` after merge) to apply.

`gpg-subkey remind --threshold 30 --notify` is what the timer runs; run it
by hand to check without waiting for the timer, or `gpg-subkey status` for a
read-only listing with no exit-code side effect.

If `--revoke-old` was skipped (or a rotate failed partway) and `gpg-subkey
status` shows more than one non-revoked on-disk subkey of the same usage,
converge it without generating yet another new key:

```bash
gpg-subkey revoke --key <primary-fpr> --subkey <old-keyid>    # touch/PIN prompt via pinentry
gpg-subkey export --repo <dotfiles-checkout> --identity <personal|company>
gpg-subkey sync --repo <dotfiles-checkout> --identity <personal|company> --fix --yes
```

`revoke` refuses card-backed/stub subkeys the same way `rotate` does (§ above
— that residency is intentional, ADR-0003 Amendment 4, not something to
revoke). A revoke changes the key material, so `sync` will report fresh
GitHub/keyserver drift right after — re-run it until it reports none. See
the `gpg-subkey-rotation` Claude Code skill for the full step-by-step
completion checklist this section maps to.

### Keeping GitHub / keys.openpgp.org in sync after a rotation

`rotate`'s generate+revoke is the one irreversible step in this whole
sequence; the repo's `keys/<identity>.pub`, GitHub's registered GPG key, and
keys.openpgp.org's published copy are all just downstream *copies* of that
local keyring state, and they can drift independently of it and of each
other — this happened for real once (a `gh` CLI quoting bug silently failed
a re-upload, leaving GitHub showing "Unverified" on new commits until
noticed by hand). There is no shared transaction across GnuPG + the GitHub
API + an independent keyserver, so instead of trying to force one, run:

```bash
gpg-subkey sync --repo <dotfiles-checkout> --identity <personal|company>          # report drift
gpg-subkey sync --repo <dotfiles-checkout> --identity <personal|company> --fix    # converge it
```

`sync` compares the local keyring (source of truth) against `keys/*.pub`,
the GitHub GPG key registration, and keys.openpgp.org, and reports what's
out of date. `--fix` re-exports the stale file, and re-registers/re-sends
the updated key where needed (GitHub does not support updating a
registration in place — `--fix` deletes the stale entry and re-adds the
current export). It is safe to run repeatedly regardless of where a
previous attempt stopped. It only checks *this host's* `hosts/<host>.nix` —
`[S]` is per-machine (ADR-0003 Amendment §1), so a second host sharing the
same identity (e.g. a personal laptop alongside a personal desktop) is
expected to carry its own independent subkey, not this machine's — and it
never edits nix files itself; a stale `hosts/<host>.nix` is reported with
the same manual-update instruction `export` prints.

## Obsidian vault backup

`home/modules/obsidian.nix` installs Obsidian, restic, `bws`, and
`obsidian-backup` on `vega` only. Obsidian is wrapped with nixGL like
the other Electron GUI applications. Three persistent systemd user timers run
a daily backup, weekly retention/health maintenance, and a monthly restore
acceptance test.

The synchronization remote and the backup remote are deliberately separate:
Self-hosted LiveSync uses Cloudflare R2, while restic uses Backblaze B2 through
its S3-Compatible API. Synchronization is not a backup.

### One-time Backblaze and Bitwarden setup

1. Create a private Backblaze B2 bucket with Object Lock disabled. Set its
   lifecycle to **Keep only the last version**; restic's S3 backend hides
   deleted objects, and without this rule B2 retains those hidden versions.
2. Create a bucket-scoped, read-write B2 application key. Record the bucket's
   S3 endpoint.
3. In Bitwarden Secrets Manager, create one project containing exactly these
   secret names:

   | Secret | Value |
   |---|---|
   | `RESTIC_REPOSITORY` | `s3:<B2 endpoint>/<bucket>/<prefix>` |
   | `RESTIC_PASSWORD` | A generated restic repository password |
   | `AWS_ACCESS_KEY_ID` | The B2 application key ID |
   | `AWS_SECRET_ACCESS_KEY` | The B2 application key |

4. Create a machine account with **read-only** access to that project and no
   other project. Generate an access token for this host.
5. Store only that revocable machine token in the login keyring:

   ```bash
   obsidian-backup configure-token
   ```

   The prompt does not echo the token. The token is not a recovery secret:
   do not duplicate it into a file or another vault. Revoke and replace it
   when rebuilding the PC. The four payload secrets remain solely in Secrets
   Manager and are injected by `bws run`; they are never materialized as an
   environment file.

The Home Manager module configures the US Bitwarden service and disables bws
state files. Without that opt-out, bws can reuse an encrypted local session for
up to an hour after the machine token is revoked; the backup path instead
re-authenticates from the Keyring token on every run.

6. Initialize the repository, run the first backup, and inspect the snapshot:

   ```bash
   obsidian-backup init
   obsidian-backup backup
   systemctl --user list-timers 'obsidian-backup*'
   ```

The user manager is tied to the graphical login session (`gpg.nix` rejects
linger), so GNOME Keyring is already unlocked when these timers run. If the
keyring is locked, Bitwarden is unavailable, a secret is missing, or B2
rejects a request, the command exits non-zero; timer failures also emit a
Herdr notification rather than reporting a success-shaped fallback.

### Retention, health checks, and restore tests

The weekly maintenance service retains 7 daily, 5 weekly, and 12 monthly
snapshots, prunes unreferenced data, then runs `restic check`:

```bash
systemctl --user start obsidian-backup-maintenance.service
journalctl --user -u obsidian-backup-maintenance.service
```

The acceptance restore unit restores the latest snapshot into a private
temporary directory and byte-compares three fixtures with the live vault. It
never writes into the live vault:

```bash
systemctl --user start obsidian-backup-restore-test.service
journalctl --user -u obsidian-backup-restore-test.service
```

`obsidian-backup-restore-test.timer` runs this automatically once a month
(`obsidian-backup-maintenance`'s weekly `restic check` only validates
repository metadata, it never reads a snapshot's actual payload back — this is
the layer that does). The three fixtures it compares —
`acceptance/roundtrip.md`, `acceptance/attachment.png`, and
`acceptance/attachment.pdf` — are permanent vault contents, not a
one-time setup artifact: `systemd.user.services.obsidian-backup-restore-test`
in `home/modules/obsidian.nix` hardcodes those paths, so the unit fails with
"source fixture is missing" if they are ever deleted from the vault.

For an ad-hoc restore test, pass one or more vault-relative files:

```bash
obsidian-backup restore-test journal/2026/2026-09-22.md
```

For a real recovery, first stop Obsidian and LiveSync, then use restic directly
with secrets injected from the same machine account. Restore into an empty
staging directory, inspect it, and only then copy the intended files into the
vault. Never target the live vault with an unreviewed `restic restore`.

### Rotation and removal

- Rotate a B2 application key by updating both AWS-named secrets in Bitwarden,
  running a backup and restore test, then revoking the old B2 key.
- Rotate the machine token with `obsidian-backup clear-token`, revoke it in
  Bitwarden, issue a replacement, and run `obsidian-backup configure-token`.
  Set the Bitwarden access token's own Expiration to roughly one year — the
  same annual cadence ADR-0003 uses to bound the `[S]` GPG subkey — so a
  missed manual rotation still lapses on its own instead of remaining valid
  indefinitely.
- Remove this host's automation by disabling both timers and clearing the
  machine token. Removing the Home Manager module removes the commands and
  units but intentionally does not delete any B2 data.

## Which layer does a new tool go in?

Decision flow for adding a tool, per
[ADR-0001](adr/0001-home-manager-as-source-of-truth.md) /
[ADR-0002](adr/0002-runtimes-and-hybrid-translation.md):

1. **Just trying it out?** Don't install it at all:

   ```bash
   nix shell nixpkgs#<tool>   # throwaway shell with the tool on PATH
   nix run nixpkgs#<tool>     # one-shot run
   , <command>                # comma (home.packages, #4 Layer 1): runs
                               # <command> once via nix-locate, no attribute
                               # name to type when it doesn't match the
                               # binary (e.g. `, rg` finds ripgrep)
   ```

   Nothing lands in any profile, so there is nothing to reclaim later.

2. **Needs root, or is a system service / driver / display-stack piece?**
   → the apt **system layer**: add it to
   `packages/declarative/apt-packages.txt` and install via
   `scripts/install-packages.sh`.

3. **Project-scoped toolchain** (language versions, per-repo pins)?
   → `mise` / `direnv` / `rustup`, declared in the project — not in this repo.

4. **Everything else** (user-space CLI, GUI app, font, prompt tooling)
   → **home-manager**, the default: add it to `home/modules/packages.nix`
   (or the topical module) and run `hms`. The other layers
   are escape hatches, not alternatives.

A tool that already slipped in ad hoc (apt / `cargo install` / `npm -g` /
pipx) should be reclaimed into the right layer. `detect-drift`(#4, Layer 2,
`crates/detect-drift`)reports these automatically — weekly via a
systemd/launchd timer (`home/modules/drift.nix`), or on demand:

```bash
detect-drift              # human-readable report, exit 1 if drift found
detect-drift --porcelain  # TSV: layer, name, nixpkgs attribute candidate
```

It never installs, removes, or modifies anything — deciding whether a
drifted package belongs in a layer above, or should stay an intentional
escape hatch, is still the human judgment call this section describes.

Once a tool's layer is decided, a second question applies whenever it needs a
concrete value (a bucket name, a project ID, a ping URL, a PRIVATE repo name):
does this repo need the **rule** (a derivation procedure with a placeholder,
public, this repo) or the **value itself** (private, the wrapper flake,
[ADR-0034](adr/0034-machine-state-wrapper-flake.md))? This repo never
names the wrapper flake — see `scripts/hms.sh`'s `private-hub` marker. Note
that `hms .` on a host with that marker registered applies this PUBLIC
worktree alone, dropping every private value module for that one apply.

### Ad-hoc installers must never prepend to PATH

Whatever the layer, an installer that puts its own directory at the **front**
of PATH breaks the whole decision flow above. nix enters PATH at
`/etc/profile.d/nix.sh` — system level, before any user file runs — so a
user-level prepend does not merely "come later", it **always outranks nix**.
The result is silent: a `cargo install`ed alacritty 0.15.1 answered to
`alacritty` for months while `home/modules/desktop.nix` declared 0.17.0-nixgl,
and nine other tools were shadowed the same way
([ADR-0029](adr/0029-path-precedence-enforces-source-of-truth.md)).

The order this repository guarantees:

```
$HOME/.local/bin  →  <nix profile>  →  <system>  →  ad-hoc installer dirs
```

So, when adding anything to the shell startup path:

- **Append, never prepend**, for any `$HOME`-local installer directory
  (`.cargo/bin`, `.deno/bin`, `.bun/bin`, …). `config/shell/common_env` is the
  place for it; `config/shell/profile` (deployed as `~/.profile`) is the one
  that governs the *graphical* session.
- **Do not `source` an installer's env script** if it prepends —
  `. "$HOME/.cargo/env"` is exactly that. Leave the file on disk (it no-ops
  once the directory is already on PATH), just stop sourcing it.
- `rustup-init`, if it is ever run here, needs **`--no-modify-path`**.
  `~/.profile` is a read-only store symlink, so the installer's attempt to
  amend it fails with `could not amend shell profile`. That failure is by
  design and harmless — PATH is already correct at that point — but the flag
  avoids the noise. Note `rustup` itself is declared in
  `home/modules/runtimes.nix`, so `rustup-init` should not be needed at all.

`$HOME/.local/bin` is the one exception to "nix always outranks the rest": it
comes first in PATH on purpose, and one entry there — `claude` — is a
deliberate shadow of the nix-profile `claude`, not drift. It is a symlink into
`~/.local/share/claude/versions/…`, kept live by the CLI's own self-updater;
`scripts/claude-plan-model` resolves concrete model IDs from that *installed*
binary's baked-in model catalog, so letting nix's copy win would silently
swap the binary `claude-plan-model` depends on ([#313](https://github.com/tarotene/dotfiles/issues/313)).
Other `.local/bin` names that happen to collide with a nix package (e.g.
`mise`, `uv`, `uvx`) are not exceptions — those should resolve to the nix
profile, and any ad-hoc binary left in `.local/bin` for them is drift to
reclaim, not a case to add here.

To check whether something is currently shadowed:

```bash
comm -12 <(ls ~/.cargo/bin) <(ls ~/.nix-profile/bin)   # names present in both
command -v <tool>                                      # which one actually wins
```

For a **GUI app** landing in step 4, check how it actually reaches fcitx5 — it is
not obvious, it differs per app, and guessing has cost real time here. There are
three routes, and the app picks one:

| route | who takes it | fcitx5 frontend |
|---|---|---|
| `zwp_text_input_v3` | native Wayland apps | `wayland_v2` |
| GTK/Qt immodule (`im-fcitx5.so`, D-Bus) | apt apps, with `GTK_IM_MODULE`/`QT_IM_MODULE` set | `dbus` |
| XIM | X11/XWayland apps | `xcb` |

A nix-installed app cannot load the apt immodule, so it takes route 1 or 3.

**Measure, do not assume.** The most direct answer is fcitx5's own view:

```bash
gdbus call --session --dest org.fcitx.Fcitx5 --object-path /controller \
  --method org.fcitx.Fcitx.Controller1.DebugInfo
```

It lists every input context with its `program:` and `frontend:`, and which one
holds focus. Corroborate with `grep im-fcitx5 /proc/<pid>/maps` (route 2),
`xprop -root _NET_CLIENT_LIST` (empty on this host — nothing is on XWayland at
all), and `WAYLAND_DEBUG=1` for which protocols the app binds.

On `company-pop-new` this currently splits as: **Chrome and Slack Desktop on
`wayland_v2`** (both are native Wayland, despite Slack having been assumed to be
an XWayland/XIM client for a while), everything apt — ghostty, Firefox — on
`dbus`. A mixed-frontend session is not a problem in itself, but it is the
precondition for the trigger-key defect recorded in
[`ime-chrome-diagnosis.md`](ime-chrome-diagnosis.md) / issue #14, so it is worth
knowing which side a new app lands on.
