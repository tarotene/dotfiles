# CONTEXT

> Design charter for the migration to a flake-based **standalone home-manager**
> configuration. Tracked by epic #207.

## Why

Make one person's **user environment** portable and reproducible across hosts —
personal and work, present and future — so a fresh machine can be provisioned
with near-zero manual steps. The immediate driver is a work-PC swap: the new
machine must be brought up identically to the personal PC "in seconds".

The current setup is a **procedural** dotfiles installer (shell scripts that
symlink configs and run `apt`/`cargo` installers). It works, but it is not
reproducible: there is no single source of truth, no pinning, and no clean
rollback. We migrate to a **declarative**, flake-based standalone home-manager
configuration.

## Scope boundaries

home-manager is the **source of truth for the user environment**. Two layers
stay deliberately outside it as escape hatches:

- **System layer (`apt`)** — kernel, drivers, display manager, input-method
  daemons (`fcitx5`), and anything that needs root or a system
  service. Slimmed to a system-only set in Phase 2 (#216). See ADR-0001.
- **Per-project runtimes (`mise` / `direnv` / `rustup`)** — language toolchains
  and project-local versions. home-manager installs the *launchers*; the actual
  toolchains stay project-scoped. See ADR-0001 / ADR-0002.

## Target end-state

| Axis | Decision | ADR |
|---|---|---|
| Scope | home-manager = source of truth; apt + per-project runtimes are escape hatches | ADR-0001 |
| Host model | Pop!_OS ×2 (do not assume root); multi-user Nix default, single-user fallback documented | ADR-0001 |
| Runtimes | consolidate Java/Go into mise (drop SDKMAN!/goup); keep rustup as the global Rust default; ROS scoped to the personal host | ADR-0002 |
| Translation | hybrid — keep working config files literal, use Nix DSL only where interpolation pays | ADR-0002 |
| Secrets / identity | YubiKey-rooted; runtime-decrypted SOPS (no sops-nix); public keys committed; retire keybase | ADR-0003 |
| Repo identity | keep the `dotfiles` name; relocate to public `tarotene/dotfiles` via a clean orphan history; no semver releases | ADR-0004 |
| Infra | latest pinned stable release channel; Determinate Systems installer; nix-centric CI; rollback via generations | ADR-0001 |

## Repository layout (two-layer flake)

The flake is **Identity / Instance** two-layer:

```
flake.nix                     # inputs (nixpkgs + home-manager, pinned) and homeConfigurations.<hostname>
home/
  common.nix                  # shared across every host
  identities/
    personal.nix              # identity-scoped settings (personal)
    company.nix               # identity-scoped settings (work)
  hosts/
    <hostname>.nix            # instance-scoped settings; imports common + one identity
docs/
  adr/0001..0004              # architecture decision records
  nixification-roadmap.md     # candidates to move from literal config to native Nix DSL
```

`homeConfigurations.<hostname>` is keyed by hostname so that
`home-manager switch` selects the right config automatically on each machine. A
host module imports `home/common.nix` plus exactly one identity module.

## Migration strategy

Strangler build + per-host clean cutover (`home-manager switch -b backup`),
in order:

1. **Phase 0 — Foundation** (#208): scaffold flake + skeleton. Build-only; no
   `switch` on any host.
2. **Phase 1 — Core user environment** (#209–#215): shell, git/identity,
   GPG/YubiKey, secrets, packages, terminal/desktop, dev-runtime escape hatches.
3. **Phase 2 — System layer** (#216): slim apt to a system-only set + thin
   installer.
4. **Phase 3 — Cutover** (#217): per-host cutover + `bootstrap.sh`
   (pre-swap work PC → personal → post-swap greenfield).
5. **Phase 4 — Cleanup & docs** (#218–#219): retire procedural machinery,
   nix-centric CI, rewrite the charter, retire keybase.
6. **Phase 5 — Relocation** (#220): clean publish to public `tarotene/dotfiles`.

## References

- [home-manager manual](https://nix-community.github.io/home-manager/)
- [home-manager standalone flakes](https://nix-community.github.io/home-manager/index.xhtml#sec-flakes-standalone)
- [Determinate Systems nix-installer](https://github.com/DeterminateSystems/nix-installer)
- [sops-nix](https://github.com/Mic92/sops-nix) — evaluated and deferred (ADR-0003)
- YubiKey + GPG operating model: <https://fuwa.dev/posts/yubikey/>
