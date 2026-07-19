# Nixification roadmap

Per ADR-0002 the migration uses a **hybrid** translation: keep working config
files literal, use Nix DSL only where interpolation pays. This file tracks
literal configs that are good candidates to move to native Nix DSL later, so the
cutover is not blocked on rewriting everything up front.

| Area | Current (literal) | Candidate native form | Notes |
|---|---|---|---|
| Shell plugins | `config/sheldon/plugins.toml` | native home-manager plugin management (e.g. `programs.zsh.plugins` / zsh-abbr) | Keep sheldon literal for now (#209). |
| Zsh modules | 15× `config/zsh/modules/*.zsh` via `xdg.configFile` | selective rewrite into `programs.zsh` options | Translate only modules where Nix interpolation pays. |
| Prompt | `config/starship.toml` | `programs.starship.settings` | Literal first (#209). |
| Terminal | `config/alacritty/alacritty.toml` | `programs.alacritty.settings` | Literal first (#214). |
| Secrets loader | `35-secrets-sops.zsh` + `sops-secrets-env.sh` | stays a runtime escape hatch | Do NOT move to sops-nix (ADR-0003). |

Add rows as new literal configs land; remove them once nixified.
