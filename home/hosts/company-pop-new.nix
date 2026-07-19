# Instance layer — new company Pop!_OS host (hostname: company-pop-new).
#
# Stage 3 target (#207): greenfield provisioning via bootstrap.sh. The on-disk
# [S] sign subkey was cut on this machine during the manual provisioning step
# (gpg.nix header / ADR-0003 amended).
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
}
