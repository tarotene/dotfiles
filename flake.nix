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

    # GL/EGL for nix-built GUI apps on a non-NixOS host (ADR-0006 / #13).
    # `follows` is load-bearing, not tidiness: nixGL ships the mesa and libglvnd
    # that get dlopen'd into our applications, so a second nixpkgs would mean a
    # second glibc and a GLIBC_2.x symbol error at runtime.
    nixgl = {
      url = "github:nix-community/nixGL";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      home-manager,
      nixgl,
      ...
    }:
    let
      system = "x86_64-linux";

      pkgs = import nixpkgs {
        inherit system;
        config.allowUnfree = true;

        overlays = [
          # `pkgs.nixgl.nixGLIntel`, available to every module without widening
          # extraSpecialArgs.
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
          (final: _prev: {
            nixgl = import "${nixgl}/default.nix" {
              pkgs = final;
              enable32bits = false;
              enableIntelX86Extensions = true;
            };
          })
        ];
      };

      # Build a standalone home-manager configuration from a single host module.
      # A host module imports home/common.nix plus exactly one identity module
      # (Identity / Instance two-layer layout — see CONTEXT.md / ADR-0001).
      mkHome =
        hostModule:
        home-manager.lib.homeManagerConfiguration {
          inherit pkgs;
          modules = [ hostModule ];
        };
    in
    {
      # Keyed by hostname so `home-manager switch` auto-selects per machine.
      # Hostname convention: <identity>-pop[-<generation>].  See #207 / stage1-prep.
      homeConfigurations = {
        "personal-pop" = mkHome ./home/hosts/personal-pop.nix;
        "company-pop-old" = mkHome ./home/hosts/company-pop-old.nix;
        "company-pop-new" = mkHome ./home/hosts/company-pop-new.nix;
      };

      # `nix flake check` evaluates every host's activation package.
      checks.${system} = builtins.mapAttrs (_name: cfg: cfg.activationPackage) self.homeConfigurations;

      formatter.${system} = pkgs.nixfmt-rfc-style;
    };
}
