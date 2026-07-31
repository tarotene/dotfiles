# Operations

Routine care of the declarative environment. For provisioning and migration,
see [`cutover-runbook.md`](cutover-runbook.md).

## Routine flake update

Backports to the pinned stable nixpkgs channel are best-effort and batched
upstream, so `flake.lock` drifts silently unless refreshed on a cadence —
weekly is enough:

```bash
nix flake update
nix flake check
home-manager switch --flake .#"$(hostname)" -b backup
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

### fcitx5 needs an explicit unit restart after a switch

`app-fcitx5@autostart.service` is **generated** from
`~/.config/autostart/fcitx5.desktop` by `systemd-xdg-autostart-generator`, and its
`Exec=` is a store path. `home-manager switch` runs `daemon-reload` but does **not**
restart a generated unit, so after any switch that moves fcitx5 the old binary is
still running:

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
   (or the topical module) and run `home-manager switch`. The other layers
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
