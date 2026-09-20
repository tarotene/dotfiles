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
  subkeyPath = "${config.home.homeDirectory}/.local/bin/gpg-subkey";
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

      # Disable recipient key ID in messages (privacy). Applies to every
      # `gpg --encrypt` on this host — a local-storage file encrypted for a
      # card-backed key (e.g. esa MCP's token.gpg) MUST pass `--no-throw-keyids`
      # explicitly, or decryption falls back to trying every card-backed
      # secret key in the keyring in turn (one "insert card" prompt per card
      # owned). See docs/claude/esa-mcp.md's "落とし穴: throw-keyids" section.
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
    # darwin has no GCR/gnome-keyring; pinentry_mac is the established
    # equivalent there (Keychain-backed GUI prompt, home-manager's own
    # gpg-agent module supports it via launchd — ADR-0018).
    pinentry.package = if pkgs.stdenv.isDarwin then pkgs.pinentry_mac else pkgs.pinentry-gnome3;

    # Cache the on-disk [S] passphrase for the whole login session. The bound
    # is not the clock but the agent's lifetime, which ends with the login
    # session — assertNoLinger below refuses to activate on a host where that
    # is not true. Threat-model delta vs. the previous 1h/2h: see the header
    # comment above and ADR-0003 Amendment 2.
    defaultCacheTtl = 34560000; # 400d — i.e. bounded by the login, not the clock
    maxCacheTtl = 34560000;

    # No `enableSshSupport` here, deliberately (#33). The [A] subkey does exist
    # on the card (keygrip AC6226020D13A46E0CD8E47A8C08C5D01C142127), but it was
    # never registered in ~/.gnupg/sshcontrol and nothing on any host speaks SSH:
    # there is no ~/.ssh, and git remotes are HTTPS via `gh auth git-credential`
    # (see git.nix). The declaration was therefore inert.
    #
    # Inert is not free. `enableSshSupport = true` makes gpg-agent claim
    # SSH_AUTH_SOCK, so the day someone does start using ssh, gpg-agent answers
    # as an ssh-agent holding zero identities and the result is a
    # `Permission denied (publickey)` whose cause is nowhere near ssh. An unused
    # declaration that breaks the first use of the thing it names is a liability,
    # not a convenience — so it is gone, along with the defaultCacheTtlSsh /
    # maxCacheTtlSsh question it dragged along (those were never set, leaving the
    # SSH cache at the 30m/2h default, asymmetric with the [S] policy above).
    #
    # Reversible in one line: the subkey stays on the card, so re-enabling this
    # and writing the keygrip into ~/.gnupg/sshcontrol is all it would take.
  };

  # gpg-subkey: generate/rotate a machine-local [S] subkey and refresh
  # keys/<identity>.pub (absorbed from a now-archived private predecessor
  # tool — see the script's own header for why; it never writes git config,
  # `programs.git.signing.key` above stays the sole declared source of truth
  # per ADR-0003).
  home.file.".local/bin/gpg-subkey" = {
    source = repoRoot + "/scripts/gpg-subkey";
    executable = true;
  };

  # Newly taken under home-manager management here — same `.backup` collision
  # quarantine (#64) as herdr's config.toml above; see quarantine.nix for why
  # this is a shared helper. Without this, a pre-existing real file/symlink at
  # this path makes `hms` fail at checkLinkTargets (#244).
  dotfiles.quarantine.managedFiles = [ ".local/bin/gpg-subkey" ];

  # Daily expiry check for every [S] subkey on this host, notified through
  # Herdr the same way git-audit-worktrees is (home/modules/worktree.nix) —
  # not notify-send/libnotify, which nothing else in this repo declares.
  # The dangling cron this replaces (from the archived predecessor tool
  # above) had been failing silently for ~11 months; a systemd/launchd unit
  # shows up in `systemctl --user --failed` instead.
  systemd.user.services.gpg-subkey-remind = lib.mkIf pkgs.stdenv.isLinux {
    Unit.Description = "Warn when a GPG [S] signing subkey is close to expiry";
    Service = {
      Type = "oneshot";
      ExecStart = "${subkeyPath} remind --notify";
      Environment = "PATH=${
        lib.makeBinPath [
          pkgs.bash
          pkgs.coreutils
          pkgs.gawk
          pkgs.gnupg
          pkgs.herdr
        ]
      }";
    };
  };

  systemd.user.timers.gpg-subkey-remind = lib.mkIf pkgs.stdenv.isLinux {
    Unit.Description = "Check GPG [S] subkey expiry once a day";
    Timer = {
      OnCalendar = "daily";
      Persistent = true;
      Unit = "gpg-subkey-remind.service";
    };
    Install.WantedBy = [ "timers.target" ];
  };

  # launchd equivalent (ADR-0018) — see worktree.nix's git-audit-worktrees
  # agent for why EnvironmentVariables.PATH must list every binary the
  # script shells out to (launchd replaces PATH rather than extending it).
  launchd.agents.gpg-subkey-remind = lib.mkIf pkgs.stdenv.isDarwin {
    enable = true;
    config = {
      ProgramArguments = [
        subkeyPath
        "remind"
        "--notify"
      ];
      StartCalendarInterval = [
        {
          Hour = 9;
          Minute = 0;
        }
      ];
      EnvironmentVariables.PATH = lib.makeBinPath [
        pkgs.bash
        pkgs.coreutils
        pkgs.gawk
        pkgs.gnupg
        pkgs.herdr
      ];
    };
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
  #
  # Linger is a systemd --user concept with no darwin equivalent: a launchd
  # agent's lifetime is already tied to the login session by construction
  # (ADR-0018), so there is nothing here to assert on darwin. Guarded with
  # mkIf rather than left unconditional so ${pkgs.systemd} — Linux-only in
  # nixpkgs — is never forced on a darwin pkgs set.
  home.activation.assertNoLinger = lib.mkIf pkgs.stdenv.isLinux (
    lib.hm.dag.entryBefore [ "writeBoundary" ] ''
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
    ''
  );
}
