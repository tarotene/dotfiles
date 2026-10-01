# Instance layer — personal Pop!_OS host (star-codename: vega, #214).
#
# Star-codename (ADR-0019) instead of the old `personal-pop` <identity>-pop
# convention — resolved via crates/hms / bootstrap.sh's resolve_host(),
# not the OS hostname. Renamed from personal-pop; content otherwise
# unchanged (signing key, imports, ROS host module carried over verbatim).
{ lib, ... }:
{
  imports = [
    ../common.nix
    ../identities/personal.nix
    ../modules/obsidian.nix
    ../modules/github-apps.nix
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

  # vega-only apt escape hatch, reclaimed from `detect-drift`'s ad-hoc
  # inventory per dotfiles#4's classification rule ("2. intentional escape
  # hatch, reason documented"). Each group below needs root, a kernel
  # module, a display-stack/session-manager piece, or a GUI app whose
  # nixGL wrapping hasn't been verified yet, so none of these are
  # candidates for `home/modules/packages.nix`.
  dotfiles.apt.extraPackages = [
    "" # spacer before the first reasoned group
    "# System76 hardware / display-manager stack: kernel-adjacent"
    "# (DKMS) or the greeter that owns the login session itself — both need to be the"
    "# same binary the OS boots with, not a Nix-store path."
    "system76-driver-nvidia" # System76 GPU driver integration (DKMS)
    "lightdm" # display manager / greeter (session-manager, not a CLI)

    ""
    "# Kernel modules (DKMS/out-of-tree) — must build against the running kernel"
    "# headers, which is squarely apt's job, not nixpkgs'."
    "v4l2loopback-dkms"
    "v4l2loopback-utils" # companion CLI for the module above

    ""
    "# Virtualization stack: libvirtd is a root system service (like tailscaled,"
    "# ADR-471); virt-manager/bridge-utils are its client-side companions and stay"
    "# with it rather than splitting across layers."
    "libvirt-daemon-system"
    "libvirt-clients"
    "virt-manager"
    "bridge-utils"

    ""
    "# GNOME Software's flatpak plugin: extends a system D-Bus service (packagekit),"
    "# not a standalone CLI."
    "gnome-software-plugin-flatpak"

    ""
    "# ROS 2 (Kilted) desktop tools: apt is ROS's own supported install path"
    "# (ros2-apt-source, see this host's ROS zsh module below); packaging this"
    "# through nixpkgs would fight ROS's own dependency resolution instead of"
    "# reusing it."
    "ros-kilted-desktop"

    ""
    "# Firmware/hardware utilities: mokutil manipulates the UEFI Secure Boot MOK"
    "# list (needs to match the running bootloader); memtester exercises physical"
    "# RAM directly. Neither benefits from a Nix-store indirection."
    "mokutil"
    "memtester"

    ""
    "# Build-toolchain -dev headers: apt-installed GTK/Qt/X11 applications built"
    "# on this host (kicad, below) link against these apt-managed system"
    "# libraries at compile time — same rationale as the shared file's own"
    "# build-essential/libudev-dev/pkg-config entries (ADR-0001 Amendment)."
    "# The build orchestrator itself (cmake) has no such system-library tie and"
    "# is declared in home.packages instead (packages.nix)."
    "libfontconfig1-dev"
    "libfreetype-dev"
    "libssl-dev"
    "libusb-1.0-0-dev"
    "libxcb-xfixes0-dev"
    "libxkbcommon-dev"

    ""
    "# OCR (tesseract-ocr + Japanese trained data): reclassified from"
    "# home.packages to this escape hatch — nixpkgs'"
    "# tesseract5 ships NO trained language data at all (verified against"
    "# pkgs/applications/graphics/tesseract/tesseract5.nix, no postInstall/"
    "# TESSDATA_PREFIX wiring, no separate tessdata package in nixpkgs), so"
    "# using it would mean hand-rolling a traineddata-fetching derivation."
    "# apt's tesseract-ocr-jpn already bundles the trained data as a normal"
    "# package dependency — the cheaper solution wins (ADR-0035 reduction axis)."
    "tesseract-ocr"
    "tesseract-ocr-jpn"

    ""
    "# GUI applications: nixGL-wrapped OpenGL/Vulkan rendering"
    "# (home/modules/desktop.nix's alacritty/chromium/warp-terminal precedent) has"
    "# not been verified for these yet — kicad and darktable in particular use"
    "# OpenGL canvases whose behavior under nixGL is unconfirmed. Revisit as"
    "# home.packages once verified working."
    "kicad"
    "darktable"
    "zulip"
    "antigravity"
    "firefox-trunk"
    "arandr"
  ];
}
