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
- Automating this as a weekly `flake.lock` PR (update-flake-lock driven by a
  GitHub App token) is tracked in
  [#3](https://github.com/tarotene/dotfiles/issues/3); until then this manual
  routine is the operating procedure.
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

## Which layer does a new tool go in?

Decision flow for adding a tool, per
[ADR-0001](adr/0001-home-manager-as-source-of-truth.md) /
[ADR-0002](adr/0002-runtimes-and-hybrid-translation.md):

1. **Just trying it out?** Don't install it at all:

   ```bash
   nix shell nixpkgs#<tool>   # throwaway shell with the tool on PATH
   nix run nixpkgs#<tool>     # one-shot run
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
pipx) should be reclaimed into the right layer — workflow tracked in
[#4](https://github.com/tarotene/dotfiles/issues/4).

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
