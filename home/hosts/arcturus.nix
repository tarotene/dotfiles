# Instance layer — company Pop!_OS host (star-codename: arcturus, #214).
#
# Star-codename (ADR-0019) instead of the old `company-pop-new` <identity>-
# pop convention — resolved via scripts/hms.sh / bootstrap.sh's
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

  # Per-machine sign subkey on this host. On-disk, annual rotation
  # (ADR-0003 amended). Master fp 92E7B05978F0FE4E5500F6F76CFC837175BE257E →
  # [S] subkey created 2026-07-13, expires 2027-07-13.
  programs.git.signing.key = "57B25182FB450B06570860488608A3F925E329CC";

  # Declarative marker for resolve_host() (ADR-0019): once this activates,
  # hms/bootstrap.sh resolve this host as "arcturus" regardless of what the
  # OS reports as $(hostname). Hand-place the marker once before the first
  # switch under the new name (`echo arcturus > ~/.config/dotfiles/host`) —
  # from the second switch onward, this declaration is the marker's source
  # of truth (same bootstrap sequencing as altair.nix, ADR-0019 D3).
  xdg.configFile."dotfiles/host".text = "arcturus\n";
}
