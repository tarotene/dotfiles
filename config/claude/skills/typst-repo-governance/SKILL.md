## What this Skill does

1. Copies parameterised CI/CD templates (6 workflows, including a per-file
   language-mixing check — `lang-mix.yml` — + composite action + CODEOWNERS)
   and config files (renovate.json, cliff.toml, .yamllint, Justfile, git hooks,
   AGENTS.md/CLAUDE.md routing skeleton — ADR-0016 in tarotene/dotfiles)
   into the target repository, substituting `__PLACEHOLDER__` values for your repo's specifics.
2. Applies repository merge settings (squash-only, delete-on-merge, wiki/projects disabled — same
   baseline `github-audit`'s `settings` domain judges, ADR-0015 in tarotene/dotfiles) via `gh api`.
3. Creates the core GitHub Rulesets (Security / Quality / Workflow) that enforce
   branch protection, required status checks, and commit signatures. A fourth,
   Review, is **opt-in** (`--with-review`) — Copilot code review auto-request +
   required conversation resolution before merge. It is left out by default
   because forcing that review round trip on every commit of an early-stage or
   pre-release repository was judged excessive and noisy (ADR-0020 in
   tarotene/dotfiles). Opt in once the repository is past that phase, or strip
   it back out of an already-governed repository with `--remove-review`.
4. Points you to `reference/manual-steps.md` for steps that require browser flows:
   Mend Renovate App installation, commit-signing setup, first-PR green-check.

This skill is the **Typst/document counterpart** of `rust-repo-governance`.
It covers the same governance goals (signing, linear history, squash-merge, 5 required
CI checks, dependency automation) using Typst-appropriate tooling instead of Cargo/clippy.

---

## Step 0: Gather parameters

Before running anything, confirm these values with the user:

| Parameter | Flag | Example |
|-----------|------|---------|
| GitHub owner | `--owner` | `tarotene` |
| Repository name | `--repo` | `cv` |
| Default branch | `--default-branch` | `main` (default) |
| Typst version pin (CI) | `--typst-version` | `0.14.2` (default) |
| Minimum Typst version | `--min-typst` | `0.14.0` (default) |
| ATS contact email | `--ats-email` | `you@example.com` |
| Target repo path | `--dest` | `/home/user/src/cv` |
| Review layer? | `--with-review` | pass flag to also apply the Review ruleset (Copilot code review + required conversation resolution — ADR-0020). Ask whether the repository is past its early-development phase before defaulting this on. |

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
~/.claude/skills/typst-repo-governance/scripts/seed.sh \
  --owner OWNER --repo REPO \
  --ats-email EMAIL \
  --dest /path/to/repo \
  --dry-run
```

Show the output to the user. Confirm they are happy to proceed.

---

## Step 2: Apply

Run without `--dry-run`:

```bash
~/.claude/skills/typst-repo-governance/scripts/seed.sh \
  --owner OWNER --repo REPO \
  --ats-email EMAIL \
  --dest /path/to/repo
```

Individual sub-scripts can be run independently (useful for re-runs):

```bash
# Copy files only (no GitHub API calls):
~/.claude/skills/typst-repo-governance/scripts/copy-files.sh \
  --owner OWNER --repo REPO --ats-email EMAIL \
  --dest /path/to/repo

# Create Rulesets only (files already copied):
~/.claude/skills/typst-repo-governance/scripts/apply-rulesets.sh \
  --owner OWNER --repo REPO --min-typst 0.14.0

# Apply repo settings only:
~/.claude/skills/typst-repo-governance/scripts/apply-repo-settings.sh \
  --owner OWNER --repo REPO
```

---

## Step 3: Manual ADJUST checklist

After `seed.sh` completes, open each copied file and address every `# ADJUST:`
comment. The table below lists the most important locations:

| File | What to update |
|------|----------------|
| `Justfile` | `COMPILE_FLAGS`, `SRC_*`, `OUT_*`, and `verify` DOCS table |
| `.github/workflows/build.yml` | `PATTERNS` regex — match your source directory layout |
| `.github/workflows/fmt.yml` | `PATTERNS` regex; `inputs:` path passed to typstyle-action |
| `.github/workflows/min-typst.yml` | `PATTERNS` regex |
| `.github/workflows/pr-title.yml` | Allowed commit types if you use a custom set |
| `.github/workflows/release.yml` | PDF filenames in the `files:` block |
| `.github/workflows/metrics-reminder.yml` | Issue body; **delete this file** if not a CV project |
| `cliff.toml` | `tag_pattern` if your CalVer tag scheme differs |
| `renovate.json` | Scheduling, grouping rules |

**Key invariant**: the `name:` field of each workflow job MUST exactly match
the `context` string in `rulesets/quality.json`. The `__MIN_TYPST__` placeholder
is substituted in both places simultaneously by seed.sh, preserving this match.
If you rename a job manually, update the Ruleset context too.

---

## Step 4: Manual steps (browser flows)

Follow `./reference/manual-steps.md` for:

1. **Mend Renovate App** — install on your repo (tracks Actions pins + Typst version).
2. **Commit signing** — SSH or GPG signing for `required_signatures` Ruleset.
3. **First PR** — push the bootstrapped branch, open PR, wait for 5 green checks.
4. **Apply Rulesets + settings** — run apply-rulesets.sh and apply-repo-settings.sh
   after the first PR is green.

---

## Step 5: Verification

### Local sanity
```bash
# Validate Ruleset JSONs
jq -e . ~/.claude/skills/typst-repo-governance/rulesets/*.json

# Check hooks are wired
git -C /path/to/repo config --local core.hooksPath
# → should print: .githooks

# Test commit-msg hook
echo "feat: test"   | just commit-check /dev/stdin  # should pass
echo "bad message"  | just commit-check /dev/stdin  # should exit 1

# Build + ATS gate
just build && just verify
```

### After pushing the first PR

```bash
# Rulesets should appear:
gh api repos/OWNER/REPO/rulesets --jq '.[].name'
# → Security, Quality, Workflow (+ Review if seeded with --with-review)

# Required checks in Quality Ruleset:
gh api repos/OWNER/REPO/rulesets \
  --jq '.[]|select(.name=="Quality")|.rules[]|select(.type=="required_status_checks")|.parameters.required_status_checks[].context'
# → Build
# → Format check
# → Lint
# → Min Typst (X.Y.Z)
# → PR title (Conventional Commits)

# Repo merge settings:
gh api repos/OWNER/REPO \
  --jq '{allow_squash_merge,allow_merge_commit,allow_rebase_merge,delete_branch_on_merge}'
# → true / false / false / true
```

### Removing the review layer (ADR-0020)

`apply-rulesets.sh --owner OWNER --repo REPO --remove-review [--dry-run]`
handles the two layouts it can meet: a standalone `Review` ruleset (this
skill's own `review.json` layout) is deleted outright; `copilot_code_review`
or `required_review_thread_resolution: true` bundled into some *other*
active branch ruleset is stripped out via a fetch-modify-`PUT` round trip
(the update endpoint takes the same shape as create, not a partial patch).
An irregular layout the script won't recognize needs manual removal via
`gh api repos/OWNER/REPO/rulesets/<id>` + a hand-built `PUT`.

### CI gates

All 5 required checks should turn green on the first PR.

If `Format check` fails with typstyle errors: run `just fmt` to auto-fix, commit, push.
If `Min Typst (X.Y.Z)` fails: the project uses features from a Typst version newer than
`__MIN_TYPST__`. Bump `--min-typst`, update `compiler` in `typst.toml`, and update the
Ruleset context string to match (all three must stay in sync).

---

## Template structure reference

```
~/.claude/skills/typst-repo-governance/
├── SKILL.md                           ← this file (orchestration instructions)
├── templates/                         ← files copied by copy-files.sh
│   ├── .github/
│   │   ├── CODEOWNERS
│   │   ├── actions/typst-setup/action.yml   typst + fonts + poppler + just
│   │   └── workflows/
│   │       ├── build.yml          required: Build         (just build + just verify)
│   │       ├── fmt.yml            required: Format check   (typstyle --check + yamllint)
│   │       ├── lint.yml           required: Lint           (actionlint + zizmor)
│   │       ├── min-typst.yml      required: Min Typst (X.Y.Z)
│   │       ├── pr-title.yml       required: PR title (Conventional Commits)
│   │       ├── release.yml        release: tag + git-cliff + gh-release w/ PDFs
│   │       └── metrics-reminder.yml  maintenance: monthly CV metrics issue
│   ├── .githooks/{commit-msg,pre-commit,pre-push}
│   ├── Justfile                       build/verify/fmt/lint/commit-check/ci/release-notes
│   ├── renovate.json                  github-actions + typst/typst version tracking
│   ├── cliff.toml                     CalVer changelog (vYYYY.MM tags)
│   ├── .yamllint                      YAML style rules
│   └── .gitignore-snippet             dist/ + editor/OS (merge manually)
├── rulesets/                        (core layer applied by default; Review is opt-in — ADR-0020)
│   ├── security.json    deletion + non_fast_forward
│   ├── quality.json     signatures + linear history + 5 status checks
│   ├── workflow.json    squash-only (core; thread resolution NOT required here)
│   └── review.json      Copilot code review + required thread resolution (opt-in addin)
├── scripts/
│   ├── seed.sh                    main orchestrator
│   ├── copy-files.sh              template copy + placeholder substitution
│   ├── apply-rulesets.sh          gh api POST the core 3 Rulesets (+ Review with
│   │                              --with-review; --remove-review strips it back out)
│   ├── apply-repo-settings.sh     gh api PATCH repo merge settings
│   └── setup-hooks.sh             git config core.hooksPath
└── reference/
    ├── manual-steps.md            post-seed checklist (App install, signing, first PR)
    └── releasing.md               release runbook (CalVer, retrigger, git-cliff)
```
