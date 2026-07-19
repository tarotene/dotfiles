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
{ pkgs, ... }:
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

    # Cache passphrases for a reasonable session window.
    defaultCacheTtl = 3600;
    maxCacheTtl = 7200;

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
}
