# Shell (#209) — zsh modules, starship, sheldon.
#
# Hybrid translation (ADR-0002): the working config files are kept *literal* and
# referenced via xdg.configFile; Nix DSL is used only to orchestrate init and to
# load starship's existing TOML. The login shell is the Nix-provided zsh
# (~/.nix-profile/bin/zsh, from home.packages below) — bootstrap.sh registers
# it in /etc/shells and prints the `chsh` command to activate it (#245).
#
# SHELL is pinned explicitly below via both home.sessionVariables (sourced by
# hm-session-vars.sh — shells that source it directly) and
# systemd.user.sessionVariables (written to ~/.config/environment.d, read by
# the systemd --user manager at login). GUI-launched terminals (Cosmic
# Terminal, Ghostty, ...) get their $SHELL from systemd --user, which caches
# it at login and only re-reads /etc/passwd on the *next* login — without the
# systemd.user half, a `chsh` leaves them spawning the old shell until a full
# logout/login (see config/i18n.nix for the same two-option pattern used for
# LOCALE_ARCHIVE).
{
  lib,
  pkgs,
  config,
  ...
}:
let
  repoConfig = ../../config;
  loginShell = "${config.home.homeDirectory}/.nix-profile/bin/zsh";
in
{
  home.sessionVariables.SHELL = loginShell;
  systemd.user.sessionVariables.SHELL = loginShell;

  programs.zsh = {
    enable = true;

    # Orchestrate init: source every literal module under zsh/modules in
    # numerical order, then the machine-local file if present — the same loading
    # scheme config/zsh/.zshrc uses.
    initContent = ''
      ZSHRC_MODULE_DIR="''${XDG_CONFIG_HOME:-$HOME/.config}/zsh/modules"
      if [[ -d "$ZSHRC_MODULE_DIR" ]]; then
        for module in "$ZSHRC_MODULE_DIR"/*.zsh(N); do
          [[ -r "$module" ]] && source "$module"
        done
      fi
      if [[ -r "''${XDG_CONFIG_HOME:-$HOME/.config}/zsh/.zshrc.local" ]]; then
        source "''${XDG_CONFIG_HOME:-$HOME/.config}/zsh/.zshrc.local"
      fi
    '';
  };

  # Starship: provide the binary, and import the existing starship.toml as the
  # settings source of truth. Note `programs.starship.settings` re-serialises
  # these into a generated config (the original file's comments/formatting are
  # not preserved), so the TOML drives the config rather than being deployed
  # verbatim. The literal module 51-tools-starship.zsh performs `starship init
  # zsh`, so home-manager's own zsh integration is disabled to avoid double init.
  programs.starship = {
    enable = true;
    enableZshIntegration = false;
    settings = lib.importTOML (repoConfig + "/starship.toml");
  };

  # Sheldon stays literal for now (nixification-roadmap.md): provide the binary,
  # let the literal module 50-tools-sheldon.zsh source it. pkgs.zsh is what
  # makes ~/.nix-profile/bin/zsh exist as the login-shell binary (#245).
  home.packages = [
    pkgs.sheldon
    pkgs.zsh
  ];

  # Literal config files, placed verbatim under ~/.config.
  xdg.configFile = {
    "zsh/modules".source = repoConfig + "/zsh/modules";
    "zsh/.zshrc.local".source = repoConfig + "/zsh/.zshrc.local";
    "shell/common_env".source = repoConfig + "/shell/common_env";
    "sheldon/plugins.toml".source = repoConfig + "/sheldon/plugins.toml";
  };
}
