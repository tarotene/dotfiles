# Shared `.backup` collision quarantine (#64).
#
# `home-manager switch -b backup` refuses to activate when a file newly taken
# under management already exists as a *real* file on disk **and** a stale
# `<path>.backup` from some earlier switch is already sitting next to it — the
# retreat path itself is occupied, so the whole activation dies at
# checkLinkTargets before writeBoundary ever runs (`home/modules/herdr.nix`
# hit this for `~/.config/herdr/config.toml`; `~/.config/mimeapps.list.backup`
# shows the same shape of residue). This is not specific to any one file: it
# recurs for every new `xdg.configFile` / `home.file` target that happens to
# already exist on disk with switch history behind it.
#
# Any module adopting such a file lists it here instead of writing its own
# `entryBefore [ "checkLinkTargets" ]` copy of this logic.
{
  lib,
  config,
  ...
}:
let
  cfg = config.dotfiles.quarantine;
in
{
  options.dotfiles.quarantine.managedFiles = lib.mkOption {
    type = lib.types.listOf lib.types.str;
    default = [ ];
    description = ''
      Paths relative to $HOME being newly taken under home-manager
      management. Before checkLinkTargets runs, a pre-existing real file
      (not a symlink) at that path is moved to `<path>.pre-nix`, and a stale
      `<path>.backup` left by a previous switch is moved to
      `<path>.backup.pre-nix` — clearing both potential `-b backup` clobber
      targets before the check can trip on them.
    '';
    example = [ ".config/example/config.toml" ];
  };

  config = lib.mkIf (cfg.managedFiles != [ ]) {
    home.activation.quarantineBackupCollisions = lib.hm.dag.entryBefore [ "checkLinkTargets" ] ''
      ${lib.concatMapStringsSep "\n" (path: ''
        target="$HOME/${path}"
        if [ -e "$target" ] && [ ! -L "$target" ]; then
          run mv -f "$target" "$target.pre-nix"
        fi
        if [ -e "$target.backup" ]; then
          run mv -f "$target.backup" "$target.backup.pre-nix"
        fi
      '') cfg.managedFiles}
    '';
  };
}
