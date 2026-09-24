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
      # Per-file GPG/age decryption for SOPS-managed secrets in private repos
      # outside this one (direnv `.envrc`, their scripts and skills call `sops`
      # directly). Removed by ADR-0022 Decision 6 on the premise that no such
      # consumer remained; that premise was wrong, so ADR-0022's Amendment
      # (#451) restores it as a plain user-space CLI.
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

      # Embedded flashing/debugging CLI (probe-rs, cargo-flash, cargo-embed
      # in one package). Unprivileged user-space CLI, so home-manager is the
      # right layer (ADR-0001) — the apt system layer already provisions its
      # device permissions (packages/declarative/apt-packages.txt's
      # gcc-arm-none-eabi/libnewlib-*/build-essential/libudev-dev/pkg-config,
      # plus /etc/udev/rules.d/69-probe-rs.rules from install-packages.sh),
      # but nothing previously installed the tool itself (#20).
      probe-rs-tools

      # AI tooling (unfree — flake sets allowUnfree; version follows the
      # nixpkgs pin, bump via `nix flake update`).
      # A native install at ~/.local/bin/claude (from Anthropic's official
      # installer) shadows this one because ~/.local/bin precedes
      # ~/.nix-profile/bin on PATH. If you inherit a host that had claude
      # installed natively, follow "Removing an ad-hoc native Claude Code
      # install" in docs/cutover-runbook.md.
      claude-code

      # Declarative Gmail filter management for a personal filters repo,
      # built on mbrt/gmailctl. Unlike vhs (a one-shot-per-PR tool used
      # elsewhere that falls back to `nix shell nixpkgs#vhs` when absent
      # from PATH), gmailctl's diff/apply/edit are run repeatedly during
      # normal filter maintenance, so it's declared here instead of
      # re-fetched per invocation.
      gmailctl

      # Google Apps Script (GAS) CLI, official (google/clasp). Lets GAS
      # projects be pushed/run from the terminal instead of copy-pasting
      # into script.google.com and reading results off the browser
      # execution log (ADR-0030 — a GAS script's Logger-only URL output was
      # missed and cost a re-run). Setup/login/day-to-day usage is the
      # gas-clasp-ops skill (docs/claude/gas-clasp-ops.md); credentials stay
      # host-local, not managed here.
      google-clasp

      # Layer 1(Issue #4): ad-hoc install の動機そのものを減らす — 試用の
      # 摩擦が高いと「とりあえず apt/cargo/npm/pipx」に流れる。
      # comma(`, <cmd>`)は nixpkgs のパッケージをインストールせず一度だけ
      # 実行する。nix-index は `nix-locate` を提供し、Layer 2 の
      # `detect-drift --porcelain` がコマンド名→nixpkgs 属性の候補注記に
      # 使う(ADR-0005: nix-locate が無ければ注記なしで黙って続行)。
      # 初回のみ手動で `nix-index` を実行してローカル DB を作る必要がある
      # (`nix-community/nix-index-database` の事前ビルド版は新規 flake
      # input になり撤収コストが導入コストを上回るため見送った、
      # docs/adr/0035-selection-grounding.md の3軸)。
      comma
      nix-index
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

  # pr-title-check: checker 単一ソース for the PR-title commit-message
  # contract (ADR-0031, docs/claude/pr-title-contract.md). Both
  # config/claude/hooks/pr-title-guard.sh (client-side PreToolUse deny) and
  # .github/workflows/pr-title.yml (server-side required check) call this
  # one script, so the grammar never drifts between the two enforcement
  # points. Same "executable in ~/.local/bin, no alias needed" placement as
  # git-prune-branches/github-audit above.
  home.file.".local/bin/pr-title-check" = {
    source = ../../scripts/pr-title-check;
    executable = true;
  };

  # adr-number-check: checker 単一ソース for ADR 採番規約 (ADR-380,
  # docs/claude/adr-numbering.md — 番号を導入 PR の番号にすることで採番
  # 衝突を構造的に不可能にする決定). Both .github/workflows/ci.yml's
  # required check and (once wired) config/claude/hooks/adr-number.sh call
  # this one script, so the rule never drifts between the two enforcement
  # points. Same "executable in ~/.local/bin, no alias needed" placement as
  # pr-title-check/git-prune-branches above.
  home.file.".local/bin/adr-number-check" = {
    source = ../../scripts/adr-number-check;
    executable = true;
  };

  # decision-colocation-check: checker 単一ソース for the ADR-396 decision-
  # colocation rule (docs/claude/decision-colocation.md — 決定成果物
  # (ADR/設計文書/skill)の新規追加、または既存 ADR への `## Amendment`
  # 追加に、その決定を執行する実ファイルの同梱を要求する決定). Both
  # config/claude/hooks/decision-colocation-guard.sh (client-side PreToolUse
  # deny) and .github/workflows/ci.yml's required check call this one
  # script, so the rule never drifts between the two enforcement points.
  # Same "executable in ~/.local/bin, no alias needed" placement as
  # pr-title-check/adr-number-check above.
  home.file.".local/bin/decision-colocation-check" = {
    source = ../../scripts/decision-colocation-check;
    executable = true;
  };

  # github-audit: read-only cross-repository GitHub audit, unified across
  # six domains (rulesets/#130, charters, naming, settings, renovate,
  # titles/ADR-0031 — ADR-0015; docs/github-audit.md). Replaces the former
  # sibling scripts
  # github-audit-rulesets/github-audit-charters. Manual command, no timer —
  # unlike git-audit-worktrees this has no Herdr notification integration
  # yet, so it stays in packages.nix rather than worktree.nix's
  # systemd.user.services pattern.
  home.file.".local/bin/github-audit" = {
    source = ../../scripts/github-audit;
    executable = true;
  };

  # writing-style-hub: resolves the private style-guide hub's absolute path
  # (marker file or env var indirection — never hardcoded, #115) for the
  # writing-style skill. docs/claude/writing-style.md has the design.
  home.file.".local/bin/writing-style-hub" = {
    source = ../../scripts/writing-style-hub;
    executable = true;
  };

  # update-own-tools (ADR-0025, #276): builds self-authored, not-yet-released
  # CLIs from origin/<branch> per a host-local registry
  # (~/.config/update-own-tools/registry.toml — never in this repo). Rust,
  # from crates/update-own-tools via pkgs.dotfiles-tools (ADR-0024);
  # docs/update-own-tools.md has the schema and the exit procedure.
  home.file.".local/bin/update-own-tools".source = "${pkgs.dotfiles-tools}/bin/update-own-tools";

  # detect-drift(Issue #4)の配備 + systemd/launchd timer は
  # home/modules/drift.nix にまとめてある(worktree.nix が
  # git-audit-worktrees をタイマーと同居させているのと同じ理由 —
  # git-audit-worktrees/dotfiles-doctor/github-rulesets-apply のように
  # タイマーを持たない手動コマンドはここ packages.nix に留める)。

  # dotfiles-doctor: reports the presence/resolvability of every host-local
  # marker this repo resolves indirectly (dotfiles/host ADR-0019,
  # dotfiles/private-hub ADR-0034, dotfiles/style-hub #115/#368) without
  # ever writing a real value itself (ADR-0034 D5: schema is public, values
  # are private). Detector only, manual command, no timer.
  home.file.".local/bin/dotfiles-doctor" = {
    source = ../../scripts/dotfiles-doctor;
    executable = true;
  };

  # github-rulesets-apply: seeds the standard Security/Quality/Workflow
  # rulesets (ADR-0021 core layer) onto one or more repositories by driving
  # the matching *-repo-governance skill's apply-rulesets.sh (#153). Owns no
  # ruleset logic itself — a thin dispatcher over the skills' rulesets/*.json.
  # Manual command, no timer.
  home.file.".local/bin/github-rulesets-apply" = {
    source = ../../scripts/github-rulesets-apply;
    executable = true;
  };

  # apply-rulesets.sh(#349)自身の PATH 配備(#417)。配備前は
  # `github-rulesets-apply dotfiles ...` が呼ぶ既定 self-apply script
  # ($SCRIPT_DIR/apply-rulesets.sh)が存在せず必ず失敗していた。
  # rulesets/*.json(下の xdg.configFile)も併せて配備しないと、
  # デプロイ先には REPO_ROOT/rulesets が存在しないため RULESETS_DIR の
  # 解決先が無くなる(スクリプト側の 3 段フォールバックに対応)。
  home.file.".local/bin/apply-rulesets.sh" = {
    source = ../../scripts/apply-rulesets.sh;
    executable = true;
  };
  xdg.configFile."dotfiles/rulesets/security.json".source = ../../rulesets/security.json;
  xdg.configFile."dotfiles/rulesets/quality.json".source = ../../rulesets/quality.json;
  xdg.configFile."dotfiles/rulesets/workflow.json".source = ../../rulesets/workflow.json;

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
