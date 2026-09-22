# LINE — no Linux native client exists, and its own Firefox extension is
# retired; the official LINE Chrome extension (Chrome Web Store id
# ophjlpahpchlmihnnnihgmmeilfjmjjc) is the only officially supported browser
# channel (checked 2026-09-22). Running it inside a general-purpose Google
# Chrome, as a raw browser tab/popup, was rejected on two counts: it drags in
# a second full browser just for one app, and it does not behave like an
# independent app (no launcher entry, no standalone window, tab-strip/omnibox
# chrome around a chat client).
#
# This wraps nixpkgs' `chromium` (not Google Chrome — no closed-source
# browser needed) with the extension declared via home-manager's
# programs.chromium, and gives it a dedicated `--app=chrome-extension://…`
# launcher entry (config/applications/LINE.desktop) so it looks and starts
# like a normal app. A real Linux-native client would still be preferable —
# Waydroid + the Android LINE app is the closer equivalent — but personal-pop's
# kernel (6.17.4-76061704-generic) ships with
# CONFIG_ANDROID_BINDER_IPC unset, so Waydroid would need an out-of-tree DKMS
# binder module in the system layer, an unstable foundation to build on.
# Revisit once #249 (CachyOS migration, binder support in-tree) lands.
#
# Identity-scoped rather than in desktop.nix on purpose, same judgment as
# warp-terminal (#9) and esa (ADR-0022, see that module) — LINE is a personal
# messaging account, and desktop.nix is imported unconditionally by
# common.nix for every host, including company ones. Only imported from
# home/identities/personal.nix.
{ lib, pkgs, ... }:
let
  repoConfig = ../../config;

  # nixGL wrapper (#13 / ADR-0006), shared with desktop.nix / personal.nix's
  # warp-terminal entry. chromium is a GL consumer like alacritty/Chrome/Slack
  # — same /run/opengl-driver bootstrap problem on these Pop!_OS hosts.
  nixGLWrap = import ./nixgl.nix { inherit pkgs; };

  # Bound rather than inlined: both programs.chromium.package and the
  # launcher entry's Exec= need this exact derivation's store path
  # (ADR-0029 — a store path is what keeps "declared" and "running" from
  # diverging).
  chromiumPackage = nixGLWrap pkgs.chromium;
in
{
  # Linux only — darwin (altair) is out of scope for this module; LINE on
  # that host, if ever wanted, is a separate decision.
  config = lib.mkIf pkgs.stdenv.isLinux {
    programs.chromium = {
      enable = true;
      package = chromiumPackage;
      # commandLineArgs stays empty on purpose: home-manager's chromium
      # module only re-wraps `package` via .override when commandLineArgs is
      # non-empty (or KDE Plasma integration applies, which is irrelevant
      # here) — leaving it empty means finalPackage stays exactly
      # chromiumPackage, so the nixGL wrap survives untouched (checked
      # against home-manager's modules/programs/chromium.nix at the pinned
      # rev d4fd24667c8cbef124bb70a20380cab75ec8474d, 2026-09-22).
      extensions = [
        { id = "ophjlpahpchlmihnnnihgmmeilfjmjjc"; } # LINE
      ];
    };

    xdg.dataFile = {
      # LINE's own launcher entry — see config/applications/LINE.desktop for
      # the --app=chrome-extension://… command and the full rationale.
      "applications/LINE.desktop".source = pkgs.replaceVars (repoConfig + "/applications/LINE.desktop") {
        chromium = "${chromiumPackage}/bin/chromium";
      };

      # Hide the bare "Chromium" launcher entry nixpkgs' chromium ships
      # (share/applications/chromium-browser.desktop, confirmed by building
      # the pinned package and listing that directory, 2026-09-22). This
      # browser exists here only to host the LINE extension — surfacing it
      # as a second general-purpose browser in the launcher would reintroduce
      # exactly the "don't want a second browser visible" complaint this
      # module exists to avoid. NoDisplay=true is the XDG-specified way to
      # suppress an entry without deleting the file it shadows (same
      # ~/.local/share > nix-profile XDG_DATA_DIRS precedence as
      # desktop.nix's Alacritty.desktop override).
      "applications/chromium-browser.desktop".text = ''
        [Desktop Entry]
        Type=Application
        Name=Chromium (hidden — LINE-only install, see home/modules/line.nix)
        NoDisplay=true
      '';
    };
  };
}
