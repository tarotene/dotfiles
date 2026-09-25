---
name: rust-repo-governance
description: Bootstrap or replicate battle-tested GitHub governance (Security/Quality/Workflow core Rulesets always applied, plus an opt-in Review ruleset for Copilot code review + required conversation resolution — ADR-0021 in tarotene/dotfiles, merge-base-diff CI, release-plz with OIDC Trusted Publishing, Renovate MSRV-safe config, git hooks, Justfile) from the telepath reference implementation into any Rust workspace repository. Use when asked to "撒く", "bootstrap governance", "apply rulesets", "apply GitHub settings", "set up release-plz", "replicate telepath's CI setup", "seed CI to a new Rust repo", "rulesets / release / renovate をまとめて適用", or "telepath の GitHub 設定を別リポジトリに持っていく".
---

## What this Skill does

1. Copies parameterised CI/CD templates (9 workflows, including a
   per-file language-mixing check — `lang-mix.yml` — + composite action + CODEOWNERS)
   and config files (renovate.json, release-plz.toml, cog.toml, rust-toolchain.toml, Justfile, git hooks,
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
   it back out of an already-governed repository with `--remove-review`.
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
| MSRV (short) | `--msrv` | `1.88` (default; used by `msrv.yml` regardless of Renovate pin status) |
| MSRV (full) | `--msrv-full` | 適用先が実際に `Cargo.toml` の `rust-version` や CI で特定バージョンを固定している場合のみ渡す(例 `1.88.0`)。省略すると `renovate.json` の `constraints.rust` ブロックと対応する保護用 packageRule は丸ごと省略される(#222 — `dtolnay/rust-toolchain@stable` のようなチャンネル名運用に架空の MSRV pin を作り込まない) |
| Canonical crate | `--canonical-crate` | `my-lib-core` — the crate that owns the git tag |
| CLI crate | `--cli-crate` | `my-cli` — the excluded crate under `tools/` |
| Target repo path | `--dest` | `/home/user/src/my-lib` |
| Firmware? | `--with-firmware` | pass flag if project has embedded firmware |
| Review layer? | `--with-review` | pass flag to also apply the Review ruleset (Copilot code review + required conversation resolution — ADR-0021). Ask whether the repository is past its early-development phase before defaulting this on. |

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
| `.github/workflows/pr-title.yml` | Nothing to adjust — calls tarotene/dotfiles' reusable workflow (ADR-0031); the reported check context is fixed (see the Exception below), no manual confirmation needed |

**Key invariant**: The `name:` field of each workflow job **must exactly match**
the `context` string in `rulesets/quality.json`. The `__MSRV__` and `__CLI_CRATE__`
placeholders are replaced in both places simultaneously by `seed.sh`, preserving
this match. But if you rename a job manually, update the Ruleset context too.

**Exception: `pr-title.yml`.** It has no local job `name:` of its own — it
calls tarotene/dotfiles' reusable workflow via `workflow_call`, and the
reported check context is GitHub's own concatenation of the **caller
job's** `name:` and the called job's `name:` ("PR Title / PR title").
The `repo-governance-common/templates/.github/workflows/pr-title.yml`
template (this skill's copy is a symlink to it) pins the caller job's
`name: PR Title`, so this string is a fixed value, not a best-effort
guess — no manual confirmation against the Checks tab is needed. (An
earlier version of this note said to confirm the string on the first real
PR; that assumed the wrong half of the concatenation was the fixed one and
missed that #337's rollout had seeded a context the then-unnamed caller
job could never satisfy — ADR-0031's 2026-09-26 Amendment.)
`.github/workflows/pr-title.yml` (dotfiles' reusable workflow) re-verifies
the match at runtime on every PR via `scripts/pr-title-context-check`.

---

## Step 4: Manual steps (browser flows)

Follow `./reference/manual-steps.md` (in this Skill directory) for:

1. **GitHub App** — install the existing shared releaser App on this
   repository (do not create a new one — see
   `repo-governance-common/reference/releaser-app.md`), set
   `RELEASER_APP_ID` and `RELEASER_APP_PRIVATE_KEY` as repo secrets.
2. **crates.io Trusted Publishing** — register each published crate with
   owner/repo/workflow=`release-plz.yml`.
3. **Bootstrap first publish** — one-time `publish-new` token for crates that
   don't yet exist on crates.io.

Short version of the secrets:
```
gh secret set RELEASER_APP_ID --repo OWNER/REPO --body "<numeric-id>"
gh secret set RELEASER_APP_PRIVATE_KEY --repo OWNER/REPO --body "$(cat key.pem)"
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
# → Security, Quality, Workflow (+ Review if seeded with --with-review)

# Required checks should be registered in Quality Ruleset:
gh api repos/OWNER/REPO/rulesets --jq '.[] | select(.name=="Quality") | .rules[] | select(.type=="required_status_checks") | .parameters.required_status_checks[].context'

# Repo merge settings:
gh api repos/OWNER/REPO --jq '{allow_squash_merge, allow_merge_commit, allow_rebase_merge, delete_branch_on_merge}'
# → true / false / false / true
```

### Removing the review layer (ADR-0021)

`apply-rulesets.sh --owner OWNER --repo REPO --remove-review [--dry-run]`
handles the two layouts it can meet:

- A standalone `Review` ruleset (this skill's own `review.json` layout) — it
  is deleted outright.
- `copilot_code_review` or `required_review_thread_resolution: true` bundled
  into some *other* active branch ruleset — the script fetches that
  ruleset's full detail, strips the rule / resets the parameter, and `PUT`s
  the filtered payload back (the update endpoint takes the same shape as
  create, not a partial patch).

An irregular layout the script won't recognize (e.g. a hand-edited ruleset
with a different name and the review layer folded into unrelated
parameters) needs manual removal: `gh api repos/OWNER/REPO/rulesets/<id>`
to inspect, then a hand-built `PUT` with `copilot_code_review` dropped from
`rules` and `required_review_thread_resolution` set to `false` on every
`pull_request` rule.

### CI gates

All 6 (or 5 without firmware) required checks should turn green on the first PR.
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
│   │       ├── release-nudge.yml  maintenance: weekly stale PR nudge
│   │       └── pr-title.yml       required: PR Title / PR title (calls tarotene/dotfiles' reusable workflow, ADR-0031)
│   ├── .githooks/{commit-msg,pre-commit,pre-push}
│   ├── renovate.json  release-plz.toml  cog.toml
│   └── rust-toolchain.toml  Justfile  .gitignore-snippet
├── rulesets/                        (core layer applied by default; Review is opt-in — ADR-0021)
│   ├── security.json    deletion + non_fast_forward
│   ├── quality.json     signatures + linear history + 6 status checks
│   ├── workflow.json    squash-only (core; thread resolution NOT required here)
│   └── review.json      Copilot code review + required thread resolution (opt-in addin)
├── scripts/
│   ├── seed.sh           main orchestrator
│   ├── copy-files.sh     template copy + placeholder substitution
│   ├── apply-rulesets.sh gh api POST the core 3 Rulesets (+ Review with --with-review;
│   │                     --remove-review strips the review layer back out)
│   ├── apply-repo-settings.sh  gh api PATCH repo merge settings
│   └── setup-hooks.sh    git config core.hooksPath
└── reference/
    ├── releasing.md      release runbook (retrigger, recovery, Trusted Publishing)
    └── manual-steps.md   browser-flow checklist (App creation, crates.io setup)
```
