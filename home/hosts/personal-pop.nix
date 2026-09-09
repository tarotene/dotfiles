# Instance layer — personal Pop!_OS host (hostname: personal-pop).
#
# Imports the shared base + the personal identity. Host-specific settings
# (hostname-scoped packages, ROS on personal only — ADR-0002, etc.) go here.
{ lib, ... }:
{
  imports = [
    ../common.nix
    ../identities/personal.nix
  ];

  # Per-machine sign subkey. On-disk, annual rotation (ADR-0003 amended).
  # Master fp 1DCDC49510DCC9BF58C89751B7D596E9AA6F36E8 → [S] subkey created
  # 2025-10-18, expires 2026-10-18.
  programs.git.signing.key = "26F56F7A73B2FD0FBF12D7FBEA9E735C451B8541";

  # ROS is scoped to the personal host only (#215 / ADR-0002): place the
  # host-scoped zsh module and source it after the shared modules. home-manager
  # loads the env (/opt/ros/* installed via apt/rosdep); nothing more.
  xdg.configFile."zsh/host.d/42-dev-ros.zsh".source = ../../config/zsh/host/personal/42-dev-ros.zsh;

  programs.zsh.initContent = lib.mkAfter ''
    for _hm in "''${XDG_CONFIG_HOME:-$HOME/.config}/zsh/host.d"/*.zsh(N); do
      [[ -r "$_hm" ]] && source "$_hm"
    done
    unset _hm
  '';
}
