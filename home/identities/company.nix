# Identity layer — company (work).
#
# Settings that follow the *person* on their work machines.  Git user.name /
# user.email are set here; the per-machine signing key lives in the host
# module (#211 / ADR-0003).
{ lib, ... }:
{
  home.username = lib.mkDefault "tarotene";
  home.homeDirectory = lib.mkDefault "/home/tarotene";

  home.sessionVariables = {
    DOTFILES_IDENTITY = "company";
  };

  # Git identity — company (non-secret).
  programs.git.settings.user = {
    name = lib.mkDefault "Kentaro Sugimoto";
    email = lib.mkDefault "sugimoto-kentaro@arkedgespace.com";
  };

  # Default browser — Chrome on work machines. home-manager takes over
  # ~/.config/mimeapps.list; the DE writes that file on its own, so force
  # is needed to clobber the pre-existing copy (same on every company host).
  xdg.configFile."mimeapps.list".force = true;
  xdg.dataFile."applications/mimeapps.list".force = true;

  xdg.mimeApps = {
    enable = true;
    defaultApplications =
      let
        browser = [ "google-chrome.desktop" ];
      in
      {
        "text/html" = browser;
        "application/xhtml+xml" = browser;
        "x-scheme-handler/http" = browser;
        "x-scheme-handler/https" = browser;

        # Carried over from the pre-managed mimeapps.list: claude-cli://
        # deep links for Claude Code login (handler desktop file lives in
        # ~/.local/share/applications/, outside home-manager).
        "x-scheme-handler/claude-cli" = [ "claude-code-url-handler.desktop" ];
      };
  };
}
