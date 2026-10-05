# Identity layer — company (work).
#
# Settings that follow the *person* on their work machines.  Git user.name /
# user.email are set here; the per-machine signing key lives in the host
# module (#211 / ADR-0003).
{ lib, pkgs, ... }:
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
  #
  # xdg.mimeApps is Linux-only in home-manager (it asserts
  # `cfg.enable -> platforms.linux`, which fails eval outright on darwin) —
  # a future company-identity darwin host would need its own default-browser
  # mechanism (macOS has no mimeapps.list equivalent; LSHandlers/`duti` is
  # the closest analogue) rather than this block. No such host exists yet
  # (ADR-0018 covers only the personal-identity altair host), so this is
  # forward guarding, not a currently-exercised path.
  xdg.configFile."mimeapps.list".force = lib.mkIf pkgs.stdenv.isLinux true;
  xdg.dataFile."applications/mimeapps.list".force = lib.mkIf pkgs.stdenv.isLinux true;

  xdg.mimeApps = lib.mkIf pkgs.stdenv.isLinux {
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

  # Tailscale prefs (ADR-471): `arcturus` stays off the Mullvad exit node by
  # default — `dotfiles.tailscale.exitNode` is left at its empty default, which
  # still emits a clearing `--exit-node=` flag (crates/tailscale-prefs), so any
  # manual `tailscale set --exit-node=...` a previous café-Wi-Fi session left
  # set is reverted on the next `hms`. Select an exit node manually only while
  # actually on café Wi-Fi (docs/operations.md's "Café Wi-Fi" section) —
  # company traffic routing through a personal exit IP by default is not this
  # repo's call to make.
  dotfiles.tailscale.shieldsUp = true;

  # No Tailscale SSH server on the company host: a tagged device cannot SSH
  # into a user-owned device regardless (Tailscale tags KB), and the ACL's
  # `ssh` section deliberately omits tag:company.
  dotfiles.tailscale.ssh = false;
}
