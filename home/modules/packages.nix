# User-space packages (#213).
#
# CLI tools live here in nixpkgs for reproducibility.  The system layer (apt)
# retains only what needs root or a system service — see #216.
# starship + sheldon come from their programs.* / shell module; direnv/mise/rustup
# are dev-runtime escape hatches (#215).
{ pkgs, ... }:
{
  home.packages = with pkgs; [
    # Core CLIs (were apt user-space)
    # git is provided by programs.git (#210) — not duplicated here.
    coreutils
    neovim
    vim
    tree
    curl
    wget
    gh
    shellcheck
    xsel
    zip
    unzip

    # Rust-tool CLIs (were cargo-binstall)
    bat
    zellij
    git-interactive-rebase-tool
    # siketyan/ghr (repo manager, `ghr cd` — what 30-tools-ghr.zsh expects).
    # Plain `pkgs.ghr` is tcnksm/ghr, a GitHub Release uploader. Binary: `ghr`.
    siketyan-ghr

    # Search tools
    ripgrep
    fd

    # Document conversion. The plan-view hook (home/modules/claude.nix) renders
    # plans to HTML with it. A stray /usr/bin/pandoc may exist on an inherited
    # host but is not declared in packages/declarative/apt-packages.txt, so the
    # declarative answer is to own it here (ADR-0001: user-space CLIs are the
    # home-manager layer). ~/.nix-profile/bin precedes /usr/bin on PATH.
    pandoc

    # AI tooling (unfree — flake sets allowUnfree; version follows the
    # nixpkgs pin, bump via `nix flake update`).
    # A native install at ~/.local/bin/claude (from Anthropic's official
    # installer) shadows this one because ~/.local/bin precedes
    # ~/.nix-profile/bin on PATH. If you inherit a host that had claude
    # installed natively, follow "Removing an ad-hoc native Claude Code
    # install" in docs/cutover-runbook.md.
    claude-code
  ];
}
