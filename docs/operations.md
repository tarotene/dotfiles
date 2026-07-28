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
not obvious and differs per app. A nix-installed app cannot load the apt GTK/Qt
immodule, so one that runs under XWayland (Slack, Zoom) falls back to XIM, while
one that goes native Wayland (Chrome) uses `zwp_text_input_v3`. Measure before
assuming: `xprop -root _NET_CLIENT_LIST` shows whether anything is on XWayland at
all, and `WAYLAND_DEBUG=1` shows which protocols the app binds. Known-unresolved
issue and the measurements already taken:
[`ime-chrome-diagnosis.md`](ime-chrome-diagnosis.md).
