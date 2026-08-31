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
| fcitx5 autostart | `config/autostart/fcitx5.desktop` + `pkgs.replaceVars` for `Exec=` | `systemd.user.services.fcitx5` | Blocked on COSMIC gaining native XDG autostart (pop-os/cosmic-session#67, pop-os/cosmic-epoch#274). Today the `.desktop` is the only mechanism; `systemd-xdg-autostart-generator` converts it. One interpolated value, so the file stays literal (ADR-0002). |
| fcitx5 config / profile | `config/fcitx5/{config,profile}`, seeded if absent | `i18n.inputMethod.fcitx5` — **not applicable** to standalone home-manager (it is an NixOS option) | fcitx5 rewrites both at runtime, so a store symlink cannot work. Seed-if-absent is the end state, not a way-station. |
| Claude Code plan-review hook | `config/claude/{hooks,commands}` via `home.file` (`home/modules/claude.nix`) | stays literal — no `programs.*` module covers Claude Code hooks | Acceptance-convergent Codex review gate on ExitPlanMode: the critic emits schema-validated findings, the hook's shell+jq judge decides, and the gate is "no BLOCKER/MAJOR left open" (max 2 rounds, fail-open; silent no-op without a codex binary per ADR-0005). Rationale and operations: [`codex-plan-review.md`](claude/codex-plan-review.md). `~/.claude/settings.json` is Claude-Code-owned, so registration is an idempotent jq merge at activation, same constraint class as the fcitx5 profile. |

Add rows as new literal configs land; remove them once nixified.
