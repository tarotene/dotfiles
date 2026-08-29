# GPG / YubiKey (#211) — programs.gpg + services.gpg-agent.
#
# Declares the non-secret YubiKey/GPG surface: the agent with pinentry and
# scdaemon for smartcard access.  Public keys are committed under keys/ so a
# fresh host can import them at activation time.
#
# System-layer note: scdaemon uses its built-in CCID driver to talk to the
# YubiKey directly — no pcscd dependency (#252).
#
# One irreducible manual step per machine (documented below):
#   1. Insert the YubiKey.
#   2. gpg --card-status          (verifies the card is seen)
#   3. gpg --import keys/*.pub    (if not already imported by activation)
#   4. gpg --edit-key <KEYID>     → trust → 5 (ultimate) → quit
#   5. Set the per-host signing key in the host module:
#        programs.git.signing.key = "<SUBKEY_ID>";
#
# This step is hardware-bound and cannot be declarative (ADR-0003).
#
# TTL / Linger (ADR-0003 Amendment 2): the on-disk [S] passphrase is cached for
# the whole login session rather than a fixed clock window. What actually
# bounds that cache is not the TTL but the agent process's own lifetime, which
# ends with the login session only if the systemd user instance does not
# linger past logout. assertNoLinger below refuses to activate rather than
# assume that — it cannot enforce it: org.freedesktop.login1.set-user-linger
# is auth_admin_keep on this OS, so calling `loginctl disable-linger` here
# would either prompt for admin auth on every switch (breaking the
# non-interactive switch ADR-0003 promises) or silently fail. Reading the
# state needs no authorization, so that is all this does.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  repoRoot = ../..;
in
{
  programs.gpg = {
    enable = true;

    settings = {
      # Prefer strong algorithms.
      personal-digest-preferences = "SHA512 SHA384 SHA256";
      cert-digest-algo = "SHA512";
      default-preference-list = "SHA512 SHA384 SHA256 AES256 AES192 AES ZLIB BZIP2 ZIP Uncompressed";

      # Show long key IDs and fingerprints.
      keyid-format = "0xlong";
      with-fingerprint = true;

      # Disable recipient key ID in messages (privacy).
      throw-keyids = true;

      # Auto-retrieve keys when verifying signatures.
      auto-key-retrieve = true;
      keyserver = "hkps://keys.openpgp.org";
    };
  };

  services.gpg-agent = {
    enable = true;

    # GUI pinentry via GCR system prompter (gnome-keyring provides it on
    # Pop!_OS/COSMIC). Unlike curses it also works when gpg is invoked without
    # a TTY (git GUIs, editors, agents); falls back to curses over SSH.
    pinentry.package = pkgs.pinentry-gnome3;

    # Cache the on-disk [S] passphrase for the whole login session. The bound
    # is not the clock but the agent's lifetime, which ends with the login
    # session — assertNoLinger below refuses to activate on a host where that
    # is not true. Threat-model delta vs. the previous 1h/2h: see the header
    # comment above and ADR-0003 Amendment 2.
    defaultCacheTtl = 34560000; # 400d — i.e. bounded by the login, not the clock
    maxCacheTtl = 34560000;

    # Enable SSH agent support so the YubiKey auth subkey can serve as an SSH key.
    enableSshSupport = true;
  };

  # Import committed public keys at activation time.  On a fresh host the user
  # still needs to run `gpg --card-status` and set trust (see header).
  home.activation.importGpgKeys =
    let
      keyDir = repoRoot + "/keys";
    in
    # lib.hm.dag.entryAfter ensures this runs after writeBoundary (files are
    # deployed).  We import only *.pub files; missing dir is a no-op.
    {
      after = [ "writeBoundary" ];
      before = [ ];
      data = ''
        keydir="${keyDir}"
        if [ -d "$keydir" ]; then
          for pubkey in "$keydir"/*.pub; do
            [ -f "$pubkey" ] || continue
            $DRY_RUN_CMD ${pkgs.gnupg}/bin/gpg --batch --import "$pubkey" 2>/dev/null || true
          done
        fi
      '';
    };

  # Assert, never enforce (see the header comment above). Runs before
  # writeBoundary so a host that fails this check gets no files written at
  # all, rather than a half-applied generation.
  home.activation.assertNoLinger = lib.hm.dag.entryBefore [ "writeBoundary" ] ''
    linger="$(${pkgs.systemd}/bin/loginctl show-user --value -p Linger \
      ${config.home.username} 2>/dev/null || true)"
    if [ "$linger" = "yes" ]; then
      echo "gpg.nix: Linger is enabled for ${config.home.username}." >&2
      echo "  gpg-agent — and with it the cached on-disk [S] passphrase —" >&2
      echo "  would survive logout for up to the 400d cache TTL set here." >&2
      echo "  Fix: sudo loginctl disable-linger ${config.home.username}" >&2
      echo "  Rationale: docs/adr/0003-secrets-and-identity.md, Amendment 2." >&2
      exit 1
    fi
  '';
}
