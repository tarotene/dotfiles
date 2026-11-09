# Instance layer — company Pop!_OS host (star-codename: arcturus, #214).
#
# Star-codename (ADR-0019) instead of the old `company-pop-new` <identity>-
# pop convention — resolved via crates/hms / bootstrap.sh's
# resolve_host(), not the OS hostname. Renamed from company-pop-new;
# content otherwise unchanged (signing key, imports carried over verbatim).
# The retired `company-pop-old` host was deleted outright rather than
# renamed (#214) — it was already decommissioned, so no replacement module
# was created for it.
{ ... }:
{
  imports = [
    ../common.nix
    ../identities/company.nix
  ];

  dotfiles.feedbackTrial.collector.enable = true;
  dotfiles.obsidian.initializeVault = true;
  # The workplace machine: the only host that checks the calendar handshake for
  # drafts waiting to be written (docs/agents/calendar-handshake.md, #1239).
  dotfiles.calendarHandshakeReminder.enable = true;

  # Built-in camera (Intel IPU7 + ov08x40): the virtual /dev/video40 the apps
  # open comes from the v4l2loopback kernel module, which is out-of-tree and
  # code loaded into the kernel — so apt/DKMS, not nix (ADR-0001 Amendment).
  # Its options and load list, and the udmabuf udev rule, are deployed from
  # config/ by scripts/install-packages.sh; the relay that feeds it is
  # home/modules/webcam-builtin.nix. (vega dropped v4l2loopback in #956 — no
  # consumer there; this host has one.)
  dotfiles.webcam.builtin.enable = true;
  dotfiles.apt.extraPackages = [
    "# v4l2loopback: virtual camera device for the built-in IPU7 camera, whose"
    "# /dev/video0-31 only yield raw Bayer (docs/webcam-builtin-ipu7.md)."
    "v4l2loopback-dkms"
  ];

  # Per-machine sign subkey on this host. On-disk, annual rotation
  # (ADR-0003 amended). Master fp 92E7B05978F0FE4E5500F6F76CFC837175BE257E →
  # [S] subkey created 2026-07-13, expires 2027-07-13.
  programs.git.signing.key = "57B25182FB450B06570860488608A3F925E329CC";

  # This host's name. home/modules/host.nix writes it as the declarative
  # marker for resolve_host() (ADR-0019): once this activates,
  # hms/bootstrap.sh resolve this host as "arcturus" even with no marker
  # present yet, as long as $(hostname) already reports "arcturus" (the
  # rename runbook, docs/cutover-runbook.md, sets the OS hostname first via
  # `hostnamectl` before running `hms`). From this switch onward, this
  # declaration is the marker's source of truth (ADR-0019 D3, Amendment 2).
  dotfiles.host.name = "arcturus";
}
