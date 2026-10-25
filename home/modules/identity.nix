# Identity declaration — which side this machine is on, and which Claude account
# belongs to that side.
#
# The identity layers (home/identities/{personal,company}.nix) set
# `dotfiles.identity`; this module is the single place that turns it into the
# environment the tools read:
#
#   DOTFILES_IDENTITY        personal | company
#   DOTFILES_CLAUDE_ACCOUNT  exact:<email> | domain:<domain>
#
# `dotfiles-identity verify` (crates/dotfiles-identity, docs/dotfiles-identity.md)
# compares the two with the Claude account actually logged in on the machine and
# succeeds only when they agree. The options have no default on purpose: an
# identity layer that forgets to declare them fails evaluation instead of
# shipping a machine with an undeclared side.
{ config, lib, ... }:
let
  cfg = config.dotfiles.identity;
in
{
  options.dotfiles.identity = {
    name = lib.mkOption {
      type = lib.types.enum [
        "personal"
        "company"
      ];
      description = "The side of this machine. A closed set: nothing else is representable.";
    };

    claudeAccount = {
      match = lib.mkOption {
        type = lib.types.enum [
          "exact"
          "domain"
        ];
        description = ''
          `exact`: the logged-in Claude account's email equals `value`.
          `domain`: its domain (the part after `@`, no subdomains) equals `value`.
        '';
      };
      value = lib.mkOption {
        type = lib.types.str;
        description = "An email address (`exact`) or a domain (`domain`).";
      };
    };
  };

  config = {
    assertions = [
      {
        assertion = (cfg.claudeAccount.match == "exact") == (lib.hasInfix "@" cfg.claudeAccount.value);
        message = "dotfiles.identity.claudeAccount: `exact` takes an email address (with @), `domain` takes a bare domain (without @).";
      }
    ];

    home.sessionVariables = {
      DOTFILES_IDENTITY = cfg.name;
      DOTFILES_CLAUDE_ACCOUNT = "${cfg.claudeAccount.match}:${cfg.claudeAccount.value}";
    };
  };
}
