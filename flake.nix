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
    # so `flake.lock` fully determines the content; bump this rev by hand when
    # tarotene/bleep cuts a new release. No `follows` needed: it is
    # only ever `exec`'d as standalone bash, never `dlopen`'d into another
    # package's process (same reasoning as herdr's overlay entry above).
    # Renamed from tarotene/publish-guard (github#28); this rev is the
    # rename itself, which is also where the Rust hook/CLI migration
    # (#25-27, `bleep`/`bleep-hook`/`hooks/bleep.sh`) lands.
    bleep = {
      url = "github:tarotene/bleep/eb5c40ac723e772fe60c9b7a619640aac7719d81";
      flake = false;
    };

    # Rust workspace builder for the hook/CLI migration (ADR-0024, #391).
    # crane over rustPlatform.buildRustPackage because buildDepsOnly caches the
    # whole workspace's dependency graph as one derivation shared by every
    # member, so a one-line change to one hook rebuilds only the workspace
    # crates, not serde & co. (docs/rust-workspace-measurements.md). crane has
    # no flake inputs of its own, so there is nothing to `follows`.
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-unstable,
      home-manager,
      nixgl,
      bleep,
      crane,
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
      # Drop this override once herdr's word list is configurable upstream:
      # https://github.com/herdrdev/herdr/issues/4374 (filed 2026-09-19).
      herdrOverlay =
        system:
        (_final: _prev: {
          herdr = nixpkgs-unstable.legacyPackages.${system}.herdr.overrideAttrs (old: {
            patches = (old.patches or [ ]) ++ [ ./patches/herdr-worktree-names.patch ];
          });
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
          package = craneLib.buildPackage (commonArgs // { inherit cargoArtifacts; });
          clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--workspace --all-targets -- --deny warnings";
            }
          );
          fmt = craneLib.cargoFmt { inherit src; };
        };

      rustOverlay = final: _prev: {
        dotfiles-tools = (rustWorkspace final).package;
      };

      mkPkgs =
        system:
        import nixpkgs {
          inherit system;
          config.allowUnfree = true;
          overlays = [
            (herdrOverlay system)
            rustOverlay
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
          modules = [ hostModule ] ++ extraModules;
          # bleep is a plain source tree (flake = false), threaded
          # through as an extra module argument rather than an overlay —
          # claude.nix only needs its store path for home.file.source, not a
          # package derivation (ADR-0009).
          extraSpecialArgs = {
            inherit bleep;
          };
        };
    in
    {
      # Keyed by logical hostname (ADR-0019 star-codename, resolved via a
      # marker file — see scripts/hms.sh / bootstrap.sh `resolve_host`, not
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
      # (docs/claude/writing-style.md's same constraint). Dependency direction
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
          dotfiles-tools = rust.package;
          dotfiles-tools-clippy = rust.clippy;
          dotfiles-tools-fmt = rust.fmt;
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
