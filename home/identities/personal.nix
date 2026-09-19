# Identity layer — personal.
#
# Settings that follow the *person* on their personal machines, independent of
# which host they sit at.  Git user.name / user.email are set here; the
# per-machine signing key lives in the host module (#211 / ADR-0003).
{ lib, ... }:
{
  imports = [
    # esa MCP token supply (ADR-0022). esa is a personal-identity service —
    # the token's gpg recipient is this identity's master fingerprint, which
    # GnuPG resolves to the YubiKey [E] subkey (ADR-0003 Amendment). A
    # company host's YubiKey cannot decrypt it, so this stays out of
    # common.nix: importing it there would just register an MCP server that
    # fails to start every session on company hosts.
    ../modules/esa.nix
  ];

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
