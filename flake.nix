{
  description = "tarotene's standalone home-manager configuration (flake-based)";

  inputs = {
    # Pinned to the latest stable release channel (ADR-0001).
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";

    home-manager = {
      # home-manager on the matching release branch.
      url = "github:nix-community/home-manager/release-25.11";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      home-manager,
      ...
    }:
    let
      system = "x86_64-linux";

      pkgs = import nixpkgs {
        inherit system;
        config.allowUnfree = true;
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
