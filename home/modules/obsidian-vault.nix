# Vault location shared by worklog, wrap-up notes and permission declarations.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.obsidian;
in
{
  options.dotfiles.obsidian = {
    vaultPath = lib.mkOption {
      type = lib.types.nullOr (lib.types.strMatching "/.*");
      default = null;
      description = "Absolute vault path, or null when no vault is declared.";
    };
    initializeVault = lib.mkEnableOption "initializing a new or empty vault";
  };

  config = lib.mkIf (cfg.vaultPath != null) {
    home.file.".local/bin/worklog".source = "${pkgs.dotfiles-tools}/bin/worklog";
    xdg.configFile."dotfiles/obsidian-vault".text = "${cfg.vaultPath}\n";
    dotfiles.claudePermissions.allowWrite = [ cfg.vaultPath ];
    # Do not publish a marker for a missing or invalid vault. Preparation is
    # after the write boundary but before home-manager links the generation.
    home.activation.prepareObsidianVault =
      lib.hm.dag.entryBetween [ "linkGeneration" ] [ "writeBoundary" ]
        ''
          run ${pkgs.dotfiles-tools}/bin/issue-inbox --prepare-vault ${lib.escapeShellArg cfg.vaultPath} ${lib.optionalString cfg.initializeVault "--initialize"}
        '';
  };
}
