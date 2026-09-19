# User-space packages (#213).
#
# CLI tools live here in nixpkgs for reproducibility.  The system layer (apt)
# retains only what needs root or a system service — see #216.
# starship + sheldon come from their programs.* / shell module; direnv/mise/rustup
# are dev-runtime escape hatches (#215).
{ pkgs, lib, ... }:
{
  home.packages =
    with pkgs;
    [
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
      zip
      unzip

      # Used by direnv (`.envrc`) in projects outside this repo for per-file
      # GPG/age-based decryption — independent of the retired SOPS runtime
      # secrets loader (ADR-0010). Previously provided by the now-deleted
      # home/modules/secrets.nix; kept here as a plain user-space CLI.
      sops

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

      # Terminal-look capture for PR Before/After evidence (pr-description
      # skill, docs/claude/pr-description.md — G_visual in pr-gate.sh enforces
      # that evidence exists). Binary: `freeze`. Chosen over termshot (also in
      # stable) because freeze renders ANSI text piped on stdin
      # (`cmd | freeze -o out.svg`), so a "Before" state captured before a
      # change can still be turned into an image afterwards; termshot instead
      # re-executes the command live in a pty, which cannot reproduce a
      # already-lost pre-change state. Use `.svg` or `.webp` output, not
      # `.png` — this build's PNG encoder segfaults on this host (Go runtime
      # crash reproduced on trivial input; SVG/WebP unaffected). `gh --attach`
      # (>= 2.99.0, shipped in the pinned stable channel since #91) accepts
      # SVG/WebP along with PNG.
      charm-freeze

      # AI tooling (unfree — flake sets allowUnfree; version follows the
      # nixpkgs pin, bump via `nix flake update`).
      # A native install at ~/.local/bin/claude (from Anthropic's official
      # installer) shadows this one because ~/.local/bin precedes
      # ~/.nix-profile/bin on PATH. If you inherit a host that had claude
      # installed natively, follow "Removing an ad-hoc native Claude Code
      # install" in docs/cutover-runbook.md.
      claude-code
    ]
    # X11 clipboard CLI — meaningless on darwin (pbcopy/pbpaste are the OS
    # equivalent and already on PATH). Not referenced by anything under
    # config/, so dropping it on darwin is a pure subtraction, not a gap.
    ++ lib.optionals pkgs.stdenv.isLinux [ pkgs.xsel ];

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
  #
  # Linux-only, deliberately: detach-open.sh hardcodes `setsid -f
  # /usr/bin/xdg-open`, neither of which exists on darwin. macOS's own
  # `/usr/bin/open` already returns immediately (it hands off to
  # LaunchServices and exits), so there is no foreground-blocking problem to
  # work around there — shadowing it would only risk breaking a tool that
  # already works.
  home.file.".local/bin/open" = lib.mkIf pkgs.stdenv.isLinux {
    source = ../../scripts/detach-open.sh;
    executable = true;
  };
  home.file.".local/bin/xdg-open" = lib.mkIf pkgs.stdenv.isLinux {
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

  # github-audit: read-only cross-repository GitHub audit, unified across
  # five domains (rulesets/#130, charters, naming, settings, renovate —
  # ADR-0015; docs/github-audit.md). Replaces the former sibling scripts
  # github-audit-rulesets/github-audit-charters. Manual command, no timer —
  # unlike git-audit-worktrees this has no Herdr notification integration
  # yet, so it stays in packages.nix rather than worktree.nix's
  # systemd.user.services pattern.
  home.file.".local/bin/github-audit" = {
    source = ../../scripts/github-audit;
    executable = true;
  };

  # ADR-0020 closed vocabularies for github-audit's naming domain (PUBLIC
  # repos only — PRIVATE-repo entries live in a *.local.tsv sibling that
  # this module does not manage, written directly to disk instead).
  xdg.configFile."github-audit/codename-registry.tsv".source =
    ../../config/github-audit/codename-registry.tsv;
  xdg.configFile."github-audit/descriptive-species.tsv".source =
    ../../config/github-audit/descriptive-species.tsv;
  xdg.configFile."github-audit/site-domains.tsv".source = ../../config/github-audit/site-domains.tsv;

  # gh-stack 拡張(stacked-pr スキルが使う `gh stack link` の提供元、#124)を
  # 宣言的に管理する。`gh extension install` は ~/.local/share/gh/extensions/
  # へ直接 git clone するだけなので、これまで home-manager 管理外のまま当機に
  # 残っていた(ADR-0001 の source of truth 原則からのドリフト、#132)。--pin
  # でバージョンを固定し、home.packages と同等の再現性を得る。
  # herdr.nix の installHerdrClaudeIntegration と同じ形:
  # ファイル存在ゲート(二重インストールを避ける)+ `${pkgs.gh}/bin/gh` を
  # 直書き(activation の PATH は ~/.nix-profile/bin を含まないため `command -v`
  # は使えない、同ファイルのコメント参照)+ `|| true` で fail-open
  # (トークン未設定機・オフライン環境でも switch 自体は止めない)。
  home.activation.installGhStackExtension = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    if [ ! -e "$HOME/.local/share/gh/extensions/gh-stack" ]; then
      run ${pkgs.gh}/bin/gh extension install github/gh-stack --pin v0.1.1 || true
    fi
  '';
}
