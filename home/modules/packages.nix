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
    # Declares jq for the Claude Code hooks (config/claude/hooks/*.sh) that
    # depend on it unconditionally — a stray /usr/bin/jq may exist on an
    # inherited host but was never declared anywhere (#28), so hooks would
    # silently emit nothing on a freshly provisioned machine.
    jq
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

  # The canonical apply wrapper (docs/operations.md).  Deployed to ~/.local/bin
  # (on PATH via 10-path.zsh) so `hms` works from any directory — the whole
  # point is not depending on being inside a checkout.
  home.file.".local/bin/hms" = {
    source = ../../scripts/hms.sh;
    executable = true;
  };

  # Shadow the system `open`/`xdg-open` (both resolve to xdg-utils 1.1.3,
  # which blocks in the foreground on COSMIC — unrecognized DE → generic
  # mode execs the MIME handler's Exec directly, so Ctrl+C kills the
  # viewer/browser along with the blocked shell). ~/.local/bin precedes
  # /usr/bin on PATH (10-path.zsh), so both names resolve here instead.
  # Also the $BROWSER target (config/shell/common_env exports it) — gh
  # browse and anything else honoring $BROWSER wait for it to exit, so it
  # needs the same detaching behavior.
  home.file.".local/bin/open" = {
    source = ../../scripts/detach-open.sh;
    executable = true;
  };
  home.file.".local/bin/xdg-open" = {
    source = ../../scripts/detach-open.sh;
    executable = true;
  };

  # git-shelve / git-unshelve: worktree 単位で所有権が分かる stash の
  # ラッパー(docs/claude/git-stash-guard.md)。~/.local/bin に置くだけで
  # git のサブコマンド解決に乗り、`git shelve` / `git unshelve` と呼べる
  # (alias 不要)。config/claude/hooks/git-stash-guard.sh の deny 案内が
  # ここへ誘導する。
  home.file.".local/bin/git-shelve" = {
    source = ../../scripts/git-shelve;
    executable = true;
  };
  home.file.".local/bin/git-unshelve" = {
    source = ../../scripts/git-unshelve;
    executable = true;
  };

  # git-prune-branches: delete local branches whose upstream is [gone]
  # (docs/git-sync.md). Same "executable in ~/.local/bin, no alias needed"
  # placement as git-shelve/git-unshelve above — used to be a
  # `config/git/hooks/prune-branches.sh` + `alias.prune-branches` pair, but
  # it isn't a git hook and doesn't need core.hooksPath's indirection.
  home.file.".local/bin/git-prune-branches" = {
    source = ../../scripts/git-prune-branches;
    executable = true;
  };
}
