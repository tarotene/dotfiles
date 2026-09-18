---
name: rust-repo-governance
description: Bootstrap or replicate battle-tested GitHub governance (3-layer Rulesets, merge-base-diff CI, release-plz with OIDC Trusted Publishing, Renovate MSRV-safe config, git hooks, Justfile) from the telepath reference implementation into any Rust workspace repository. Use when asked to "撒く", "bootstrap governance", "apply rulesets", "apply GitHub settings", "set up release-plz", "replicate telepath's CI setup", "seed CI to a new Rust repo", "rulesets / release / renovate をまとめて適用", or "telepath の GitHub 設定を別リポジトリに持っていく".
---

## What this Skill does

1. Copies parameterised CI/CD templates (9 workflows, including a
   per-file language-mixing check — `lang-mix.yml` — + composite action + CODEOWNERS)
   and config files (renovate.json, release-plz.toml, cog.toml, rust-toolchain.toml, Justfile, git hooks,
   AGENTS.md/CLAUDE.md routing skeleton — ADR-0016 in tarotene/dotfiles)
   into the target repository, substituting `__PLACEHOLDER__` values for your repo's specifics.
2. Applies repository merge settings (squash-only, delete-on-merge, wiki/projects disabled — same
   baseline `github-audit`'s `settings` domain judges, ADR-0015 in tarotene/dotfiles) via `gh api`.
3. Creates the three GitHub Rulesets (Security / Quality / Workflow) that enforce
   branch protection, required status checks, Copilot review, and commit signatures.
4. Points you to `reference/manual-steps.md` for the steps that require browser flows:
   GitHub App creation, crates.io Trusted Publishing entry registration, first bootstrap publish.

---

## Step 0: Gather parameters

Before running anything, confirm the following values with the user:

| Parameter | Flag | Example |
|-----------|------|---------|
| GitHub owner | `--owner` | `acme` |
| Repository name | `--repo` | `my-lib` |
| Default branch | `--default-branch` | `main` (default) |
| MSRV (short) | `--msrv` | `1.88` (default) |
| MSRV (full) | `--msrv-full` | `1.88.0` (default) |
| Canonical crate | `--canonical-crate` | `my-lib-core` — the crate that owns the git tag |
| CLI crate | `--cli-crate` | `my-cli` — the excluded crate under `tools/` |
| Target repo path | `--dest` | `/home/user/src/my-lib` |
| Firmware? | `--with-firmware` | pass flag if project has embedded firmware |

If any value is unclear, ask the user before proceeding.

Also check prerequisites:

```
gh auth status
command -v jq git just
```

---

## Step 1: Dry-run preview

Run seed.sh with `--dry-run` so the user can review what will change before
any files are written or API calls are made:

```bash
~/.claude/skills/rust-repo-governance/scripts/seed.sh \
  --owner OWNER --repo REPO \
  --canonical-crate CANONICAL --cli-crate CLI \
  --dest /path/to/repo \
  --dry-run
```

Show the output to the user. Confirm they are happy to proceed.

---

## Step 2: Apply

Run without `--dry-run`:

```bash
~/.claude/skills/rust-repo-governance/scripts/seed.sh \
  --owner OWNER --repo REPO \
  --canonical-crate CANONICAL --cli-crate CLI \
  --dest /path/to/repo
```

Individual sub-scripts can be run independently (useful for re-runs):

```bash
# Copy files only (no GitHub API calls):
~/.claude/skills/rust-repo-governance/scripts/copy-files.sh \
  --owner OWNER --repo REPO --canonical-crate CANONICAL --cli-crate CLI \
  --dest /path/to/repo

# Create Rulesets only (files already copied):
~/.claude/skills/rust-repo-governance/scripts/apply-rulesets.sh \
  --owner OWNER --repo REPO --msrv 1.88

# Apply repo settings only:
~/.claude/skills/rust-repo-governance/scripts/apply-repo-settings.sh \
  --owner OWNER --repo REPO
```

---

## Step 3: Manual ADJUST checklist

After `seed.sh` completes, open each copied file and address every `# ADJUST:`
comment. The table below lists the most important locations:

| File | What to update |
|------|----------------|
| `.github/workflows/host.yml` | `PATTERNS` regex — replace `telepath-(wire\|server\|...)` with your crate names |
| `.github/workflows/tools.yml` | `PATTERNS` regex; feature flags in `clippy-tools` and `mcp-test` Justfile recipes |
| `.github/workflows/msrv.yml` | `PATTERNS` regex — all workspace + excluded crate paths |
| `.github/workflows/firmware.yml` | Chip name, target triple, example path. **Delete this file** if no embedded firmware, and remove the `Firmware (cross-compile nRF52840-DK)` entry from `rulesets/quality.json` |
| `.github/workflows/release-plz.yml` | `host-pty-server` git-only package name; additional excluded crates in TREE_PAYLOAD |
| `.github/workflows/release-binaries.yml` | License file names (`LICENSE-MIT`, `LICENSE-APACHE`), README path |
| `.github/workflows/release-nudge.yml` | AGENTS.md anchor URL |
| `renovate.json` | `cargo.managerFilePatterns` — add your excluded crate paths; adjust embedded HAL package list |
| `release-plz.toml` | `[[package]]` entries — add your workspace crates, remove `host-pty-server` if not applicable |
| `Justfile` | Smoke test assertions in `host-pty-smoke`; feature combos in `clippy-tools` and `mcp-test` |

**Key invariant**: The `name:` field of each workflow job **must exactly match**
the `context` string in `rulesets/quality.json`. The `__MSRV__` and `__CLI_CRATE__`
placeholders are replaced in both places simultaneously by `seed.sh`, preserving
this match. But if you rename a job manually, update the Ruleset context too.

---

## Step 4: Manual steps (browser flows)

Follow `./reference/manual-steps.md` (in this Skill directory) for:

1. **GitHub App** — create with Contents/Issues/PRs R/W, get App ID + private key,
   set `RELEASE_PLZ_APP_ID` and `RELEASE_PLZ_APP_PRIVATE_KEY` as repo secrets.
2. **crates.io Trusted Publishing** — register each published crate with
   owner/repo/workflow=`release-plz.yml`.
3. **Bootstrap first publish** — one-time `publish-new` token for crates that
   don't yet exist on crates.io.

Short version of the secrets:
```
gh secret set RELEASE_PLZ_APP_ID --repo OWNER/REPO --body "<numeric-id>"
gh secret set RELEASE_PLZ_APP_PRIVATE_KEY --repo OWNER/REPO --body "$(cat key.pem)"
```

---

## Step 5: Verification

### Local sanity
```bash
# Validate Ruleset JSONs
jq -e . ~/.claude/skills/rust-repo-governance/rulesets/*.json

# Check hooks are wired
git -C /path/to/repo config --local core.hooksPath
# → should print: .githooks

# Try a Conventional Commit (should pass)
cd /path/to/repo && echo "feat: test" | just commit-check /dev/stdin
```

### After pushing the first PR

```bash
# Rulesets should appear:
gh api repos/OWNER/REPO/rulesets --jq '.[].name'
# → Security, Quality, Workflow

# Required checks should be registered in Quality Ruleset:
gh api repos/OWNER/REPO/rulesets --jq '.[] | select(.name=="Quality") | .rules[] | select(.type=="required_status_checks") | .parameters.required_status_checks[].context'

# Repo merge settings:
gh api repos/OWNER/REPO --jq '{allow_squash_merge, allow_merge_commit, allow_rebase_merge, delete_branch_on_merge}'
# → true / false / false / true
```

### CI gates

All 5 (or 4 without firmware) required checks should turn green on the first PR.
If `MSRV (X.Y)` or `Tools (my-cli CLI ...)` fail with "context not found",
verify job `name:` in the workflow files matches the Ruleset context strings exactly.

---

## Template structure reference

```
~/.claude/skills/rust-repo-governance/
├── SKILL.md                      ← this file (orchestration instructions)
├── templates/                    ← files copied by copy-files.sh
│   ├── .github/
│   │   ├── CODEOWNERS
│   │   ├── actions/rust-setup/action.yml
│   │   └── workflows/
│   │       ├── fmt.yml            required: Format check
│   │       ├── host.yml           required: Host (clippy + test + smoke)
│   │       ├── tools.yml          required: Tools (__CLI_CRATE__ CLI clippy + tests)
│   │       ├── msrv.yml           required: MSRV (__MSRV__)
│   │       ├── firmware.yml       optional: Firmware (cross-compile nRF52840-DK)
│   │       ├── release-plz.yml    release: tag + crates.io publish
│   │       ├── release-binaries.yml  release: 4-target binary builds
│   │       └── release-nudge.yml  maintenance: weekly stale PR nudge
│   ├── .githooks/{commit-msg,pre-commit,pre-push}
│   ├── renovate.json  release-plz.toml  cog.toml
│   └── rust-toolchain.toml  Justfile  .gitignore-snippet
├── rulesets/
│   ├── security.json    deletion + non_fast_forward
│   ├── quality.json     signatures + linear history + 5 status checks
│   └── workflow.json    squash-only + thread resolution + Copilot review
├── scripts/
│   ├── seed.sh           main orchestrator
│   ├── copy-files.sh     template copy + placeholder substitution
│   ├── apply-rulesets.sh gh api POST the 3 Rulesets
│   ├── apply-repo-settings.sh  gh api PATCH repo merge settings
│   └── setup-hooks.sh    git config core.hooksPath
└── reference/
    ├── releasing.md      release runbook (retrigger, recovery, Trusted Publishing)
    └── manual-steps.md   browser-flow checklist (App creation, crates.io setup)
```
