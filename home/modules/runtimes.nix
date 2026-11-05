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

  # Global mise config (#826): literal file, versions pinned (ADR-0035 axis 1).
  xdg.configFile."mise/config.toml".source = ../../config/mise/config.toml;

  # uv's package cache. Inside the sandbox it was read-only, so `uv sync` failed with
  # `Read-only file system (os error 30)` at ~/.cache/uv (measured 2026-10-29). The tool owns
  # this directory, so declare just it (docs/agents/agent-permissions.md, #1164). The matching
  # network destinations (pypi.org, files.pythonhosted.org) are in agent-permissions.nix.
  #
  # cargo's crate registry (the downloaded index and crate sources). Inside the sandbox it was
  # read-only, so `cargo metadata --offline --locked` (what `cargo run -p pipeline-map -- render`
  # starts with) failed with exit 101 on a write under ~/.cargo (measured 2026-11-05). Declare
  # just the registry, not ~/.cargo: `~/.cargo/bin` holds executables the sandbox must not be
  # able to replace. Fetching a crate that is not cached yet also needs crates.io hosts, which are
  # not in `domains`; that case is not measured yet.
  dotfiles.claudePermissions.allowWrite = [
    "~/.cache/uv"
    "~/.cargo/registry"
  ];

  home.packages = with pkgs; [
    # Runtime meta-tools; their shell hooks come from the zsh modules
    # (43-dev-mise, 44-dev-uv, 45-dev-deno). Java/Go consolidate into mise.
    # mise comes from the nixpkgs-unstable overlay (flake.nix, #823): worktree
    # trust sharing needs >= 2026.7.5, the stable channel has 2026.5.12.
    mise
    uv
    deno

    # Rust: rustup as the global default (cross + C-Rust FFI / embedded — ADR-0002).
    # Deliberately NOT adding nixpkgs `rustc`/`cargo` to avoid a competing shim.
    rustup
  ];
}
