# Terminal & desktop (#214) — alacritty, fcitx5 wiring, fonts.
#
# Hybrid translation (ADR-0002): config files stay literal via xdg.configFile.
# The fcitx5 *packages* stay an apt escape hatch (system input method); only the
# user-level env/autostart/profile wiring is managed here.
#
# How nix-installed GUI apps actually reach fcitx5 — and what was measured
# rather than assumed — is written up in docs/ime-chrome-diagnosis.md.
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
    #
    # Chrome 149 already runs as a native Wayland client and already binds
    # zwp_text_input_v3 — measured with WAYLAND_DEBUG=1, see
    # docs/ime-chrome-diagnosis.md. Adding --ozone-platform-hint=auto /
    # --enable-wayland-ime / --wayland-text-input-version=3 changes nothing at
    # the protocol level, so do not add them expecting to fix Japanese input.
    # Slack Desktop is native Wayland too (`ozone-platform=wayland`); with all
    # three of these running, `xprop -root _NET_CLIENT_LIST` is empty, so nothing
    # here is on XWayland or XIM.
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

  # Deliberately NOT here: a service that restarts fcitx5 on logind's
  # Session.Lock, to dodge the trigger-key failure that clusters after an unlock.
  # It was tried and withdrawn — it made the failure permanent instead of
  # transient. See docs/ime-chrome-diagnosis.md ("Withdrawn").

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

  # Same seed-if-absent treatment for fcitx5's main config, which fcitx5 also
  # rewrites at runtime. The value worth carrying to a new machine is
  # ShareInputState=All: with the default (No) every focus change drops the
  # input method back to inactive, so Ctrl+Space has to be pressed constantly —
  # and fcitx5 sometimes swallows the trigger key entirely (see
  # docs/ime-chrome-diagnosis.md and issue #14). Sharing the state across
  # applications cuts how often the key must be pressed, which is the only lever
  # available locally; it does not make any individual press more reliable.
  # ActiveByDefault stays False on purpose: True would start terminals and the
  # omnibox in Japanese mode.
  home.activation.seedFcitx5Config = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    cfg="''${XDG_CONFIG_HOME:-$HOME/.config}/fcitx5/config"
    if [ ! -e "$cfg" ]; then
      run install -D -m 600 ${repoConfig + "/fcitx5/config"} "$cfg"
    fi
  '';

  # Quarantine the pre-migration hand-placed autostart entry. Both it and the
  # home-managed fcitx5.desktop become app-*@autostart.service units execing
  # /usr/bin/fcitx5, so they race at login and the loser dies with "Unable to
  # request dbus name" — leaving the surviving daemon nondeterministically
  # either the managed or the unmanaged one. Renamed rather than deleted: the
  # file is outside home-manager's ownership, and the generator only picks up
  # *.desktop, so a .bak suffix is enough to retire it.
  home.activation.quarantineStrayFcitx5Autostart = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    stray="''${XDG_CONFIG_HOME:-$HOME/.config}/autostart/org.fcitx.Fcitx5.desktop"
    if [ -e "$stray" ] || [ -L "$stray" ]; then
      run mv -f "$stray" "$stray.bak"
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
