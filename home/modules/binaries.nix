# The one table of executables deployed from `pkgs.dotfiles-tools` — the crane-built
# workspace of Rust CLIs and hooks (ADR-0024) — to a directory under $HOME.
#
# A module that needs an executable on disk adds its name under the directory it goes to:
#
#   dotfiles.binaries.".local/bin" = [ "git-prune-worktrees" ];
#   dotfiles.binaries.".claude/hooks" = [ "attribution-guard" ];
#
# and this module turns every row into `home.file."<dir>/<name>".source =
# "${pkgs.dotfiles-tools}/bin/<name>"`. The row is the module's own, so a binary that only
# one identity or one host needs (home/identities/personal/, home/hosts/vega/) stays with
# the module that needs it and is deployed only there; what is shared is the one place
# that spells the store path and the target, and the check that no name is listed twice.
#
# The deployed file name is the executable's name. The deployments whose names differ stay
# hand-written `home.file` entries, each with its reason beside it: `codex` (the launcher,
# home/modules/codex.nix), `apply-rulesets.sh` (the old name the governance templates call,
# home/modules/packages.nix), and `open` / `xdg-open` (both are `detach-open`, Linux only,
# home/modules/packages.nix).
#
# What the table does not cover on purpose: an activation script that runs
# "${pkgs.dotfiles-tools}/bin/<name>" straight from the store, a systemd unit's ExecStart,
# and `home.file` entries with another shape (`executable = true`, a source that is not
# the store path). `dotfiles-tools-references` (home/modules/agents.nix) still reads all of
# those, so a name that the workspace no longer builds fails the build either way.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.binaries;
  rows = lib.concatLists (
    lib.mapAttrsToList (dir: names: map (name: { inherit dir name; }) names) cfg
  );
  targetOf = row: "${row.dir}/${row.name}";
  targets = map targetOf rows;
  duplicates = lib.unique (lib.filter (t: lib.count (u: u == t) targets > 1) targets);
in
{
  options.dotfiles.binaries = lib.mkOption {
    type = lib.types.attrsOf (lib.types.listOf lib.types.str);
    default = { };
    example = {
      ".local/bin" = [ "git-prune-worktrees" ];
    };
    description = ''
      Executables of `pkgs.dotfiles-tools` to deploy, by the directory under $HOME they
      go to: `dotfiles.binaries.<dir> = [ "<name>" … ]` deploys `<dir>/<name>` as a
      symlink to `''${pkgs.dotfiles-tools}/bin/<name>`. A name listed twice for the same
      directory fails the evaluation.
    '';
  };

  config = {
    assertions = [
      {
        assertion = duplicates == [ ];
        message = "dotfiles.binaries: deployed more than once: ${lib.concatStringsSep ", " duplicates}";
      }
    ];

    home.file = lib.listToAttrs (
      map (row: {
        name = targetOf row;
        value.source = "${pkgs.dotfiles-tools}/bin/${row.name}";
      }) rows
    );
  };
}
