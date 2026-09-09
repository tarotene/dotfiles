# Identity layer — personal.
#
# Settings that follow the *person* on their personal machines, independent of
# which host they sit at.  Git user.name / user.email are set here; the
# per-machine signing key lives in the host module (#211 / ADR-0003).
{ lib, ... }:
{
  home.username = lib.mkDefault "tarotene";
  home.homeDirectory = lib.mkDefault "/home/tarotene";

  home.sessionVariables = {
    DOTFILES_IDENTITY = "personal";
  };

  # Git identity — personal (non-secret).
  programs.git.settings.user = {
    name = lib.mkDefault "Kentaro Sugimoto";
    email = lib.mkDefault "tarotene@gmail.com";
  };
}
