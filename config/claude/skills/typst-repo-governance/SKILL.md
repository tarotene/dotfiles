---
name: typst-repo-governance
description: Bootstrap or replicate battle-tested GitHub governance (Security/Quality/Workflow core Rulesets always applied, plus an opt-in Review ruleset for Copilot code review + required conversation resolution — ADR-0021 in tarotene/dotfiles, per-file language-mixing CI check, cliff.toml, Renovate, git hooks, Justfile) into any Typst/document repository. Use when asked to "撒く", "bootstrap governance", "apply rulesets", "apply GitHub settings", "seed CI to a new Typst repo", "rulesets / release / renovate をまとめて適用", or "Typst リポジトリに governance を播く". This is the Typst/document counterpart of `rust-repo-governance`; use `astro-site-governance` instead for Astro site repositories.
---

## What this Skill does

1. Copies parameterised CI/CD templates (a consolidated `ci.yml` — every PR
   gate job plus the `CI passed` aggregate required check, ADR-591 in
   tarotene/dotfiles — + `pr-title.yml`, `release.yml`,
   `metrics-reminder.yml`, a per-file language-mixing check `lang-mix.yml`,
   `nav-docs.yml` + composite action, an ADR-number check `adr-number.yml`
   — ADR-380 in tarotene/dotfiles — + CODEOWNERS)
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
   pre-release repository was judged excessive and noisy (ADR-0021 in
   tarotene/dotfiles). Opt in once the repository is past that phase, or strip
   it back out of an already-governed repository — remove `.github/rulesets/
   review.json` from the declaration, then `apply-rulesets.sh OWNER/REPO
   --delete-ruleset Review` (see "Removing the review layer" below).
4. Points you to `reference/manual-steps.md` for steps that require browser flows:
   Mend Renovate App installation, commit-signing setup, first-PR green-check.

This skill is the **Typst/document counterpart** of `rust-repo-governance`.
It covers the same governance goals (signing, linear history, squash-merge, a
`CI passed`/`PR title` required-check pair, dependency automation) using
Typst-appropriate tooling instead of Cargo/clippy.

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
| Review layer? | `--with-review` | pass flag to also apply the Review ruleset (Copilot code review + required conversation resolution — ADR-0021). Ask whether the repository is past its early-development phase before defaulting this on. |

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

# Create Rulesets only (files already copied — the generic apply script,
# not part of this skill, reads OWNER/REPO's own .github/rulesets/*.json;
# ADR-0000-rulesets-declaration-in-repo):
apply-rulesets.sh OWNER/REPO --unverified-contexts

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
| `.github/workflows/ci.yml` — `build` job | `PATTERNS` regex — match your source directory layout |
| `.github/workflows/ci.yml` — `fmt` job | `PATTERNS` regex; `inputs:` path passed to typstyle-action |
| `.github/workflows/ci.yml` — `min-typst` job | `PATTERNS` regex |
| `.github/workflows/pr-title.yml` | Nothing to adjust — it calls tarotene/dotfiles' composite action (ADR-0031/ADR-591); the reported check context is simply this job's own `name: PR title`, no manual confirmation needed |
| `.github/workflows/close-linked-issues.yml` | Nothing to adjust — closes the issues a merged PR names with `Closes #N` even when the PR was stacked on a non-default base (GitHub only acts on the keyword for PRs targeting the default branch, tarotene/dotfiles#609); needs no secrets, only `issues: write` |
| `.github/zizmor.yml` | Nothing to adjust — `"tarotene/*": ref-pin` covers the composite action's symbolic-ref `uses:` (#491); zizmor's own blanket default (hash-pin) still applies to every other `uses:` |
| `.github/workflows/release.yml` | PDF filenames in the `files:` block |
| `.github/workflows/metrics-reminder.yml` | Issue body; **delete this file** if not a CV project |
| `cliff.toml` | `tag_pattern` if your CalVer tag scheme differs |
| `renovate.json` | Typst toolchain group, custom manager — scheduling/grouping/automerge live in the shared preset it extends |

**Key invariant**: the `name:` field of each workflow job in `ci.yml`
**must exactly match** the entries in `ci-passed`'s `needs:` (job *id*, not
`name:`) — the Ruleset only ever requires `CI passed`/`PR title`
(ADR-591), so individual job `name:` values are display labels and no
longer need to match anything in `.github/rulesets/quality.json`.
`workflow-naming-check` (tarotene/dotfiles, a required check on every PR)
verifies this `needs:` coverage automatically.

**`pr-title.yml`** has no `workflow_call` concatenation to worry about
(ADR-591 replaced the old reusable-workflow form with a composite action,
docs/adr/591-ci-workflow-naming.md D3 in tarotene/dotfiles) — the reported
check context is simply this job's own `name: PR title`, pinned by the
`repo-governance-common/templates/.github/workflows/pr-title.yml` template
(this skill's copy is a symlink to it).

---

## Step 4: Manual steps (browser flows)

Follow `./reference/manual-steps.md` for:

1. **Renovate** — nothing to do per repository; the App is installed account-wide (see `repo-governance-common/reference/renovate-app.md`).
2. **Commit signing** — SSH or GPG signing for `required_signatures` Ruleset.
3. **First PR** — push the bootstrapped branch, open PR, wait for both
   required checks (`CI passed`, `PR title`) to go green.
4. **Apply Rulesets + settings** — run apply-rulesets.sh and apply-repo-settings.sh
   after the first PR is green.

---

## Step 5: Verification

### Local sanity
```bash
# Validate Ruleset JSONs
jq -e . ~/.claude/skills/typst-repo-governance/templates/.github/rulesets/*.json

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
# → CI passed
# → PR title

# Repo merge settings:
gh api repos/OWNER/REPO \
  --jq '{allow_squash_merge,allow_merge_commit,allow_rebase_merge,delete_branch_on_merge}'
# → true / false / false / true
```

### Removing the review layer (ADR-0021)

Remove `.github/rulesets/review.json` from the repository first (the
declaration is the source of truth — ADR-0000-rulesets-declaration-in-repo),
commit that, then run `apply-rulesets.sh OWNER/REPO --delete-ruleset Review
[--dry-run]` — it refuses to run while `review.json` is still declared, so
the order above is enforced, not just recommended. It only handles the
standalone `Review` ruleset shape (this skill's own `review.json` layout);
`copilot_code_review` or `required_review_thread_resolution: true` bundled
into some *other* active branch ruleset needs manual removal via
`gh api repos/OWNER/REPO/rulesets/<id>` + a hand-built `PUT`
(`crates/rulesets-write-guard` denies this from a Claude session — pass
`RULESETS_WRITE_GUARD_BYPASS=1` if you're deliberately doing this by hand).

### CI gates

The two required checks (`CI passed`, `PR title`) should turn green on the
first PR. `CI passed` aggregates every job in `ci.yml` (`build`, `fmt`,
`lint`, `min-typst`) via `needs:` — check the individual job's own logs
(not the Ruleset context string) to diagnose a red PR.

If `Format check` fails with typstyle errors: run `just fmt` to auto-fix, commit, push.
If `Min Typst (X.Y.Z)` fails: the project uses features from a Typst version newer than
`__MIN_TYPST__`. Bump `--min-typst` and update `compiler` in `typst.toml`
(both are substituted into `ci.yml`'s `min-typst` job in sync by
`copy-files.sh`).

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
│   │       ├── ci.yml             required: CI passed (aggregate of build/
│   │       │                      fmt/lint/min-typst via needs:, ADR-591
│   │       │                      in tarotene/dotfiles)
│   │       ├── pr-title.yml       required: PR title (calls tarotene/dotfiles'
│   │       │                      composite action, ADR-0031/ADR-591)
│   │       ├── release.yml        release: tag + git-cliff + gh-release w/ PDFs
│   │       └── metrics-reminder.yml  maintenance: monthly CV metrics issue
│   ├── .githooks/{commit-msg,pre-commit,pre-push}
│   ├── Justfile                       build/verify/fmt/lint/commit-check/ci/release-notes
│   ├── renovate.json                  github-actions + typst/typst version tracking
│   ├── cliff.toml                     CalVer changelog (vYYYY.MM tags)
│   ├── .yamllint                      YAML style rules
│   └── .gitignore-snippet             dist/ + editor/OS (merge manually)
│   ├── .github/rulesets/            (declaration copied into the target repo —
│   │   │                             ADR-0000-rulesets-declaration-in-repo)
│   │   ├── security.json    shared with repo-governance-common: deletion + non_fast_forward
│   │   ├── quality.json     symlink to repo-governance-common: signatures + linear
│   │   │                    history + the fixed pair CI passed/PR title (ADR-591)
│   │   ├── workflow.json    shared with repo-governance-common: squash-only (core)
│   │   └── review.json      shared with repo-governance-common: Copilot code review +
│   │                        required thread resolution (opt-in, --with-review only)
├── scripts/
│   ├── seed.sh                    main orchestrator — copy-files.sh, setup-hooks.sh,
│   │                              apply-repo-settings.sh, then the generic
│   │                              apply-rulesets.sh (not part of this skill;
│   │                              home-manager deploys it to ~/.local/bin)
│   ├── copy-files.sh              template + .github/rulesets/*.json copy, placeholder
│   │                              substitution, verify_declaration safety check
│   ├── apply-repo-settings.sh     gh api PATCH repo merge settings
│   └── setup-hooks.sh             git config core.hooksPath
└── reference/
    ├── manual-steps.md            post-seed checklist (App install, signing, first PR)
    └── releasing.md               release runbook (CalVer, retrigger, git-cliff)
```
