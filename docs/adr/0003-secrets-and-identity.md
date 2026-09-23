# ADR-0003 — secrets & identity: YubiKey-rooted, runtime-decrypted SOPS

- Status: Accepted
- Date: 2025
- Epic: #207

## Context

Identity (git signing, SSH, decryption) is rooted in a YubiKey. Secrets are
currently decrypted at runtime by a SOPS shell module. We must decide how
secrets and identity fit into a declarative home-manager world, and whether to
adopt `sops-nix`.

## Decision

- **Identity is YubiKey-rooted.** GPG signing/decryption and SSH auth use
  per-machine subkeys cut to the smartcard. The public keys are **committed**
  (non-secret) so a new host can import them declaratively.
- **Secrets stay runtime-decrypted via SOPS** in the interactive shell — the
  existing `35-secrets-sops.zsh` + `sops-secrets-env.sh` behaviour, ported into
  a home-manager-managed zsh module (escape hatch preserved).
- **Do NOT adopt `sops-nix`.** sops-nix decrypts at *activation* time, which —
  with a YubiKey-backed age/GPG recipient — would force an interactive PIN entry
  on every `home-manager switch`. That breaks unattended/CI builds and the
  "silent failure when the key is absent" property. Evaluated and deferred.
- `.sops.yaml` recipients = the identity's YubiKey **encryption subkey**.
  Secrets are decrypted only in the interactive shell, never baked into the Nix
  store.
- **Retire keybase.** Public keys live in the repo; keybase is dropped.
- One irreducible **manual** provisioning step per machine: insert the YubiKey,
  cut/import the per-machine subkey, and set git `signingkey`. This is
  hardware-bound and cannot be declarative (documented in #211).

## Consequences

- No private key material ever enters the Nix store or git history.
- `home-manager switch` stays non-interactive and CI-buildable (no PIN prompts).
- With the YubiKey present, secrets load into the interactive shell; with it
  absent, the shell still starts cleanly (silent-failure preserved).
- A documented, minimal manual step remains per host — accepted as the cost of a
  hardware root of trust.

## Amendment (2026 — post-cutover, #238)

The end-to-end grilling that preceded #233 surfaced four points where this ADR,
as originally written, did not match the identity & secret model as actually
deployed. The corrections are recorded here in place (the original Decision text
above is left intact for provenance); where they conflict, this amendment wins.

1. **Subkey placement is not uniform.** The Decision reads as if every subkey is
   cut to the smartcard. In practice only `[A]` (auth) and `[E]` (encrypt) live
   on the YubiKey; the `[S]` (sign) subkey is held **on-disk per-machine**,
   passphrase-protected, and rotated annually. Rationale: a smartcard `[S]`
   forces a YubiKey touch on every `git commit`; an on-disk passphrase-protected
   `[S]` trades that for a memorized passphrase plus a rotation cadence. Evidence:
   in `gpg --list-secret-keys --with-keygrip`, the `[A]`/`[E]` `ssb` lines carry
   the `ssb>` smartcard-stub marker while the `[S]` line does not.

2. **Identities are plural.** The Decision's singular "the identity" understates
   the design: there are **two** identities — personal and company — each with
   its own YubiKey, its own offline master, and a 1:1 binding to a host module
   (`home/hosts/<identity>-pop[-<generation>].nix`, e.g. `personal-pop.nix`,
   `company-pop-old.nix`, `company-pop-new.nix`). One host resolves to exactly
   one identity; no host mixes the two.

3. **`.sops.yaml` is host-local, and recipients are the master fingerprint.**
   The Decision line "`.sops.yaml` recipients = the identity's YubiKey encryption
   subkey" is inaccurate on both counts. `.sops.yaml` is **not** committed: it is
   generated per host at `~/.sops/.sops.yaml` by `scripts/setup-sops-secrets.sh`,
   and the encrypted `~/.sops/.env` is likewise host-local. Its `pgp` recipients
   list the identity's **master key fingerprint**; GnuPG then resolves encryption
   to the `[E]` subkey internally at encrypt time. No secret material and no
   `.sops.yaml`/`.env` ever enter the repo or the Nix store.

4. **Migration ⊆ rotation.** Bringing up a new host is treated as an **irregular
   rotation** of the on-disk `[S]` subkey rather than a distinct procedure. This
   unifies three cases under one primitive: Stage 2 cutover reuses the existing
   `[S]`, Stage 3 greenfield cuts a fresh `[S]` on the new host, and the annual
   cadence rotates `[S]` in place. `[A]`/`[E]` are unaffected — they stay on the
   smartcard across host swaps.

## Amendment 2 (2026-08 — #35)

Grilling a request to skip signing on squash-only remotes surfaced that the
real pain was passphrase-entry frequency, not signing itself, and that fixing
it changes the effective grain of Amendment §1's "a memorized passphrase plus
a rotation cadence." Recorded here rather than folded into the Amendment above
because it stems from a different investigation.

1. **The passphrase cache window moved from a fixed clock to the login
   session.** `home/modules/gpg.nix` previously set `defaultCacheTtl=3600` /
   `maxCacheTtl=7200` — a forced re-entry at least every 2 hours. Both are now
   effectively unbounded (400d). What actually bounds the cache is not the
   clock but the `gpg-agent` process's own lifetime, which ends when its
   systemd user instance dies — normally at logout. In effect, the on-disk
   `[S]` passphrase is now entered once per login rather than once every
   couple of hours. The corollary: **the cache also survives a screen lock**,
   since locking does not end the login session.

2. **This bound is asserted, not enforced.** Whether "agent dies at logout"
   holds depends on `loginctl`'s per-user `Linger` setting, which was measured
   on only one host. `gpg.nix` cannot force `Linger=no` itself:
   `org.freedesktop.login1.set-user-linger` is `auth_admin_keep` on this OS
   (`/usr/share/polkit-1/actions/org.freedesktop.login1.policy:137`, no
   override rule), so calling `loginctl disable-linger` from activation would
   either prompt for admin authentication on every `home-manager switch` —
   breaking this ADR's own Consequence that switch stays non-interactive — or
   silently fail behind an error guard. Reading the setting needs no
   authorization, so `home.activation.assertNoLinger` only checks it: if
   `Linger=yes`, activation exits 1 before `writeBoundary` (no files written)
   and points at `sudo loginctl disable-linger <user>` as the fix.

3. **The remaining sentries are unchanged from Amendment §1**: the annual
   `[S]` rotation cadence, and the fact that the key is still passphrase-
   protected on disk (clearing the agent's cache still requires re-entry).
   `grabKeyboardAndMouse` stays `true`; the moment the passphrase is asked for
   is instead moved earlier, to a `SessionStart` hook that prompts while the
   user is already looking at the screen (`config/claude/hooks/sign-prewarm.sh`,
   `docs/claude/sign-prewarm.md`).

## Amendment 3 (2026-09 — #235)

Absorbing a private predecessor tool's subkey tooling into `scripts/gpg-subkey`
surfaced that Amendment §1 Decision 4's phrase "the annual cadence rotates
`[S]` **in place**" is ambiguous between two different operations, and the
tooling review needed to pick one before it could rotate a real identity's
`[S]` subkey.

1. **"Rotation" means generate-new-then-revoke-old, not extend the same
   key's expiry.** "In place" describes *where* the new `[S]` lands (still
   on-disk, still per-machine, no card round-trip) — not that the same key
   material survives. This is now the tested, deployed behavior
   (`scripts/gpg-subkey rotate --revoke-old`) and matches actual production
   history: the company identity's `[S]` went through two real rotations
   (2025-12, 2026-07), each cutting a **new** keyid and revoking the
   previous one, never extending an existing key's expiry
   (`gpg --quick-set-expire`/`--edit-key ... expire` was never used).
   Rationale: the rotation cadence exists to bound how long any *one*
   on-disk key stays valid (Amendment §1's stated trade for skipping a
   smartcard touch per commit) — extending one key's expiry indefinitely
   would defeat that bound.

2. **The local passphrase protecting each generated `[S]` may be reused
   across rotations.** The passphrase's job is local-disk protection of
   whichever `[S]` currently exists; it is orthogonal to the key-material
   rotation cadence in point 1, and forcing a fresh passphrase on every
   annual rotation only pushes toward weaker, predictable passphrases.
   Per NIST SP 800-63B (memorized secrets should not be rotated on a fixed
   schedule; a verifier "shall force a change if there is evidence of
   compromise of the authenticator", otherwise not —
   <https://pages.nist.gov/800-63-3/sp800-63b.html>, retrieved 2026-09-20),
   the same passphrase may be reused for a newly generated `[S]` subkey.
   Reuse must stop and the passphrase must change the moment there is any
   suspicion it was itself exposed (shoulder-surfed, logged by a compromised
   pinentry, found in a leaked backup) — that is a compromise event
   independent of the annual key-material cadence.

## Amendment 4 (2026-09 — #252)

Investigating a complaint that esa MCP (#239, ADR-0022) forces a physical
YubiKey insertion at every Claude Code session start surfaced that `[E]`
was the one subkey Amendment §1 left card-backed, and that this — not
signing — was the actual remaining source of per-session card friction. The
fix applies Amendment §1's own reasoning to `[E]`, plus two empirical
findings from validating it on real hardware.

1. **`[E]` is now also held on-disk per-machine, in addition to the
   card-backed original.** Generated/rotated via `scripts/gpg-subkey
   generate|rotate --usage encrypt` (#252, extending the `--usage sign`
   default tooling built for Amendment §1/§3). The card's original `[E]` is
   **deliberately not revoked** — it stays live as a fallback / disaster-
   recovery path (Consequences §2 below), unlike `[S]` where the smartcard
   copy was never cut in the first place. `rotate`'s revoke-candidate
   collection and "which subkey is currently active" resolution are both
   restricted to on-disk secret material only (GnuPG `--with-colons` field
   15 == `+`) — the card-backed `[E]` is structurally excluded from ever
   being an `--revoke-old` target, so a future annual rotation cannot
   accidentally revoke it.

2. **GnuPG resolves encryption to the newest valid `[E]` automatically.**
   `gpg --encrypt -r <primary-fingerprint>` (the existing convention this
   repo already used for `token.gpg`, unchanged by this amendment) picks the
   most-recently-created non-revoked `[E]` subkey when more than one exists.
   Verified empirically (2026-09-20, personal identity): with a 2025-07
   card-backed `cv25519` `[E]` and a freshly generated 2026-09 on-disk
   `cv25519` `[E]` both valid, encryption selected the on-disk one with no
   `!`-suffixed subkey pinning needed.

3. **`[S]` and `[E]` are independent `gpg-agent` cache entries, even under
   an identical passphrase string.** Cache entries are keyed by keygrip, not
   by passphrase value — unlocking `[S]` does not warm `[E]`'s entry and
   vice versa (verified empirically, 2026-09-20/21: killing `gpg-agent` and
   probing each subkey with `--pinentry-mode error` showed both cold
   independently, and warming one left the other cold). `sign-prewarm.sh`
   (Amendment §2/§3) was extended to prewarm `[E]` too, as a fully
   independent code path gated on `~/.config/esa/token.gpg`'s existence
   (mirroring `scripts/esa-mcp-launcher`'s own path resolution) rather than
   on any git config — a host without esa MCP set up (company-pop-*, or a
   personal-pop before #252's rollout) sees this path stay silent. This does
   not reduce the prompt count below Amendment §2's existing "once per
   login" shape — it batches the (now) two independent prompts at
   `SessionStart` instead of letting `[E]`'s fire unpredictably whenever the
   esa MCP server first launches.

4. **A GnuPG-native external password cache does not eliminate even that
   one prompt on this host, and Amendment §3's passphrase-reuse allowance
   does not change that.** `gpg-agent` permits pinentry to persist a
   passphrase to an external cache (e.g. via libsecret/gnome-keyring) by
   default (`--no-allow-external-cache` is the opt-out; GnuPG Project,
   "Agent Options",
   <https://www.gnupg.org/documentation/manuals/gnupg/Agent-Options.html>,
   retrieved 2026-09-20) and the host's `pinentry-gnome3` binary is linked
   against `libsecret`/`libgcr-base`. In practice, though, its GCR system-
   prompter dialog on this host (Pop!\_OS, GNOME/GCR 3.41.2) never offers a
   "save to keyring" option and no Secret Service item is ever created —
   verified empirically by inspecting `org.freedesktop.secrets` via
   `gdbus` before and after multiple real unlocks. This is recorded so it
   is not re-investigated without new evidence (a GCR/pinentry-gnome3
   version bump, or a different pinentry flavor). The accepted residual
   is therefore Amendment §2's existing shape — one passphrase prompt per
   subkey-usage-class per login — applied to two independent classes
   (`[S]`, `[E]`) instead of one.

## Consequences (Amendment 4)

- No YubiKey insertion is required for any routine Claude Code session
  start any more — the original complaint behind #252 is fully resolved.
- The card's `[E]` remains a valid, non-revoked fallback: a lost/wiped
  on-disk `[E]` (or a fresh checkout on a machine that hasn't run the
  `--usage encrypt` rollout) can still decrypt anything encrypted to this
  identity's primary fingerprint, by inserting the card.
- `keys/<identity>.pub` and `gpg-subkey sync` now track two independent
  `[S]`/`[E]` on-disk lifecycles per host instead of one; `docs/claude/
  esa-mcp.md` and `docs/claude/sign-prewarm.md` carry the operational
  detail.

## Amendment 5 (2026-09 — #348)

Investigating an unrelated sudo-askpass check surfaced that Amendment §4
item 4's "no Secret Service item is ever created" claim was too broad. A
bare Assuan `GETPIN` command sent directly to the `pinentry-gnome3`
binary — bypassing `gpg-agent` entirely — returned the real login
password via the `PASSWORD_FROM_CACHE` external-cache protocol, with no
human interaction (#348, reproduced twice). The GCR system-prompter
dialog (what Amendment §4 actually tested) indeed never offers a "save
to keyring" UI; the mistake was extrapolating from that to "the external
cache is unreachable" — a caller that skips the dialog and drives the
Assuan protocol directly can still reach it, because the login keyring
is already unlocked (PAM auto-unlocks it at login) and same-uid access
to an unlocked Secret Service item is a GNOME-acknowledged trust
boundary, not a bug GNOME will fix (CVE-2018-19358 was not accepted as
a vulnerability on this basis).

1. **`services.gpg-agent.noAllowExternalCache` is now declared `true`.**
   This is home-manager's typed option for GnuPG's own
   `--no-allow-external-cache` (Amendment §4 item 4 already cited this
   flag but concluded it was unnecessary; #348 supersedes that
   conclusion). It stops `gpg-agent` from ever sending pinentry the
   `OPTION allow-external-password-cache` that enables the cache path.
2. **This closes only the channel `gpg-agent` itself opens.** It does
   not stop an arbitrary same-uid process from launching pinentry
   directly and driving the Assuan protocol itself, as #348 did.
   Nothing in GnuPG or GCR distinguishes "`gpg-agent` asked" from
   "anything on this uid asked". Closing that fully would require not
   trusting the login keyring's auto-unlock at all, which conflicts
   with this host's own use of it (`scripts/obsidian-backup` reads a
   Bitwarden token from the same keyring via `secret-tool lookup` for
   unattended systemd-timer runs). The residual risk (same-uid Secret
   Service read) is therefore accepted and only detected, not
   eliminated — #348 stays open for the human follow-up (credential
   rotation, login-keyring item inventory) that is out of this repo's
   reach (ADR-0034: no real secret values in this public repo).

### 執行点

- `home/modules/gpg.nix` — `services.gpg-agent.noAllowExternalCache = true;`(#348)

## Consequences (Amendment 5)

- Amendment §4 item 4's claim narrows: the GCR dialog still never offers
  a "save to keyring" UI, but the underlying external-cache protocol is
  reachable by a direct pinentry caller, and is now explicitly closed at
  the `gpg-agent` level.
- `docs/claude/sign-prewarm.md`'s parallel claim is corrected in the
  same PR.
