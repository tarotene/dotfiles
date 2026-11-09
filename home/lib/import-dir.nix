# A directory of modules, as the list of paths to import (`importDir ./modules`).
#
# The directory's contents ARE the import list: a module dropped into it is
# evaluated, with no second list to forget to update (the old hand-written list
# in common.nix evaluated a new module only once it was added there, and no
# error or warning said so, #1071). The listing is closed on purpose — anything
# visible that is not a regular `*.nix` file (a subdirectory, a README, a
# symlink) fails the evaluation instead of being skipped, so a file placed here
# can neither be silently ignored nor silently pulled in.
#
# Names starting with `.` are not part of the directory's contents: they are
# never modules, and an untracked one can reach a `path:` flake input (a
# `.DS_Store` on macOS, an editor's swap file, a tool's stand-in directory).
#
# A file that is not a module (a plain function such as nixgl.nix) does not
# belong in a directory that is imported this way: it goes in `home/lib`.
{ lib }:
dir:
let
  entries = lib.filterAttrs (name: _: !(lib.hasPrefix "." name)) (builtins.readDir dir);
  isNixFile = name: type: type == "regular" && lib.hasSuffix ".nix" name;
  stray = lib.attrNames (lib.filterAttrs (name: type: !(isNixFile name type)) entries);
in
assert lib.assertMsg (stray == [ ])
  "importDir ${toString dir}: only regular *.nix files may live here, found: ${lib.concatStringsSep ", " stray}";
map (name: dir + "/${name}") (lib.attrNames entries)
