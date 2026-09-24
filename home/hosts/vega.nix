# Instance layer — personal Pop!_OS host (star-codename: vega, #214).
#
# Star-codename (ADR-0019) instead of the old `personal-pop` <identity>-pop
# convention — resolved via scripts/hms.sh / bootstrap.sh's resolve_host(),
# not the OS hostname. Renamed from personal-pop; content otherwise
# unchanged (signing key, imports, ROS host module carried over verbatim).
{ lib, ... }:
{
  imports = [
    ../common.nix
    ../identities/personal.nix
    ../modules/obsidian.nix
  ];

  # Per-machine sign subkey. On-disk, annual rotation (ADR-0003 Amendment 3).
  # Master fp 1DCDC49510DCC9BF58C89751B7D596E9AA6F36E8 → [S] subkey created
  # 2026-09-22, expires 2027-09-22. Rotated via `gpg-subkey rotate`; the
  # previous subkey (…01E5FF8AC9A9306F) is not yet revoked (`--revoke-old`
  # needs the YubiKey touch/PIN, done separately) — its signature history
  # stays verifiable either way.
  programs.git.signing.key = "464382A473897DEBF8BCB369F7F5798C1372F95D";

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

  # Declarative marker for resolve_host() (ADR-0019): once this activates,
  # hms/bootstrap.sh resolve this host as "vega" even with no marker present
  # yet, as long as $(hostname) already reports "vega" (the rename runbook,
  # docs/cutover-runbook.md, sets the OS hostname first via `hostnamectl`
  # before running `hms`). From this switch onward, this declaration is the
  # marker's source of truth (ADR-0019 D3, Amendment 2).
  xdg.configFile."dotfiles/host".text = "vega\n";
}
