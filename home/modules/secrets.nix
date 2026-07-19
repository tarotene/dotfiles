# Secrets — SOPS runtime loader (#212 / ADR-0003).
#
# Wire the existing SOPS runtime loader through home-manager without changing
# the trust model.  Secrets stay runtime-decrypted in the interactive shell
# only; no sops-nix (activation-time decryption would force interactive PIN
# entry with a YubiKey recipient).
#
# The literal zsh module (35-secrets-sops.zsh) is already deployed by shell.nix
# as part of config/zsh/modules.  This module:
#   - provides the `sops` binary via home.packages;
#   - deploys the wrapper script (sops-secrets-env.sh) to ~/.local/bin/ so the
#     zsh module can find it at a stable, home-manager-managed path;
#   - keeps the MCP-gdrive credential generation helper (in the zsh module).
{ pkgs, ... }:
let
  repoScripts = ../../scripts;
in
{
  home.packages = [
    pkgs.sops
  ];

  # Deploy the wrapper script to ~/.local/bin (on PATH via 10-path.zsh).
  home.file.".local/bin/sops-secrets-env.sh" = {
    source = repoScripts + "/sops-secrets-env.sh";
    executable = true;
  };
}
