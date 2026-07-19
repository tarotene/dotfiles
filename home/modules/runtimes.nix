# Dev-runtime escape hatches (#215) — ADR-0002 runtime boundary.
#
# Install the runtime *meta-tools* from nixpkgs; project versions stay outside
# the repo (mise/direnv) and the global Rust toolchain stays under rustup.
{ pkgs, ... }:
{
  # direnv + nix-direnv. The literal 41-dev-direnv.zsh already runs the hook, so
  # home-manager's own zsh integration is disabled to avoid a double hook.
  programs.direnv = {
    enable = true;
    nix-direnv.enable = true;
    enableZshIntegration = false;
  };

  home.packages = with pkgs; [
    # Runtime meta-tools; their shell hooks come from the zsh modules
    # (43-dev-mise, 44-dev-uv, 45-dev-deno). Java/Go consolidate into mise.
    mise
    uv
    deno

    # Rust: rustup as the global default (cross + C-Rust FFI / embedded — ADR-0002).
    # Deliberately NOT adding nixpkgs `rustc`/`cargo` to avoid a competing shim.
    rustup
  ];
}
