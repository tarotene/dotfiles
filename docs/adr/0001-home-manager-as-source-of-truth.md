# ADR-0001 — home-manager is the source of truth; apt and per-project runtimes are escape hatches

- Status: Accepted
- Date: 2025
- Epic: #207

## Context

The current dotfiles are procedural shell scripts that symlink configs and shell
out to `apt` and `cargo`. There is no single source of truth, no pinning, and no
clean rollback. We want a fresh machine — personal or work — provisioned to an
identical user environment with near-zero manual steps.

We need to decide *what* owns the user environment, and where the boundaries
with the operating system sit.

## Decision

- **home-manager (standalone, flake-based) is the source of truth for the user
  environment.** Shell, git, terminal, user-space CLIs, fonts, prompts, and
  per-user services are declared in the flake.
- Two layers stay **outside** home-manager as deliberate escape hatches:
  - **System layer (`apt`)** — anything requiring root or a system service:
    kernel, drivers, display manager, input-method daemons (`fcitx5`).
    Slimmed to a system-only set in Phase 2 (#216).
  - **Per-project runtimes (`mise` / `direnv` / `rustup`)** — language
    toolchains and project-local versions. home-manager installs the launchers;
    toolchains stay project-scoped (see ADR-0002).
- **Host model:** two Pop!_OS machines. We do **not** assume root: the
  multi-user Nix install (daemon) is the default, with the single-user install
  documented as a fallback.
- **Infra:** pin to the latest **stable release** channel; install Nix with the
  Determinate Systems installer; nix-centric CI; rollback via home-manager
  generations (`home-manager switch -b backup`, `home-manager generations`).

## Consequences

- One declarative source builds the same user environment on every host.
- The OS install stays thin and replaceable; we never try to manage drivers or
  the display stack through Nix.
- Project toolchains remain fast to switch and do not bloat the global closure.
- Rollback is a generation switch, not a hand-written snapshot system.

## Amendment (2026-07 — fcitx5 moves to home-manager, #14)

The Decision above lists `fcitx5` under the system layer, on the stated criterion
"anything requiring root or a system service." **That criterion never applied to
fcitx5 on these hosts.** fcitx5 runs as a *per-user* process:
`app-fcitx5@autostart.service` inside `user@1000.service/app.slice`, started from
`~/.config/autostart/fcitx5.desktop`, requiring no root and no system unit. This
ADR's own rule therefore places it in the user environment. The original placement
was a mis-application of the criterion, not a trade-off between competing ones.

What prompted looking at it: the investigation in
[`docs/ime-chrome-diagnosis.md`](../ime-chrome-diagnosis.md) / issue #14 needed to
test a newer fcitx5, and apt noble caps it at `5.1.7-1build3` (upstream tag
2024-01-16, no newer candidate) — so the system layer offered no way to change the
version at all, not even to rule it out.

**The version turned out not to be the cause** (it was measured and refuted; the
cause is a leftover password content type, and the fix is a config option). That
does not weaken this amendment, it clarifies it: the argument here is about the
*criterion*, not about any particular defect. Being unable to choose the version of
a per-user daemon is a defect in the layer boundary whether or not a specific bug
happens to be fixed by choosing it. The pinned nixpkgs ships 5.1.19.

Scope of the move:

- The **daemon and the mozc engine** move to home-manager, as
  `qt6Packages.fcitx5-with-addons.override { addons = [ fcitx5-mozc ]; }`, with
  the autostart `Exec=` pointing at the wrapped store path.
- The **client-side immodules stay apt** (`fcitx5-frontend-all`). An
  apt-installed GTK/Qt application can only load an immodule out of `/usr/lib`,
  and two do so today — verified by `im-fcitx5.so` appearing in ghostty's and
  Firefox's `/proc/<pid>/maps`.

The system-layer criterion is therefore restated as: **root, a system service,
kernel/driver integration, or code that must be loaded into an apt-installed
process.** The last clause is the part that generalises — it is what makes the
next borderline case decidable instead of ad hoc.

Kernel, drivers and the display stack are unchanged: still apt, still out of scope
for Nix.

### Consequence worth stating

A home-manager-owned input method is only as current as the pin. The failure mode
this amendment fixes — running two-year-old software with a known fix released —
is now a flake-update question rather than a distro-release question, which is
strictly better but not automatic. `docs/operations.md` owns that cadence.

## Amendment (2026-08 — nixpkgs-unstable escape hatch for herdr, #42)

The Decision above pins to "the latest **stable release** channel" with no stated
exception. `herdr` (the worktree/workspace manager this repo's own hooks already
assume — see `docs/claude/git-worktree-allow.md`) landed in nixpkgs after the
`nixos-26.05` branch-off and is not evaluable there at all
(`nix eval` on the pinned channel errors with "does not provide attribute
… herdr"). Being on the stable channel offered no way to reach the package, not
even an old version to accept as a trade-off — the same shape of gap the fcitx5
amendment above describes, one layer up (a missing *package*, not a capped
*version*).

Unlike the `nixgl` input, a second nixpkgs here does not need
`inputs.nixpkgs.follows`: `follows` is load-bearing for `nixgl` because its mesa
and libglvnd get `dlopen`'d into *this repo's own* GUI processes, so a second
nixpkgs would mean a second glibc in the same address space
(`GLIBC_2.x not found`). `herdr` is a plain TUI binary that is only ever `exec`'d,
never `dlopen`'d into anything this repo builds — a second glibc in its own
closure is invisible to every other package.

**Criterion for the next borderline case**: a single-package `nixpkgs-unstable`
overlay is acceptable for a user-space tool that (a) is absent from the pinned
stable channel, and (b) is never `dlopen`'d into another package's process (i.e.
it does not need to share a libc/ABI with anything else in the closure — the same
"loaded into a process" distinction the fcitx5 amendment already draws for the
system-layer boundary, applied here to the *input* boundary instead). A tool that
fails (b) — anything resembling `nixgl` — still needs `follows`, or stays out of
this escape hatch entirely.

Version tracking follows the same discipline as any other pinned package here
(cf. `claude-code` in `home/modules/packages.nix`): the lock fixes the version:
it does not move on its own, only via a deliberate `nix flake update
nixpkgs-unstable`. An unstable input moves faster than the stable channel it sits
beside, so `docs/operations.md`'s routine-update cadence calls that out
separately.

**File an issue to drop this input the moment `nixpkgs.herdr` evaluates on
`nixos-26.05`** — this escape hatch is not meant to be a permanent second input;
Issue #42 tracks it for the current instance.
