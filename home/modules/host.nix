# The host's own name — the one fact a host module gives every other module.
#
# A host module (home/hosts/<host>.nix) sets `dotfiles.host.name` to its own
# file name; a module that needs the host's name reads
# `config.dotfiles.host.name` instead of writing it (a module that writes
# "vega" has silently become vega-only code in a directory every host
# evaluates — `repo-structure-check` rejects a host name in a module's strings,
# docs/agents/repo-structure-check.md).
#
# The same option writes ~/.config/dotfiles/host, the marker `hms` and
# bootstrap.sh's resolve_host() read to pick the host module (ADR-0019 D3,
# Amendment 2). From the first switch on, this declaration is the marker's
# source of truth. This module is the only writer: the file's `text` is a
# `lines` option, so a second writer would not conflict, it would be joined
# with a newline (two writers of "<host>\n" give "<host>\n\n<host>\n").
#
# No default: a host module that forgets the name fails the evaluation with
# "option `dotfiles.host.name` is used but not defined", instead of deploying a
# marker that names another machine.
{ config, lib, ... }:
{
  options.dotfiles.host.name = lib.mkOption {
    type = lib.types.strMatching "[a-z][a-z0-9-]*";
    example = "polaris";
    description = ''
      This host's name: the file name of its module under home/hosts/ (a star
      codename, ADR-0019). Also the content of ~/.config/dotfiles/host.
    '';
  };

  config.xdg.configFile."dotfiles/host".text = "${config.dotfiles.host.name}\n";
}
