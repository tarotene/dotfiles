# Terminal & desktop (#214) — alacritty, fcitx5 wiring, fonts.
#
# Hybrid translation (ADR-0002): config files stay literal via xdg.configFile.
# The fcitx5 *packages* stay an apt escape hatch (system input method); only the
# user-level env/autostart/profile wiring is managed here.
#
# The fcitx5 profile is seeded once, not symlinked (fcitx5 rewrites it at
# runtime). If fcitx5 wrote its default profile before the first switch,
# recover with: rm ~/.config/fcitx5/profile && home-manager switch && fcitx5 -r
{ lib, pkgs, ... }:
let
  repoConfig = ../../config;
in
{
  home.packages = [
    pkgs.alacritty
    # FiraCode Nerd Font from nixpkgs (retires scripts/install-firacode-font.sh).
    pkgs.nerd-fonts.fira-code

    # GUI apps (unfree; allowUnfree is set at the flake's pkgs import).
    # All three run under XWayland by default; fcitx5 input works via the
    # GTK/QT_IM_MODULE vars in environment.d/10-fcitx5.conf. If screenshare
    # or fractional scaling misbehaves, set NIXOS_OZONE_WL=1 in environment.d
    # — the nixpkgs chrome/slack wrappers then add --ozone-platform-hint=auto
    # --enable-wayland-ime (re-verify Japanese input after flipping it).
    pkgs.google-chrome
    pkgs.slack
    pkgs.zoom-us
  ];

  # Make the home-managed font discoverable by fontconfig.
  fonts.fontconfig.enable = true;

  xdg.configFile = {
    # Terminal — literal alacritty.toml.
    "alacritty/alacritty.toml".source = repoConfig + "/alacritty/alacritty.toml";

    # fcitx5 wiring (env + autostart). The fcitx5 packages stay apt (#216).
    "autostart/fcitx5.desktop".source = repoConfig + "/autostart/fcitx5.desktop";
    "environment.d/10-fcitx5.conf".source = repoConfig + "/environment.d/10-fcitx5.conf";

    # Session locale: LC_CTYPE=ja for JP glyph fallback (UI stays English).
    "environment.d/20-locale.conf".source = repoConfig + "/environment.d/20-locale.conf";
  };

  # X11-session fallback; Wayland/COSMIC relies on environment.d + autostart.
  # im-config's 23_fcitx5.rc starts /usr/bin/fcitx5 and sets the IM variables.
  home.file.".xinputrc".text = "run_im fcitx5\n";

  # JP-capable terminal font for cosmic-term. cosmic-text has no glyph
  # fallback for fullwidth Latin (U+FF00 block), so the configured font must
  # cover it itself or preedit text renders in a random (serif) CJK face.
  # Symlinked into ~/.local/share/fonts because cosmic-term enumerates fonts
  # via fontdb, which scans XDG data dirs but not the nix profile.
  xdg.dataFile."fonts/udev-gothic-nf".source = "${pkgs.udev-gothic-nf}/share/fonts";

  # Seed the fcitx5 input-method profile (Mozc in the default group) only when
  # absent — fcitx5 owns the file afterwards, so a store symlink would conflict.
  home.activation.seedFcitx5Profile = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    profile="''${XDG_CONFIG_HOME:-$HOME/.config}/fcitx5/profile"
    if [ ! -e "$profile" ]; then
      run install -D -m 600 ${repoConfig + "/fcitx5/profile"} "$profile"
    fi
  '';

  # Seed cosmic-term's font (COSMIC settings rewrite these files at runtime,
  # so seed-if-missing like the fcitx5 profile above). Restart cosmic-term to
  # pick up newly installed fonts — fontdb only enumerates at startup.
  home.activation.seedCosmicTermFont = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    font_name="''${XDG_CONFIG_HOME:-$HOME/.config}/cosmic/com.system76.CosmicTerm/v1/font_name"
    if [ ! -e "$font_name" ]; then
      run install -D -m 644 ${repoConfig + "/cosmic-term/font_name"} "$font_name"
    fi
  '';
}
