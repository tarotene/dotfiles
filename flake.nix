{
  description = "tarotene's standalone home-manager configuration (flake-based)";

  inputs = {
    # Pinned to the latest stable release channel (ADR-0001).
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

    home-manager = {
      # home-manager on the matching release branch.
      url = "github:nix-community/home-manager/release-26.05";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Escape hatch for packages not yet in the pinned stable channel:
    #   - herdr: absent from nixos-26.05 entirely.
    # (gh's version-cap escape hatch was dropped once stable shipped >= 2.99.0,
    # see #91.) Drop herdr's overlay entry below once stable catches up too;
    # drop this whole input once it has.
    nixpkgs-unstable.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    # GL/EGL for nix-built GUI apps on a non-NixOS host (ADR-0006 / #13).
    # `follows` is load-bearing, not tidiness: nixGL ships the mesa and libglvnd
    # that get dlopen'd into our applications, so a second nixpkgs would mean a
    # second glibc and a GLIBC_2.x symbol error at runtime.
    nixgl = {
      url = "github:nix-community/nixGL";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # public-publish-guard's upstream (ADR-0009): a plain source tree, not a
    # flake (`flake = false`) — we only take `home.file.source` from it, never
    # evaluate it as a flake. Pinned to a commit SHA rather than a branch name
    # so `flake.lock` fully determines the content. This pin is the last
    # known-good rev, not the one `hms` applies: `hms` (remote apply) pre-builds
    # with `--override-input bleep` at bleep's main and applies that only if the
    # deployed-hook canary (`home.checks`, home/modules/claude.nix) passes,
    # otherwise it falls back to this pin (crates/hms). `hms .` and CI use
    # this pin as-is. Bump it by hand — together with the call sites it
    # changes — when bleep makes a breaking change the canary rejects.
    # No `follows` needed: it is
    # only ever `exec`'d as standalone bash, never `dlopen`'d into another
    # package's process (same reasoning as herdr's overlay entry above).
    # Renamed from tarotene/publish-guard (github#28). This rev additionally
    # carries the orgs.txt fail-loud fix (tarotene/bleep#31): scan* now
    # returns ask instead of silently passing when the org denylist is
    # unconfigured (ADR-0034 Amendment). Bumped past the judgement ledger
    # (tarotene/bleep#32) and the `cd ~/...`/`git -C ~/...` HOME expansion
    # fix (tarotene/bleep#39, tarotene/bleep#40) — cd/git -C with a tilde no
    # longer produces a nonexistent push_dir that escalated push-diff checks
    # to ask regardless of the actual diff content. Bumped again (#604,
    # 2026-10-01) to pick up the `gh -R` position-independent destination
    # detection (tarotene/bleep#23, #43 via #45) — the previous pin only read
    # `gh -R X issue create`, so `gh issue create -R X` resolved the target to
    # the cwd repo and let a private-cwd post to a public repo through
    # unchecked — plus the unresolved-shell-variable ask (#34, reason_id
    # `unresolved-var`, recorded by verdict-escalate since #573), the
    # word-boundary fix (#35), and `bleep doctor` (#56). Bumped again (#675,
    # 2026-10-02) to pick up bleep ADR-0003 (tarotene/bleep#74, #75): push is
    # judged from git's pre-push stdin (`scan-push --pre-push`, wired by
    # config/git/hooks/pre-push) instead of guessing a range from the command
    # string, PreToolUse only denies the forms that disable pre-push
    # (`push-hook-bypass`), and gh posts must be in the canonical form
    # (literal `-R OWNER/REPO` + `--body-file <absolute path>`, otherwise
    # `gh-noncanonical`). The pre-push wiring, the verdict schema and the skill
    # examples move in the same PR so the pin and its call sites switch together.
    # Bumped again (#689, #712, 2026-10-05) to pick up ADR-0004 / LEX_PROTOCOL 5
    # (tarotene/bleep#79: a gh post that reads a body file must stand alone as one
    # command, else `gh-noncanonical` / detail `not-alone`), ADR-0005
    # (tarotene/bleep#82: code vs data position by shell syntax structure) and
    # tarotene/bleep#86 (the body-file path is not scanned — an agent scratchpad
    # path that embeds a private project name no longer blocks a public post — and
    # the deny reason names where the match was). That bump pinned the #86 head
    # commit. Re-pinned (2026-10-15) to bleep main d23af90, the #86 squash commit,
    # which also carries tarotene/bleep#93: a substitution inside double quotes
    # (`"$(… '"\(.x)"' …)"`) is no longer cut at an inner quote or operator, so a
    # read-only `gh … --jq` with a double-quoted jq string is not rejected as an
    # unclosed quote. Re-pinned (2026-10-21, #801) to bleep main 2d265fd, which carries
    # tarotene/bleep#105 (a read-only `gh api graphql` query is not a post, so the
    # quoting of the same query no longer changes the verdict, #801) and #104 (a repo-ref
    # deny names the term hash and a runnable `bleep explain` line, so a false positive
    # such as a public library name in an MCP query can be traced to the denylist entry),
    # plus #103 (`--help` and `2>&1` are not post false positives), #107 (allow-tools.txt)
    # and #110 (a variable command name or text piped into a shell is opaque-exec, deny).
    # LEX_PROTOCOL stays 5, so the deployed hooks and the verdict schema are unchanged.
    bleep = {
      url = "github:tarotene/bleep/2d265fd783b258b591cd9347a8188f7ef2ff99de";
      flake = false;
    };

    # frontend-slides: a third-party skill that builds visually distinctive
    # single-file HTML decks (https://github.com/zarazhangrui/frontend-slides,
    # MIT). Plain source tree like bleep, pinned to a rev. It is deliberately
    # NOT placed under ~/.claude/skills/: its description ("Use when the user
    # wants to build a presentation … slides for a talk") would fire on every
    # slide request and compete with the presentation-authoring skill, which writes the
    # speaker script first. It lands in ~/.local/share/frontend-slides/ (outside
    # the skills scan) and presentation-authoring reads its SKILL.md only when the user
    # explicitly asks for a visually elaborate HTML deck. It has no speaker-notes
    # output, so it is a rendering-layer candidate, not the source of truth
    # (docs/agents/presentation-authoring.md). Pinned at main HEAD 9906a34 (2026-06-23).
    frontend-slides = {
      url = "github:zarazhangrui/frontend-slides/9906a34d640d2111f724544cbc50f7f130569ae1";
      flake = false;
    };

    # Rust workspace builder for the hook/CLI migration (ADR-0024, #391).
    # crane over rustPlatform.buildRustPackage because buildDepsOnly caches the
    # whole workspace's dependency graph as one derivation shared by every
    # member, so a one-line change to one hook rebuilds only the workspace
    # crates, not serde & co. (docs/rust-workspace-measurements.md). crane has
    # no flake inputs of its own, so there is nothing to `follows`.
    crane.url = "github:ipetkov/crane";

    # Flatpak declarations for home-manager (ADR-0001 4th escape hatch, #24).
    # home-manager has no built-in flatpak module; this is the de facto
    # standard one (searched: no official home-manager module exists as of
    # 2026-09-29). `follows` is not applicable — this module only shells out
    # to the system's own `flatpak` binary at activation time, it never
    # dlopens anything nixpkgs builds.
    nix-flatpak.url = "github:gmodena/nix-flatpak";
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-unstable,
      home-manager,
      nixgl,
      bleep,
      frontend-slides,
      crane,
      nix-flatpak,
      ...
    }:
    let
      inherit (nixpkgs) lib;

      # aarch64-darwin added for the altair host (2022 M2 MacBook Air, ADR-0018).
      linuxSystem = "x86_64-linux";
      darwinSystem = "aarch64-darwin";
      systems = [
        linuxSystem
        darwinSystem
      ];
      forAllSystems = lib.genAttrs systems;

      # `pkgs.nixgl.nixGLIntel`, available to every Linux module without
      # widening extraSpecialArgs. Linux-only: nixGL wraps a Linux OpenGL/EGL
      # loader and has nothing to wrap on darwin, which uses its own native GL
      # stack (ADR-0006). Applied only to Linux pkgs below, never to darwin's.
      #
      # default.nix is imported directly rather than taking
      # nixgl.packages.<system>.nixGLIntel, because nixGL's own flake output
      # hardcodes enable32bits = true on x86_64-linux and there is no way to
      # override it from the outside. Measured: dropping the 32-bit mesa
      # takes the wrapper closure from 2.1 GiB to 1.1 GiB with no behavioural
      # change for any GL consumer we install — all four (alacritty, Chrome,
      # Slack, Zoom) are x86_64. Set it back to true if a 32-bit GL consumer
      # (Steam, wine) ever enters home.packages.
      #
      # enableIntelX86Extensions stays true: it is what puts
      # LIBVA_DRIVERS_PATH at intel-media-driver, i.e. the difference between
      # Chrome having a GPU and Chrome having a GPU that can decode video.
      nixglOverlay = final: _prev: {
        nixgl = import "${nixgl}/default.nix" {
          pkgs = final;
          enable32bits = false;
          enableIntelX86Extensions = true;
        };
      };

      # herdr from unstable (not yet in nixos-26.05). Unlike nixGL this does
      # not need `follows`: herdr is a TUI that never dlopens GL, so a second
      # glibc in its closure is harmless. Remove once stable has herdr.
      #
      # legacyPackages.${system} reuses the input's own already-instantiated
      # nixpkgs rather than `import nixpkgs-unstable { inherit system; }`,
      # which would re-instantiate a second whole nixpkgs for one package
      # and silently drop this flake's `config.allowUnfree = true`.
      #
      # `overrideAttrs` layers patches/herdr-worktree-names.patch on top:
      # a personal-taste patch swapping herdr's hardcoded generated-worktree
      # word list (adjective-noun, e.g. "brave-river") for hololive talent
      # nicknames (e.g. "okayu"). Whatever herdr's own build system does with
      # a patched source tree, this forces a from-source build instead of a
      # binary-cache fetch (a few minutes on `nix build`/CI per system).
      # nixpkgs' herdr derivation declares `meta.platforms = lib.platforms.unix`
      # (darwin gets extra cctools/xcbuild inputs upstream), so this overlay is
      # applied per-system below rather than hardcoded to one.
      # The upstream request to make the word list configurable,
      # https://github.com/herdrdev/herdr/issues/4374 (filed 2026-09-19), was
      # closed as not planned on 2026-09-19 — herdr's `[worktrees]` config
      # only exposes `directory`, so there is no config-driven exit either.
      # This override is a **permanent local patch** (decided in
      # tarotene/dotfiles#449, 2026-09-25): the word list backs the herdr
      # sidebar's `$oshi` fan-mark feature, so dropping it would drop that
      # feature, not just revert cosmetic naming.
      #
      # mise rides the same overlay (ADR-0001 Amendment 2026-10, #823): the
      # pinned stable channel ships 2026.5.12, which asks for `mise trust` in
      # every new worktree. Two upstream fixes cure that — 2026.6.6
      # (jdx/mise#10360: a `[tools]`-only mise.toml loads without trust) and
      # 2026.7.5 (jdx/mise#10890: a linked worktree inherits the trust of the
      # same path in the main checkout). Like herdr it is only ever `exec`'d,
      # never `dlopen`'d, so no `follows`; no override, so it is a
      # binary-cache fetch. Drop the entry once stable ships mise >= 2026.7.5
      # (#823).
      #
      # spotify-player rides it too (#861): the pinned stable channel ships
      # 0.23.0, which cannot parse the Web API responses of a newly created
      # developer app (`missing field followers` / `available_markets`, plus
      # endless "Unknown playback item" warnings). The fix is the rspotify 0.16
      # migration (aome510/spotify-player#973), first released in v0.25.0; the
      # locked unstable rev ships 0.25.1. Like herdr it is only ever `exec`'d,
      # so no `follows`. No override here — the Linux audio backend is chosen
      # in home/modules/spotify-player.nix, because that override is the one
      # that forces a from-source build. Drop the entry once stable ships
      # spotify-player >= 0.25.0 (#861).
      herdrOverlay =
        system:
        (_final: _prev: {
          herdr = nixpkgs-unstable.legacyPackages.${system}.herdr.overrideAttrs (old: {
            patches = (old.patches or [ ]) ++ [ ./patches/herdr-worktree-names.patch ];
          });
          mise = nixpkgs-unstable.legacyPackages.${system}.mise;
          spotify-player = nixpkgs-unstable.legacyPackages.${system}.spotify-player;
        });

      # The Rust hook/CLI workspace (Cargo.toml, crates/*; ADR-0024 / #391),
      # exposed as `pkgs.dotfiles-tools` so home modules can point a hook
      # command at "${pkgs.dotfiles-tools}/bin/<name>" — a store path, not a
      # home.file copy. Built with crane; `rustWorkspace` carries the pieces
      # the flake's checks and devShell reuse.
      rustWorkspace =
        pkgs:
        let
          craneLib = crane.mkLib pkgs;
          # cleanCargoSource keeps *.rs / Cargo.* / *.toml only, so editing a
          # bash hook or a doc never invalidates the Rust build.
          src = craneLib.cleanCargoSource ./.;
          commonArgs = {
            inherit src;
            pname = "dotfiles-tools";
            version = "0.1.0";
            strictDeps = true;
            # Tests run in CI through `nix develop` (nix.yml), not in the
            # build sandbox: the fixture oracles execute the bash originals,
            # which need /usr/bin/env and the rest of the repo tree.
            doCheck = false;
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        in
        {
          inherit craneLib;
          # buildDepsOnly の出力。package・clippy の build 時だけの入力で、どの
          # activationPackage からも参照されない。CI がキャッシュに残すために、
          # checks に名前を付けて出す(下の dotfiles-tools-deps)。
          deps = cargoArtifacts;
          package = craneLib.buildPackage (commonArgs // { inherit cargoArtifacts; });
          clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--workspace --all-targets -- --deny warnings";
            }
          );
          fmt = craneLib.cargoFmt {
            inherit src;
            inherit (commonArgs) pname version;
          };
        };

      rustOverlay = final: _prev: {
        dotfiles-tools = (rustWorkspace final).package;
      };

      # bleep-hook (#561): the `bleep` input's bash body (`hooks/bleep`) is
      # pin-fixed above, but the Rust adapter/lexer it shells out to
      # (`bleep-hook`, resolved via `~/.cargo/bin` by `hooks/bleep.sh`'s
      # `find_hook_bin`) had no nix-managed counterpart — it depended on a
      # manual `cargo install --path <bleep checkout>` that silently drifted
      # from the pinned bash version (real incident, 2026-09-29: the two
      # disagreed on `hash`'s output format after a pin bump). Building it
      # from the same `bleep` input here ties both to one pin: bumping
      # `inputs.bleep` moves the bash body and this binary together.
      # `crateNameFromCargoToml` reads pname/version straight from bleep's
      # own Cargo.toml rather than hardcoding them, so a future rename/bump
      # never needs a matching edit here (same crane placeholder-avoidance
      # as `rustWorkspace` above).
      bleepHookOverlay =
        final: _prev:
        let
          craneLib = crane.mkLib final;
        in
        {
          bleep-hook = craneLib.buildPackage (
            (craneLib.crateNameFromCargoToml { cargoToml = "${bleep}/Cargo.toml"; })
            // {
              src = bleep;
              strictDeps = true;
              doCheck = false;
            }
          );
        };

      mkPkgs =
        system:
        import nixpkgs {
          inherit system;
          config.allowUnfree = true;
          overlays = [
            (herdrOverlay system)
            rustOverlay
            bleepHookOverlay
          ]
          ++ lib.optionals (lib.hasSuffix "-linux" system) [ nixglOverlay ];
        };

      pkgsFor = forAllSystems mkPkgs;

      # Build a standalone home-manager configuration from a single host module.
      # A host module imports home/common.nix plus exactly one identity module
      # (Identity / Instance two-layer layout — see ADR-0001).
      #
      # extraModules (ADR-0034): additional modules layered on top of
      # hostModule, e.g. a private wrapper flake's own value modules. Kept as
      # a third positional argument rather than folded into hostModule so the
      # four in-repo call sites below stay untouched in shape (`[ ]`) — only
      # an outside caller through the exported `lib.mkHome` ever supplies a
      # non-empty list. Order matters: extraModules come after hostModule, so
      # a private module can override a `lib.mkDefault` the public host
      # module set, but never the reverse.
      mkHome =
        system: hostModule: extraModules:
        home-manager.lib.homeManagerConfiguration {
          pkgs = pkgsFor.${system};
          modules = [
            hostModule
            nix-flatpak.homeManagerModules.nix-flatpak
            # The dotfiles commit this generation was built from, as a one-line
            # file the Rust CLIs read for `--version` (#807). It is NOT compiled
            # into the binaries: a compile-time env var would change the crane
            # derivation on every commit and rebuild the whole workspace, which
            # `cleanCargoSource` (Rust and Cargo files only) otherwise avoids for
            # doc-only commits. A path input (`hms .`) has no rev, so it says so.
            {
              home.file.".local/share/dotfiles/rev".text =
                (self.rev or self.dirtyRev or "unknown (path input, no commit)") + "\n";
            }
          ]
          ++ extraModules;
          # bleep is a plain source tree (flake = false), threaded
          # through as an extra module argument rather than an overlay —
          # claude.nix only needs its store path for home.file.source, not a
          # package derivation (ADR-0009). frontend-slides is threaded the same
          # way (home.file.source only, docs/agents/presentation-authoring.md).
          extraSpecialArgs = {
            inherit bleep frontend-slides;
          };
        };
    in
    {
      # Keyed by logical hostname (ADR-0019 star-codename, resolved via a
      # marker file — see crates/hms / bootstrap.sh `resolve_host`, not
      # the OS `hostname`). All three Linux hosts moved off the old
      # `<identity>-pop[-<generation>]` convention (#207 / stage1-prep) to
      # star codenames in #214: personal-pop → vega, company-pop-new →
      # arcturus. company-pop-old was retired outright (no replacement
      # module) rather than renamed.
      homeConfigurations = {
        "vega" = mkHome linuxSystem ./home/hosts/vega.nix [ ];
        "arcturus" = mkHome linuxSystem ./home/hosts/arcturus.nix [ ];

        # First darwin host (2022 M2 MacBook Air) — ADR-0018/ADR-0019.
        "altair" = mkHome darwinSystem ./home/hosts/altair.nix [ ];

        # Transitional aliases (#214): the old hostname-keyed entries, kept
        # pointed at the SAME new host modules, so `hms`'s hostname fallback
        # (resolve_host() when no ~/.config/dotfiles/host marker is placed
        # yet) still resolves on a host whose OS hostname has not yet been
        # renamed via the cutover runbook (docs/cutover-runbook.md,
        # "Renaming an existing host to a star codename", ADR-0019
        # Amendment 2). Drop once every physical host's OS hostname has been
        # renamed (tracked separately, not this PR — see #214's own
        # follow-up note).
        "personal-pop" = mkHome linuxSystem ./home/hosts/vega.nix [ ];
        "company-pop-new" = mkHome linuxSystem ./home/hosts/arcturus.nix [ ];
      };

      # Exported so an outside private wrapper flake (ADR-0034) can build its
      # own homeConfigurations from this flake's host modules plus its own
      # private value modules, without this repo ever taking that flake as an
      # input — flake.lock records an input's `{owner, repo}` in the clear,
      # and that flake's name cannot appear in this PUBLIC repo's source
      # (docs/agents/writing-style.md's same constraint). Dependency direction
      # is inverted instead: the private flake takes this one as an input.
      lib = {
        inherit mkHome;
      };

      # `nix flake check` evaluates every host's activation package, filtered
      # to the check's own system — a darwin host's activationPackage cannot
      # be built (only evaluated) from a Linux `checks.x86_64-linux`, and vice
      # versa, so each system only claims the homeConfigurations whose
      # activationPackage actually targets it.
      #
      # The Rust workspace adds its package, clippy and rustfmt checks on top
      # (ADR-0024 / #391); nix.yml's rust job builds these.
      checks = forAllSystems (
        system:
        let
          rust = rustWorkspace pkgsFor.${system};
        in
        lib.mapAttrs (_name: cfg: cfg.activationPackage) (
          lib.filterAttrs (_name: cfg: cfg.activationPackage.system == system) self.homeConfigurations
        )
        // {
          agent-skills = import ./home/tests/agent-skills.nix {
            pkgs = pkgsFor.${system};
            inherit lib;
            files =
              self.homeConfigurations.${if system == linuxSystem then "vega" else "altair"}.config.home.file;
          };
          dotfiles-tools = rust.package;
          # 依存だけの build(crane の buildDepsOnly)。CI の build-host が GC root に
          # して、キャッシュに残す(.github/workflows/ci.yml の「Root the Rust
          # dependency artifacts」)。ここに名前が無いと、参照する手段が無い。
          dotfiles-tools-deps = rust.deps;
          dotfiles-tools-clippy = rust.clippy;
          dotfiles-tools-fmt = rust.fmt;
        }
        // lib.optionalAttrs (system == linuxSystem) {
          # private wrapper(`lib.mkHome` の `extraModules`)のように、このリポジトリの外が
          # pipeline-map のノードを持たない timer を足しても、評価が通ること(#992、
          # home/modules/pipeline.nix)。最初の版は最終の config の名前を全部数えたので、
          # wrapper の timer を未対応として `hms .` の評価を落とした。評価するだけで、
          # generation は build しない(`drvPath` を強制した後の空の derivation)。
          pipeline-external-timer =
            let
              cfg = mkHome linuxSystem ./home/hosts/vega.nix [
                {
                  systemd.user.timers.external-example = {
                    Unit.Description = "outside-the-repo timer (pipeline-external-timer check)";
                    Timer.OnCalendar = "daily";
                  };
                }
              ];
            in
            builtins.seq cfg.activationPackage.drvPath (
              pkgsFor.${system}.runCommand "pipeline-external-timer" { } "touch $out"
            );
        }
      );

      # Addressable alias for the patched herdr build (overlay above). CI's
      # build-host job builds this explicitly to root it for GC before
      # building the (much larger, unpatched-input) activationPackages, so
      # cache-nix-action's post-run GC can keep the saved cache scoped to
      # herdr's own closure instead of the whole /nix store.
      packages = forAllSystems (system: {
        herdr = pkgsFor.${system}.herdr;
        dotfiles-tools = pkgsFor.${system}.dotfiles-tools;
      });

      # `nix develop`: the toolchain for crates/ (ADR-0024). nix.yml's rust
      # job runs cargo test through this shell, so jq/git/bash are here for
      # the fixture oracles that execute the bash originals.
      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor.${system};
        in
        {
          default = (rustWorkspace pkgs).craneLib.devShell {
            packages = [
              pkgs.rust-analyzer
              pkgs.jq
              pkgs.git
              pkgs.bashInteractive
              pkgs.hyperfine
              # crates/gpg-subkey のテストは実 GnuPG を一時 GNUPGHOME で走らせる
              # (addkey/revkey の command-file の仕組みそのものが検証対象、#414)。
              pkgs.gnupg
              # crates/github-app-snapshot のテストは実 openssl で RS256 JWT を
              # 署名・検証する(bash 版 selftest と同じ前提、#414)。
              pkgs.openssl
              # crates/pipeline-map の `render` が、manifest から作った .d2 を SVG にする
              # (ELK 同梱の単一バイナリ)。CI の再生成差分検査も同じ d2 を使うので、
              # flake.lock の pin でローカルと CI の出力が一致する(ADR-872)。
              pkgs.d2
            ];
          };
        }
      );

      # nixfmt-tree, not nixfmt itself (#30). `nix fmt` with no arguments hands
      # the formatter the whole tree, and bare nixfmt reads that as stdin and
      # dies on the first non-Nix file; nixfmt upstream now points at this
      # wrapper by name. It is a treefmt wrapper that walks the tree and feeds
      # nixfmt only the *.nix files, so `nix fmt` works unqualified — which is
      # what CLAUDE.md and the docs tell you to run.
      formatter = forAllSystems (system: pkgsFor.${system}.nixfmt-tree);
    };
}
