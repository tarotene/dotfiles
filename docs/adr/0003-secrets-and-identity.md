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
   `docs/sign-prewarm.md`).
