# Identity layer — personal.
#
# Settings that follow the *person* on their personal machines, independent of
# which host they sit at.  Git user.name / user.email are set here; the
# per-machine signing key lives in the host module (#211 / ADR-0003).
{ lib, pkgs, ... }:
let
  # nixGL wrapper (#13 / ADR-0006), shared with desktop.nix. Only needed on
  # Linux — darwin has its own native GL stack.
  nixGLWrap = import ../modules/nixgl.nix { inherit pkgs; };
in
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

  # warp-terminal (#9): cloud-connected AI terminal. Identity-scoped rather
  # than in desktop.nix on purpose — desktop.nix is imported unconditionally
  # by common.nix for every host, including company ones, and a cloud AI
  # tool must not silently land there. Was previously installed ad hoc via
  # apt (a third-party source with an expired signing key that prints a
  # scary GPG warning on every `apt-get update`); this replaces that
  # unmanaged install, reclaiming it into the layer ADR-0001 says it
  # belongs in (unprivileged user-space GUI app → home-manager).
  #
  # Linux only for now, needing the nixGL wrap (same GL bootstrap problem as
  # alacritty/Chrome/Slack/Zoom in desktop.nix — Warp is GPU-accelerated and
  # looks for its driver under the NixOS-only /run/opengl-driver).
  #
  # darwin (altair) is excluded, not just left unwrapped: the pinned
  # nixpkgs' `warp-terminal` derivation fails to build there entirely.
  # Upstream switched to an APFS-formatted .dmg, and nixpkgs' `undmg`-based
  # unpackPhase only understands HFS+ ("only HFS file systems are
  # supported") — a currently-open nixpkgs bug
  # (https://github.com/NixOS/nixpkgs/issues/516928, confirmed against this
  # pin by the `build altair` CI job failing with that exact message).
  # Revisit once the pinned channel picks up a fix.
  home.packages = lib.optionals pkgs.stdenv.isLinux [ (nixGLWrap pkgs.warp-terminal) ];

  # Git identity — personal (non-secret).
  programs.git.settings.user = {
    name = lib.mkDefault "Kentaro Sugimoto";
    email = lib.mkDefault "tarotene@gmail.com";
  };
}
