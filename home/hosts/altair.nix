# Instance layer — personal macOS host (2022 M2 MacBook Air, aarch64-darwin).
#
# First darwin host (ADR-0018). Star-codename (ADR-0019) instead of the
# <identity>-pop convention the three Linux hosts use — resolved via
# scripts/hms.sh / bootstrap.sh's resolve_host(), not the OS hostname.
{ ... }:
{
  imports = [
    ../common.nix
    ../identities/personal.nix
  ];

  # macOS home directory — overrides identities/personal.nix's
  # `mkDefault "/home/tarotene"`.
  home.homeDirectory = "/Users/tarotene";

  # Per-machine sign subkey (ADR-0003 Amendment 4), generated via
  # `gpg-subkey generate --usage sign` (docs/setup-macos.md §6). Still
  # needs importing on the physical machine before the first `hms` there.
  programs.git.signing.key = "464382A473897DEBF8BCB369F7F5798C1372F95D";

  # Declarative marker for resolve_host() (ADR-0019): once this activates,
  # `hms`/`bootstrap.sh` resolve this host as "altair" regardless of what
  # macOS reports as $(hostname). The very first bootstrap still needs the
  # marker hand-placed before activation can even select this host module —
  # see docs/setup-macos.md — but from the second switch onward, this
  # declaration is the marker's source of truth.
  xdg.configFile."dotfiles/host".text = "altair\n";
}
