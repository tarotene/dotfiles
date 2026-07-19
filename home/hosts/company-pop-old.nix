# Instance layer — old company Pop!_OS host (hostname: company-pop-old).
#
# Stage 2 target (#207): in-place cutover from procedural dotfiles to
# home-manager. The existing on-disk [S] sign subkey is reused as-is.
#
# Imports the shared base + the company identity. Host-specific settings
# (cross / C-Rust FFI toolchains — ADR-0002, etc.) go here.
{ ... }:
{
  imports = [
    ../common.nix
    ../identities/company.nix
  ];

  # Existing per-machine sign subkey on this host. On-disk, annual rotation
  # (ADR-0003 amended). Master fp 92E7B05978F0FE4E5500F6F76CFC837175BE257E →
  # [S] subkey created 2025-12-01, expires 2026-12-01.
  programs.git.signing.key = "FC1FD255E55AE983D7DBDE3118A3B92ADAFFB79C";
}
