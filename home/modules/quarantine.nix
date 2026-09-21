# Shared quarantine for files home-manager does not own (#64).
#
# Two distinct mechanisms live here, both about a file on disk that
# home-manager did not put there:
#
#   `managedFiles` — a file we are *adopting*. Cleared out of the way before
#     `checkLinkTargets`, suffix `.pre-nix`.
#   `strayFiles`   — a file we are *retiring*. Renamed after `writeBoundary`,
#     suffix `.bak`.
#
# Neither deletes anything: the files are outside home-manager's ownership.
#
# --- managedFiles ---
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
#
# --- strayFiles ---
#
# The mirror-image case: a hand-placed file that no longer has a reason to
# exist, because home-manager now declares the same thing. Left alone it does
# not just sit there harmlessly — it *competes*, and it names a different
# binary than the declared one. Two instances so far, both of that exact
# shape: the pre-migration fcitx5 autostart entry (`/usr/bin/fcitx5`, apt's
# 5.1.7, racing the declared 5.1.19) and the retired shell installer's
# alacritty launcher entry (`~/.cargo/bin/alacritty`, cargo's 0.15.1, racing
# the declared 0.17.0-nixgl — see ADR-0029).
#
# Renamed rather than deleted, for the same ownership reason as above, and
# because the consumers of both only scan for a specific suffix (`*.desktop`
# for the XDG autostart generator and the application launcher), so a `.bak`
# suffix is enough to retire the file.
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

  options.dotfiles.quarantine.strayFiles = lib.mkOption {
    type = lib.types.listOf lib.types.str;
    default = [ ];
    description = ''
      Paths relative to $HOME holding an unmanaged, hand-placed file that
      home-manager now supersedes. After writeBoundary, the file is moved to
      `<path>.bak`, retiring it without deleting anything home-manager does
      not own.
    '';
    example = [ ".local/share/applications/example.desktop" ];
  };

  config = lib.mkMerge [
    (lib.mkIf (cfg.managedFiles != [ ]) {
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
    })

    (lib.mkIf (cfg.strayFiles != [ ]) {
      home.activation.quarantineStrayFiles = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
        ${lib.concatMapStringsSep "\n" (path: ''
          stray="$HOME/${path}"
          if [ -e "$stray" ] || [ -L "$stray" ]; then
            run mv -f "$stray" "$stray.bak"
          fi
        '') cfg.strayFiles}
      '';
    })
  ];
}
