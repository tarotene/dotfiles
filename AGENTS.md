# dotfiles agent instructions

See [README.md](README.md) for what this repository is and does, and
[CONTRIBUTING.md](CONTRIBUTING.md) for what Issues and pull requests are
accepted.
This file is for agent operating instructions only (ADR-0016) — it does not
duplicate README/CONTRIBUTING content.

## Project Overview

Declarative, flake-based **standalone home-manager** configuration for one
person's user environment across three Pop!_OS hosts (`personal-pop`,
`company-pop-old`, `company-pop-new`) and two identities (personal, company).

**Core purpose**: home-manager is the single source of truth for the user
environment, so a fresh machine is provisioned to an identical setup with
near-zero manual steps. Migrated from the old procedural shell-script installer
(epic #207). See `README.md` and `docs/adr/` for the full charter.

## Three-layer model

| Layer | Owns | Managed by | ADR |
|-------|------|------------|-----|
| **User environment** (source of truth) | shell, git, terminal, user-space CLIs, fonts, prompts, per-user services, GPG agent, GUI apps, **fcitx5 daemon + mozc**, **herdr (binary + sidebar config)** | home-manager (`flake.nix` + `home/`) | ADR-0001 (+ Amendment) |
| **System layer** (escape hatch) | root, a system service, kernel/driver integration, **or code loaded into an apt-installed process**: build toolchain, cross C toolchain, `scdaemon`, fcitx5 *immodules* (`fcitx5-frontend-all`), login-shell fallback | `apt` via `scripts/install-packages.sh` + `packages/declarative/apt-packages.txt` | ADR-0001 (+ Amendment) |
| **Per-project runtimes** (escape hatch) | language toolchains, project-local versions | `mise` / `direnv` / `rustup` launchers (installed by home-manager; toolchains stay project-scoped) | ADR-0002 |

Note on graphics: the driver stack itself is root-owned and stays in the system
layer, but nix GUI apps cannot use it — they load **nix's own mesa** through a
per-package `nixGL` wrapper (`home/modules/nixgl.nix`, ADR-0006), applied to
Linux GUI packages in `home/modules/desktop.nix` and identity-scoped ones
like `warp-terminal` (`home/identities/personal.nix`, #9).

Note on `herdr`: not yet in the pinned stable nixpkgs channel. Comes from a
single-package `nixpkgs-unstable` overlay in `flake.nix` (ADR-0001 Amendment,
#42) — drop the overlay entry once stable catches up. Unlike `nixgl`, this
overlay does not need `inputs.nixpkgs.follows`: herdr is only ever `exec`'d,
never `dlopen`'d into another package's process, so a second glibc in its
closure is harmless. (`gh`'s own version-capped escape hatch on this same
overlay was dropped once stable shipped >= 2.99.0, see #91.)

## Project Structure

```
dotfiles/
├── flake.nix                 # inputs (nixpkgs + home-manager, pinned; nixpkgs-unstable
│                             #   is a herdr + gh escape hatch, ADR-0001 Amendment) +
│                             #   homeConfigurations.<hostname>
├── flake.lock
├── patches/                   # source patches applied via `overrideAttrs` in
│                             #   flake.nix's package overlay (herdr-worktree-
│                             #   names.patch: personal-taste worktree-naming
│                             #   patch, drop once herdrdev/herdr#4374 lands)
├── home/                     # home-manager modules (Identity / Instance two-layer)
│   ├── common.nix            # shared across every host; imports all modules/
│   ├── identities/           # identity-scoped (git identity, browser default)
│   │   ├── personal.nix       # also home.packages for cloud-connected tools that must
│   │   │                     #   not auto-deploy to company hosts (warp-terminal, #9)
│   │   └── company.nix
│   ├── hosts/                # instance-scoped; imports common + one identity + per-host signing key
│   │   ├── personal-pop.nix
│   │   ├── company-pop-old.nix
│   │   └── company-pop-new.nix
│   └── modules/              # shell, atuin, git, gpg, sudo-askpass, packages, desktop, nixgl,
│                             #   runtimes, herdr, claude, claude-mcp-servers, worktree,
│                             #   quarantine, hm-warnings, esa, line
│                             #   (sudo-askpass: SUDO_ASKPASS を gpg.nix の pinentry パッケージに
│                             #   つなぐ helper。tty のない agent セッションから sudo を成立させる、
│                             #   docs/claude/sudo-askpass.md;
│                             #   esa: esa.io MCP token supply, personal identity only,
│                             #   ADR-0022 — imported from identities/personal.nix, not here;
│                             #   line: LINE を chromium --app の独立ウィンドウとして宣言配備、
│                             #   personal identity only — esa と同じ理由で identities/personal.nix
│                             #   からのみ import;
│                             #   claude-mcp-servers: reconcile 型で ~/.claude.json の
│                             #   .mcpServers(user scope の MCP サーバー)を宣言集合に
│                             #   一致させる口。populate は identities/personal.nix /
│                             #   esa.nix、PR #315;
│                             #   nixgl: shared nixGL wrapper function, ADR-0006, consumed by
│                             #   desktop.nix and identities/personal.nix, #9;
│                             #   quarantine: two shared options — managedFiles moves an
│                             #   existing real file to .pre-nix before home-manager adopts
│                             #   it, strayFiles renames an unmanaged leftover to .bak so it
│                             #   stops competing with a managed one; neither ever deletes)
├── config/                   # literal config files, deployed verbatim via xdg.configFile / home.file
│   ├── zsh/                  # zsh modules (loaded in numeric order)
│   ├── claude/               # hooks/: plan-review gate, wrap-up inbox, plan-view,
│   │                         #   plan-scope-gate, plan-precedent-gate,
│   │                         #   plan-fresh-gate, pr-gate,
│   │                         #   attribution-guard, issue-index, sign-prewarm,
│   │                         #   git-worktree-allow, git-stash-guard,
│   │                         #   herdr-sidebar-metadata (hook half only);
│   │                         #   (public-publish-guard moved upstream to
│   │                         #   tarotene/publish-guard, ADR-0009 — deployed
│   │                         #   from a flake input, not this source tree);
│   │                         #   assets/: non-hook files kept beside their
│   │                         #   consumer for source-tree purity (ADR-0007) —
│   │                         #   plan-view.css, copilot-plan-review's output
│   │                         #   schema; statusline/: claude-statusline.sh +
│   │                         #   claude-usage.sh (herdr tab-bar command, not a
│   │                         #   hook) — deployed alongside hooks/ under
│   │                         #   ~/.claude/hooks/ regardless of this split;
│   │                         #   commands/, skills/: slash commands + skills
│   │                         #   (also mirrored into .agents/skills/, ADR-0016)
│   ├── git/hooks/            # core.hooksPath targets: pre-push (worktree push guard),
│   │                         #   pre-commit (protected-branch guard, then chains to
│   │                         #   the repo-local hook)
│   ├── herdr/                # Herdr config.toml (theme + sidebar rows + keybindings), fully managed —
│   │                         #   xdg.configFile deploys it verbatim (store symlink,
│   │                         #   read-only; in-app settings writes fail by design);
│   │                         #   oshi-marks.tsv: hololive talent name → fan-mark
│   │                         #   emoji lookup for the $oshi sidebar token, shared
│   │                         #   by all 3 agent metadata hooks, keyed against
│   │                         #   patches/herdr-worktree-names.patch's talent list
│   ├── codex/hooks/          # herdr-codex-metadata.sh: sidebar reporter for Codex CLI
│   │                         #   panes (herdr-sidebar-metadata.md); deployed beside
│   │                         #   herdr's own ~/.codex/ integration, not registered
│   │                         #   through Codex's own config surface; attribution-guard.sh:
│   │                         #   thin PreToolUse adapter that sources claude/hooks/
│   │                         #   attribution-guard.sh's decision engine (#192)
│   ├── copilot/              # agents/: plan-reviewer.agent.md (copilot-plan-review);
│   │                         #   hooks/: herdr-copilot-metadata.sh, same role as
│   │                         #   codex/hooks/ above, for Copilot CLI panes;
│   │                         #   attribution-guard.sh: same adapter role as the
│   │                         #   Codex one, for Copilot's preToolUse (#192)
│   ├── github-audit/         # ADR-0020 closed vocabularies (PUBLIC repos only):
│   │                         #   codename-registry.tsv, descriptive-species.tsv,
│   │                         #   site-domains.tsv — deployed verbatim to
│   │                         #   ~/.config/github-audit/; PRIVATE-repo entries
│   │                         #   live in a *.local.tsv sibling on-disk only,
│   │                         #   never in this tree (docs/claude/public-publish-guard.md)
│   ├── fontconfig/conf.d/      # 75-color-emoji-fallback.conf: 端末フォント
│   │                         #   (FiraCode Nerd Font)を明示要求したパターンに
│   │                         #   限って Noto Color Emoji を strong binding で
│   │                         #   追加し、絵文字をカラー字形にする(#305 系統 B、
│   │                         #   docs/claude/herdr-sidebar-metadata.md)
│   ├── applications/         # Alacritty.desktop: launcher entry overriding nixpkgs',
│   │                         #   with Exec=/TryExec= pinned to the nixGL-wrapped store
│   │                         #   path via pkgs.replaceVars (ADR-0029) — a bare Exec=
│   │                         #   resolves against the COSMIC session's PATH, not the
│   │                         #   one home-manager reasons about
│   ├── shell/                # common_env (sourced by 20-environment.zsh) + profile
│   │                         #   (deployed as ~/.profile via home.file, Linux only):
│   │                         #   ad-hoc installer dirs (.cargo/.deno/.bun) are appended,
│   │                         #   never prepended, so nix keeps winning (ADR-0029)
│   ├── git/, alacritty/, sheldon/, fcitx5/, environment.d/, ...
│   └── starship.toml
├── packages/declarative/
│   └── apt-packages.txt      # system-layer packages ONLY
├── scripts/                  # mix of home-manager-deployed user-environment tools
│   │                         #   (hms, git-shelve/unshelve,
│   │                         #   git-prune-branches, git-audit/prune-worktrees,
│   │                         #   git-worktree-create-guard, claude-plan-model,
│   │                         #   esa-mcp-launcher)
│   │                         #   and true escape-hatch /
│   │                         #   diagnostic scripts (install-packages,
│   │                         #   install-falcon-sensor, fix-ssh-permissions)
│   │                         #   — not escape-hatch-only
│   │                         #   (SOPS runtime secrets loader retired,
│   │                         #   ADR-0010 — no consumer had survived it)
│   ├── hms.sh                # canonical apply wrapper (deployed to ~/.local/bin/hms):
│   │                         #   switch + daemon-reload + fcitx5 restart + verification
│   ├── install-packages.sh   # thin system-layer apt installer (#216)
│   ├── install-falcon-sensor.sh # company EDR agent installer; FALCON_CID is
│   │                         #   prompted interactively at install time, not
│   │                         #   read from SOPS (ADR-0010)
│   ├── fix-ssh-permissions.sh
│   ├── detach-open.sh        # deployed as ~/.local/bin/open AND ~/.local/bin/xdg-open
│   │                         #   (shadows the system xdg-open, which blocks in the
│   │                         #   foreground on COSMIC), and as the $BROWSER target
│   ├── git-shelve             # worktree-tagged `git stash push` wrapper
│   │                         #   (deployed to ~/.local/bin/git-shelve, called as
│   │                         #   `git shelve` via git's subcommand resolution)
│   ├── git-unshelve           # resolves + applies + drops this worktree's own
│   │                         #   shelve entry (SHA-based, TOCTOU-safe drop)
│   ├── git-prune-branches     # deletes local branches whose upstream is [gone]
│   │                         #   (deployed to ~/.local/bin, called as `git prune-branches`)
│   ├── git-audit-worktrees    # detects (never deletes) stale herdr worktree
│   │                         #   registrations, both classes (prunable + orphaned)
│   │                         #   (deployed to ~/.local/bin + a systemd user timer)
│   ├── git-prune-worktrees    # removes what git-audit-worktrees detects, both
│   │                         #   classes — prunable registrations (git worktree
│   │                         #   prune --expire=now) and orphaned checkouts
│   │                         #   (git worktree remove, --force optional)
│   │                         #   (deployed to ~/.local/bin, called as `git prune-worktrees`)
│   ├── git-checkout-freshness # fetch + ff-only merge one or more parent
│   │                         #   checkouts onto origin/<base> when clean
│   │                         #   and on the default branch (deployed to
│   │                         #   ~/.local/bin + a systemd user timer, #78)
│   ├── github-audit           # read-only cross-repository GitHub audit,
│   │                         #   unified across 5 domains — rulesets (#130) /
│   │                         #   charters / naming (ADR-0014) / settings /
│   │                         #   renovate (ADR-0015; deployed to ~/.local/bin,
│   │                         #   manual command, no timer)
│   ├── github-rulesets-apply  # seeds standard rulesets via the governance
│   │                         #   skills' apply-rulesets.sh (#153; deployed
│   │                         #   to ~/.local/bin, manual command)
│   ├── claude-plan-model      # cycles Opus Plan Mode's (plan side, execution
│   │                         #   side) pair — fable/sonnet, opus/sonnet,
│   │                         #   fable/opus — and re-resolves the concrete
│   │                         #   model IDs from the installed claude binary's
│   │                         #   baked catalog (deployed to ~/.local/bin; the
│   │                         #   `sync` subcommand runs from home-manager
│   │                         #   activation, `--selftest` from CI)
│   ├── git-worktree-create-guard # PreToolUse guard helper for `git worktree add`
│   │                         #   (deployed to ~/.local/libexec, not ~/.local/bin)
│   ├── esa-mcp-launcher       # decrypts ~/.config/esa/token.gpg and execs the
│   │                         #   esa.io MCP server (deployed to ~/.local/libexec,
│   │                         #   personal identity only, ADR-0022)
│   ├── sudo-askpass           # SUDO_ASKPASS helper: speaks pinentry's Assuan
│   │                         #   protocol directly (GETPIN) so sudo can read its
│   │                         #   password without a controlling terminal
│   │                         #   (deployed to ~/.local/libexec, --selftest from CI
│   │                         #   against a stubbed pinentry only, docs/claude/sudo-askpass.md)
│   ├── gpg-subkey             # generate/rotate/export/sync/status/remind
│   │                         #   subcommands for [S]/[E] subkey management
│   │                         #   (deployed to ~/.local/bin, ADR-0003 Amendment)
│   ├── writing-style-hub      # resolves the private style-guide hub's path
│   │                         #   via marker file / env var indirection, for
│   │                         #   the writing-style skill (deployed to
│   │                         #   ~/.local/bin, --selftest from CI, #115)
│   ├── register-codex-hooks   # activation-only (writeShellScript, not a
│   │                         #   deployed file): idempotent variadic merger for
│   │                         #   ~/.codex/hooks.json — worktree.nix registers the
│   │                         #   worktree guard/context hooks, herdr.nix registers
│   │                         #   the sidebar-metadata reporter, in one shared file
│   ├── register-copilot-hooks # activation-only (writeShellScript, not a
│   │                         #   deployed file): idempotent variadic merger for
│   │                         #   ~/.copilot/settings.json's native "hooks" object
│   │                         #   (herdr.nix registers the sidebar-metadata reporter)
│   └── fcitx5-key-trace.pl   # fcitx5 trace redactor + trigger-key detector (#14)
├── keys/                     # committed public keys (non-secret), imported at activation
├── bootstrap.sh              # greenfield: Nix install → apt → home-manager switch
├── docs/
│   ├── README.md             # index of everything below, by category
│   ├── adr/0001..0017        # architecture decision records
│   ├── setup.md              # step-by-step host setup guide
│   ├── operations.md         # the canonical apply (hms) + routine flake update + tool-layer decision flow
│   ├── cutover-runbook.md    # per-host migration procedure
│   ├── git-sync.md           # machine-wide git config + hooks guarding herdr's parallel worktrees
│   ├── ime-chrome-diagnosis.md  # fcitx5 trigger-key investigation record (#14)
│   ├── worktree-lifecycle.md # herdr worktree create/prune lifecycle across scripts/hooks
│   ├── github-audit.md       # unified 5-domain audit (ADR-0015): rulesets
│   │                         #   rule-type-union judgement, charters schema/
│   │                         #   routing (ADR-0016), naming class pattern
│   │                         #   (ADR-0014), settings, renovate — why one
│   │                         #   command instead of 5 sibling scripts, why
│   │                         #   judgement skips any LLM call
│   ├── repo-lifecycle.md     # visibility/license policy, Maintain/Archive/
│   │                         #   Delete triage criteria, deprecate-then-archive
│   │                         #   checklist, theme-monorepo consolidation
│   │                         #   (snapshot+PROVENANCE) — migrated from a
│   │                         #   private portfolio-management repo (ADR-0023);
│   │                         #   lifecycle layer, distinct from github-audit's
│   │                         #   drift layer
│   ├── claude/               # Claude Code tooling docs (design + rationale per hook)
│   │   ├── copilot-plan-review.md  # Copilot plan-review gate: read-only custom agent, why it gates on severity, not on a verdict
│   │   ├── git-worktree-allow.md # PreToolUse hook: validated programmatic allow for `git -C <worktree>`
│   │   ├── git-stash-guard.md    # PreToolUse hook: deny bare `git stash` (shared stack across worktrees)
│   │   ├── attribution-guard.md  # PreToolUse hook: deny a gh pr|issue
│   │   │                     #   create|edit|comment / gh pr review whose body
│   │   │                     #   has no attribution footer (escape hatch:
│   │   │                     #   `No-Attribution: <reason>`); decision engine
│   │   │                     #   shared across Claude Code / Codex CLI / Copilot
│   │   │                     #   CLI via thin per-agent adapters (#192)
│   │   ├── public-publish-guard.md # PreToolUse hook: deny/ask on git push /
│   │   │                     #   gh pr|issue create|edit|comment/MCP GitHub
│   │   │                     #   tool calls that would leak a company/private
│   │   │                     #   repo name (#130 era incident) — design now
│   │   │                     #   lives upstream in tarotene/publish-guard
│   │   │                     #   (ADR-0009); this doc covers dotfiles wiring only
│   │   ├── worktree-fresh-base.md # SessionStart hook: silently fast-forward a
│   │   │                     #   pristine worktree to origin/<base>
│   │   ├── git-checkout-freshness.md # systemd user timer: fast-forward a
│   │   │                     #   parent checkout to origin/<base> on a
│   │   │                     #   10-minute interval (one level up from
│   │   │                     #   worktree-fresh-base.md, #78)
│   │   ├── plan-fresh-gate.md    # PreToolUse/ExitPlanMode hook: ff-only when
│   │   │                     #   pristine, deny when origin/<base>'s progress
│   │   │                     #   intersects plan-referenced files, converges
│   │   │                     #   via a denied-SHA session state
│   │   ├── issue-index.md        # SessionStart hook: inject an Issue index, not a full crawl
│   │   ├── pr-gate.md            # Stop hook: PR completion barrier (CI/push/issue-link/visual-evidence)
│   │   ├── pr-description.md     # PR body skeleton + mandatory Before/After
│   │   │                     #   visual evidence (gate: G_visual, skill: pr-description)
│   │   ├── sign-prewarm.md       # SessionStart hook: pre-warm the git-signing
│   │   │                     #   [S] AND esa MCP token.gpg [E] passphrase
│   │   │                     #   caches (#252) — independent gpg-agent cache
│   │   │                     #   entries, warmed independently
│   │   ├── plan-view.md          # /plan-view: render the in-progress plan to HTML in Chrome
│   │   ├── wrapup-inbox.md       # Stop hook: out-of-scope findings → issue-filing inbox
│   │   ├── wrapup-chores.md      # skill: triage the wrap-up inbox into one batch chores PR
│   │   ├── herdr-sidebar-metadata.md # Herdr sidebar: per-agent mode/model/branch
│   │   │                     #   via pane metadata (Claude full, Codex/Copilot
│   │   │                     #   branch+model only; tab-bar usage deferred, #117)
│   │   ├── claude-permissions.md # permissions.allow: declarative, idempotent jq merge like registerHooks
│   │   ├── esa-mcp.md         # esa.io MCP サーバのトークン供給: ホストローカル
│   │   │                     #   GPG 暗号化ファイル + 専用 launcher +
│   │   │                     #   ~/.claude.json への宣言的 merge(personal
│   │   │                     #   identity 層限定、ADR-0022)
│   │   ├── writing-style.md      # skill: 執筆規約への薄いポインタ。ハブの
│   │   │                     #   絶対パスはマーカーファイル/環境変数で間接
│   │   │                     #   参照し、無ければ明示的に失敗する(#115)
│   │   ├── opusplan-model-aliases.md # Opus Plan Mode はエイリアスのペア:
│   │   │                     #   opus/sonnet の 2 本を乗っ取り、モードを
│   │   │                     #   (Plan 側, 実行側) のペア 3 種として
│   │   │                     #   claude-plan-model で巡回する
│   │   │                     #   (モード=実行時状態 / 具体 ID=宣言が毎回引き直し)
│   │   ├── claude-usage.md   # Herdr tab bar: 5h/weekly rate-limit usage +
│   │   │                     #   pace-at-reset projection, from the undocumented /usage API
│   │   ├── global-claude-md.md   # global ~/.claude/CLAUDE.md: injects research
│   │   │                     #   discipline into every session
│   │   ├── diagramming.md        # skill: diagramming (SKILL.md + cases.md)
│   │   ├── living-description.md # skill: treat Issue/PR body as living source of
│   │   │                     #   truth, not an at-filing-time snapshot
│   │   ├── skill-gardening.md    # skill: crystallize session learnings into this
│   │   │                     #   repo (meta-skill)
│   │   ├── test-grounding.md     # skill: ground verification items in facts before
│   │   │                     #   writing test procedures
│   │   ├── scope-inventory.md    # global CLAUDE.md rule + skill: enumerate every
│   │   │                     #   requirement item before planning so none is
│   │   │                     #   silently dropped; gate: plan-scope-gate.sh
│   │   ├── precedent-grounding.md # global CLAUDE.md rule + skill: for every
│   │   │                     #   non-obvious design decision in a Plan, cite
│   │   │                     #   the prior art it follows or deviates from
│   │   │                     #   (`## 先行例との対比`, ADR-0012 — replaces
│   │   │                     #   asking for "adversarial review" per prompt);
│   │   │                     #   critic: copilot-plan-review lens A, gate:
│   │   │                     #   plan-precedent-gate.sh
│   │   ├── repo-charter.md       # skill: README/CONTRIBUTING charter schema
│   │   │                     #   (purpose sentence / Scope / CONTRIBUTING
│   │   │                     #   Issues section / naming class / topics),
│   │   │                     #   ADR-0013 + ADR-0016 + ADR-0017
│   │   └── github-audit-triage.md # skill: monitor-driven bulk remediation
│   │                         #   across repositories (ADR-0015 LLM node);
│   │                         #   absorbed charter-sweep (#180)
│   ├── falcon-sensor.md      # EDR agent notes
│   └── nixification-roadmap.md
└── .github/workflows/        # nix.yml (flake check + per-host build) + ci.yml (slim shellcheck)
```

## Architecture Decision Records

- **ADR-0001** — home-manager is the source of truth; apt + per-project runtimes are escape hatches.
- **ADR-0002** — runtime consolidation (Java/Go → mise; rustup/uv kept) + hybrid config translation.
- **ADR-0003** — secrets & identity: YubiKey-rooted key model. **See the Amendments** for the deployed model ([S] *and* [E] subkeys on-disk per-machine as of Amendment 4/#252 — the card-backed originals are kept live as a fallback, never revoked; two identities; host-local `.sops.yaml`; migration ⊆ rotation). The runtime-decrypted-SOPS Decision item is superseded by **ADR-0010** (retired — no consumer survived a re-audit).
- **ADR-0004** — repo identity & relocation (keep the `dotfiles` name; publish to public `tarotene/dotfiles` via clean orphan history; no semver releases).
- **ADR-0005** — shell-extension init gates on binary existence, never on auth credentials.
- **ADR-0006** — nix GUI apps carry their own GL stack: `/run/opengl-driver` is NixOS-only and the system mesa cannot be loaded into a nix process, so GL-using GUI packages are wrapped per-package with `nixGL` (nix's mesa). The system graphics stack stays untouched in apt.
- **ADR-0007** — naming & layout conventions: extension policy (drop `.sh` from the deployed name for PATH-resolved executables), shebang policy, hook-role vocabulary (`-guard`/`-gate`/`-allow`/no suffix, new hooks only), `config/claude/hooks/` source-tree purity, docs-correspondence principle, and `_DIR` env-var suffixing. No retroactive bulk rename of existing files.
- **ADR-0008** — documentation artifact selection: a new decision or piece of research goes to (1) an investigation record if it decays over time (external preview status, tool version, open-issue counts), (2) an ADR if it is a single significant decision (Nygard's five areas), even at single-developer scope, (3) `docs/claude/<name>.md` if it is the living design rationale for one hook/skill/tool, or (4) existing docs otherwise. ADRs stay immutable; link out to decaying facts rather than embedding them.
- **ADR-0009** — public-publish-guard's upstream split: the guard is now maintained in a separate public repo (`tarotene/publish-guard`), consumed here as a pinned flake input, because this repo's "No semver releases" policy and orphan-history rewrites (ADR-0004) are structurally incompatible with plugin-distribution commit-SHA/tag pinning.
- **ADR-0010** — retirement of the SOPS runtime secrets channel (the shell-startup GPG PIN prompt): a consumer audit found every secret it decrypted had already migrated away or gone unused, so the loader, wrapper, setup script, and home-manager wiring are removed entirely. Supersedes the runtime-SOPS Decision item of ADR-0003.
- **ADR-0011** — local activity log capture: atuin (offline-only, no sync/update-check) plus a Claude Code turn-boundary JSONL log (`agent-events.jsonl`), both write-only capture points whose path/field contract a separate downstream repository depends on.
- **ADR-0012** — precedent grounding over prompted adversarial review: instead of turning the user's per-prompt "run an adversarial review" / "check the literature" requests into an abstract standing CLAUDE.md instruction (which a literature survey found does not improve design/reasoning tasks), each non-obvious design decision in a Plan is grounded against prior art in the plan body itself; a context-isolated critic (copilot-plan-review's lens A) audits the citations, and a deterministic gate (`plan-precedent-gate.sh`) enforces the section's form.
- **ADR-0013** — README charter schema enforced across every self-authored repository: a purpose sentence (mirrored verbatim by the GitHub description), `## Scope`, `## Issue litmus` (judging question + accepted/rejected examples), and at least one topic — enforced at creation time by the `repo-charter` skill and audited after the fact by `github-audit charters`, deliberately without any LLM call. The README schema itself is partially superseded by ADR-0016; the Issue litmus item is partially superseded by ADR-0017 (moved to CONTRIBUTING.md's `## Issues` section, "Issue litmus" vocabulary retired).
- **ADR-0014** — repository naming class taxonomy (codename / descriptive / pj / site), authoritative record in GitHub topics; format match is machine-judged, class assignment is a human decision surfaced through `github-audit-triage`. Decision 1 (the four-class taxonomy) is superseded by ADR-0026 (splits codename, adds an orthogonal lifecycle axis); the rest of this ADR stands.
- **ADR-0015** — diagnostics unified into the `github-audit` CLI, structured as a deterministic node (audit) plus an LLM node (`github-audit-triage`) triage loop; retires charter-sweep's auto-merge behavior in favor of a PR-only completion definition.
- **ADR-0016** — repository document canon: fixed README section schema, Issue litmus moved to CONTRIBUTING.md, a closed root-file allowlist, per-file language-mixing ban, and full separation of human-facing docs from AI-facing docs (AGENTS.md as the AI canon, CLAUDE.md as a router, skills routed through `.agents/skills/`) — grounded in standard-readme, GitHub's own docs, and Art of README. Partially supersedes ADR-0013's README schema and AGENTS.md treatment. This file is a direct consequence of that ADR's Decision 5. Decision 2 (Issue litmus placement) is partially superseded by ADR-0017.
- **ADR-0017** — CONTRIBUTING.md gets its own fixed section schema (`## Issues` with a judging question + Accepted/Rejected examples, `## Pull requests`, `## Expectations`), retiring the self-invented "Issue litmus" vocabulary in favor of GitHub's own "Issues" terminology. Grounded in GitHub's own docs, Open Source Guides, and the nayafia/contributing-template precedent. Partially supersedes ADR-0013 Decision 1's Issue litmus item and ADR-0016 Decision 2.
- **ADR-0018** — the first darwin host (altair) extends standalone home-manager as-is; the macOS system layer is Homebrew Bundle (a Brewfile, apt's symmetric counterpart) rather than nix-darwin. Linux-only modules branch on isLinux/isDarwin internally instead of being split into separate files.
- **ADR-0019** — new hosts are named with star codenames (no role/identity/generation embedded in the name); the logical hostname resolves via a marker file first, falling back to `hostname`. Renaming the existing three hosts is tracked separately (#214).
- **ADR-0020** — repository governance flips from event-driven event detection to a generative one: each naming class draws its variable slot from a closed vocabulary (a codename cast registry, a descriptive-species set, a site-domain set) instead of being validated after free-form creation; `required_status_checks` is derived from CI presence but never silently excused (`ci-absent` keeps a CI-less repository visible for triage); existing repositories are grandfathered by `createdAt` (no retroactive rename, ADR-0007 precedent); and naming/purpose-sentence drift is checked by blind re-derivation (derive a name/purpose from the repository's contents with the real one hidden, then compare) instead of by accumulating more inspection rules per incident. Partially supersedes ADR-0014 (adds the closed vocabularies on top of its four-class taxonomy).
- **ADR-0021** — `github-audit`'s ruleset baseline splits into a core layer (always required) and a review layer (Copilot code review + required conversation resolution, opt-in addin). Uses presence-detection rather than a phase-declaration ledger, so early-development repos are not forced into review round-trips. Partially amends ADR-0015's rulesets-domain baseline.
- **ADR-0022** — esa.io MCP token supply moves from a broken-supplier private repository (SOPS + direnv, a container for effectively one secret) to a host-local plain GPG-encrypted file + a dedicated launcher + declarative merge into `~/.claude.json`. That private repository is archived. First real application of ADR-0010's "re-choose the supply channel each time" — the sole sops consumer's disappearance also removes the `sops` package.
- **ADR-0023** — repository lifecycle governance (visibility/license policy, Maintain/Archive/Delete triage criteria, deprecate-then-archive checklist, theme-monorepo consolidation via snapshot+PROVENANCE) is migrated from a private portfolio-management repository into `docs/repo-lifecycle.md`, distinct from `github-audit`'s drift layer. The source private repository is archived once the migration is verified.
- **ADR-0024** — hook/CLI スクリプト群(約 40 本・15,000 行)の実装技術を Rust とする決定。bash 続投(writeShellApplication)は closure 固定は解けても保守性・表現力の主因を解決せず、Deno + TypeScript は closure 固定手法(deno2nix)がアーカイブ済みで must 制約未達のため不採用。`git-stash-guard.sh` の実移植 PoC で Rust の起動 1.2ms(50ms 予算の 1/30 以下)・出力完全一致・`cargo test` 移行を実測。一括移行はせず後続 Issue に段階分割する。調査記録: `docs/shell-successor-research.md`。
- **ADR-0025** — 自作・タグ付きリリース未達の pre-release CLI(実例: `tarotene/telepath`)の導入を、ホストローカルレジストリファイルによる opt-in 方式で実現する決定。dotfiles 側はスクリプトとスキーマのみ提供し、対象リポの名前は git 管理外のホストローカル設定ファイルにのみ記録する。対象リポ自体には一切触れない — 当初検討したマーカーファイル opt-in 方式(対象リポ自身に痕跡を置く)は、開発中の自作 OSS への不自然な露出になるため棄却。ADR-0001 への scoped exception。先行例: ADR-0020 の `*.local.tsv` パターン、ADR-0022 のホストローカル GPG ファイル。
- **ADR-0026** — 命名クラス体系(ADR-0014)を改訂する決定。`naming-codename` を「無意味な恣意的ラベル(ADR-0020 の閉語彙を継続適用)」の `naming-codename` と「著者固有の命名形態論に基づく造語(閉語彙なし)」の `naming-coined` に分割し、5 クラス体制にする。加えて `lifecycle-timeboxed`(外部成果物を持つ時限プロジェクト)/ `lifecycle-study`(研究・学習記録、完了・進行中いずれも可)という、`naming-*` とは独立に併用できるライフサイクル軸を新設する。完了済み研究アーカイブが `naming-descriptive` の受けに事後的に流れていた問題と、`naming-codename` が意味的に異質な命名を混在させていた問題を、別々の直交する軸として解決する。ADR-0014 Decision 1 を supersede。

- **ADR-0029** — PATH の優先順位を ADR-0001 の*執行機構*として宣言下に置く決定。nix は `/etc/profile.d/nix.sh` がシステムレベルで PATH に入れるため、ユーザレベルの prepend は構造的に必ず nix を追い越す — 実際 `~/.profile` の `. "$HOME/.cargo/env"` が `~/.cargo/bin` を先頭に置き、宣言済みの alacritty 0.17.0-nixgl に代わって cargo 版 0.15.1 が起動し続けていた(shadow は計 10 件 + `deno`)。順序を `.local/bin` → nix → system → ad-hoc installer dirs に規定し、ad-hoc インストーラの prepend を禁じ、`~/.profile` を home-manager 管理下(read-only store symlink)に取る。`.desktop` の `Exec=` も store path に固定して二枚重ねにする。ADR-0001 の決定自体は変えない。

## Development Rules

### Nix modules (`home/`)
- Modules are the source of truth. A host module imports `home/common.nix` plus
  exactly one identity module; `common.nix` imports everything under `modules/`.
- `homeConfigurations.<hostname>` in `flake.nix` is keyed by hostname so
  `home-manager switch --flake .#"$(hostname)"` auto-selects per machine.
- Identity-scoped values (git `user.name`/`email`, browser default) go in
  `identities/`; per-machine values (signing key bound to the host's YubiKey/[S]
  subkey) go in `hosts/`.
- Format with `nix fmt` (nixfmt-tree — a treefmt wrapper that feeds nixfmt only
  the `*.nix` files, so no arguments are needed).

### Hybrid translation (ADR-0002)
- **Keep working config files literal** and deploy them via `xdg.configFile` /
  `home.file` (the `config/zsh/modules/*.zsh`, `starship.toml`, `alacritty.toml`,
  `sheldon/plugins.toml`, ...). Do not rewrite battle-tested config into Nix DSL
  wholesale.
- **Use Nix DSL only where interpolation pays** — per-host/identity values, or
  where a `programs.*` module removes real boilerplate.
- Track literal configs worth nixifying later in `docs/nixification-roadmap.md`.

### Shell-extension init (ADR-0005)
- `eval "$(tool init …)"` / `source <(tool …)` MUST gate only on binary
  existence (`command -v`), never on auth credentials (e.g. `GITHUB_TOKEN`).
  Suppress tool warnings with `2>/dev/null`, not by skipping the loader — a
  token-gated loader breaks in the token-less home-manager session.

### Git sync guards (herdr's parallel worktrees)
- `home/modules/git.nix` sets `pull.ff=only` / `fetch.prune` / `push.autoSetupRemote` /
  `rerere.enabled` / `merge.conflictStyle=zdiff3` machine-wide — herdr creates a
  worktree from the parent checkout's HEAD without fetching or setting an upstream.
- `config/git/hooks/pre-commit` blocks a direct commit on `main`/`master`.
  Bypass with `GIT_ALLOW_MAIN_COMMIT=1`, **not** `--no-verify` — `--no-verify`
  would also skip the chained repo-local pre-commit (other repos' ruff/mypy).
- `git prune-branches` (`scripts/git-prune-branches`, deployed to `~/.local/bin`
  and resolved via git's `git-<subcommand>` mechanism, no alias) deletes
  local branches whose upstream is `[gone]`, after listing them and asking once.
  Full rationale: `docs/git-sync.md`.

### Escape-hatch scripts
- Keep the surviving scripts small and POSIX/bash-lint clean (shellcheck
  severity=error in CI). Support `--dry-run` where it makes sense.
- `install-packages.sh` installs the **system layer only** — user-space CLIs
  belong in `home/modules/packages.nix`, not apt.

### CI (nix-centric)
- `nix.yml` runs `nix flake check` + a per-host activation build matrix.
- `ci.yml` is a slim shell pass: shellcheck the surviving scripts, `bootstrap.sh`
  + `install-packages.sh` `--dry-run`, and a zsh module syntax check.

### Scope inventory (Claude Code only)
- A request with multiple items (a Tracking Issue with sub-issues, a bulleted
  ask) gets a `## 要求インベントリ` (requirement inventory) at the top of its
  plan: every item, verbatim, with an `Rn` id, before design starts.
- Every `Rn` gets a disposition: which stage implements it, or one of the
  closed tags `Blocked-Upstream:` / `Obsolete:` / `User-Excluded:`. Size,
  effort, or session length are never valid reasons to drop an item.
  A referenced Issue that isn't an implementation target gets
  `Reference-Only: #N — <reason>` instead of being silently ignored.
- The `PreToolUse`/`ExitPlanMode` hook (`plan-scope-gate.sh`) enforces this:
  it cross-checks referenced Issues' sub-issues (or unchecked task-list items
  as a fallback) against the plan's inventory, and separately checks the
  inventory section's internal consistency (every `Rn` has a disposition,
  tags are from the closed set, no duplicates). It calls no LLM. Full
  rationale + how-to: `docs/claude/scope-inventory.md`, skill:
  `config/claude/skills/scope-inventory/`.

### Pull request descriptions
- A coding task is **not done** until the PR exists: commit → push →
  `gh pr create` in one motion, without pausing to ask. The Stop hook
  (`G_pr` in pr-gate.sh) enforces this.
- Every PR body **must** either close an issue or say why there is none:
  - `Closes #<n>` — one line per issue (`Fixes`/`Resolves` and
    `owner/repo#<n>` work too). Without this, merging does not touch the issue
    and a finished piece of work sits open until someone re-triages it by hand —
    which is exactly how #28 and #29 survived months after being solved.
  - `No-Issue: <reason>` — when the work genuinely has no issue behind it
    (feature work born mid-session is the common case). This is a real escape
    hatch, not a formality: do not file a throwaway issue just to have a number.
- Keep the keyword **out of code spans and fences**. GitHub ignores
  `` `Closes #1` `` and anything inside ``` fences, so a quoted example does not
  link anything — and the gate deliberately reads the body the same way GitHub
  does.
- The `G_link` judgement in `config/claude/hooks/pr-gate.sh` blocks the Stop hook
  when neither is present. Rationale: `docs/claude/pr-gate.md`.
- Closing keywords only fire when the PR targets the **default branch**. On a
  stacked PR the gate says so, but it will not stop you — carry the keyword on
  the stage that actually closes that issue (`stacked-pr` skill, §6): each
  stage's `Closes #N` / `No-Issue:` line reflects what *that stage* completes,
  not the stack as a whole. GitHub auto-retargets a stage's base to the
  default branch once the stage below it merges, so a stage's own keyword
  fires once it lands — you don't need to close by hand unless a stage never
  merges on its own.
- **Dependent PRs go into a stacked PR**, not parallel PRs off `main`: when a
  later change references an earlier PR's output, or edits the same section
  of the same file, base it on the parent branch instead. Full rationale +
  how-to: `docs/claude/stacked-pr.md`, skill: `config/claude/skills/stacked-pr/`.
  A `Stack: <n>/<total> (base: #<parent>)` line goes next to `Closes #N` /
  `No-Issue:` when stacking; `pr-gate.sh` does not check it.
- Every PR body follows a 5-section skeleton (full rationale + how-to:
  `docs/claude/pr-description.md`, skill: `config/claude/skills/pr-description/`):
  ```
  Closes #N / No-Issue: <reason>

  ## 課題
  ## 解決策
  ## Before / After
  ## 検証
  ## 要確認   (omit the whole section if there is nothing)
  ```
- `## Before / After` needs one of: an uploaded image (`gh pr create|edit
  --attach './after.png#Alt'`, gh >= 2.99.0), a fenced code block under that
  heading for text-only diffs, or `No-Visual: <reason>` when the change has no
  visible effect (GUI/Web **and** terminal/TUI appearance both count as
  visible). The `G_visual` judgement in `pr-gate.sh` blocks the Stop hook when
  none of the three is present — it rides the same terminal block as `G_link`,
  so both body fixes cost one round trip.

## Verification / Testing
- `nix flake check` — evaluates every host's activation package.
- `nix build .#homeConfigurations.<host>.activationPackage --no-link` — build a host.
- `hms` — canonical apply (pushed main); `hms .` applies the current
  checkout/worktree for pre-push verification (wraps switch + daemon-reload +
  fcitx5 restart; see `docs/operations.md`).
- `home-manager switch --flake .#"$(hostname)" -b backup` — the raw switch
  `hms` wraps (see runbook).
- Rollback via generations: `home-manager generations`, then `--rollback`.
- Provisioning procedures: `docs/cutover-runbook.md`.
- Routine flake update + which layer a new tool goes in: `docs/operations.md`.

## Scope boundaries
- Do **not** manage drivers, the display stack, or anything root-owned through
  Nix — that stays in the thin apt system layer.
- Do **not** re-introduce the procedural symlink/dev-tool/keybase installers;
  they were retired in Phase 4 (#218). Git history preserves them.
- Do **not** add a root-level `*.md` file outside README/CONTRIBUTING/CHANGELOG/
  AGENTS.md/CLAUDE.md/LICENSE* — deep documentation goes in `docs/` (ADR-0016).
