---
name: rust-repo-governance
description: Bootstrap or replicate battle-tested GitHub governance (Security/Quality/Workflow core Rulesets always applied, plus an opt-in Review ruleset for Copilot code review + required conversation resolution — ADR-0021 in tarotene/dotfiles, merge-base-diff CI, release-plz with OIDC Trusted Publishing, Renovate MSRV-safe config, git hooks, Justfile) from the telepath reference implementation into any Rust workspace repository. Use when asked to "撒く", "bootstrap governance", "apply rulesets", "apply GitHub settings", "set up release-plz", "replicate telepath's CI setup", "seed CI to a new Rust repo", "rulesets / release / renovate をまとめて適用", or "telepath の GitHub 設定を別リポジトリに持っていく".
---

## What this Skill does

1. Copies parameterised CI/CD templates (a consolidated `ci.yml` — every PR
   gate job plus the `CI passed` aggregate required check, ADR-591 in
   tarotene/dotfiles — + `pr-title.yml`, `release-plz.yml`,
   `release-binaries.yml`, `release-reminder.yml`, a per-file
   language-mixing check `lang-mix.yml`, `nav-docs.yml` + composite action,
   an ADR-number check `adr-number.yml` — ADR-380 in tarotene/dotfiles —
   + CODEOWNERS)
   and config files (renovate.json, release-plz.toml, cog.toml, rust-toolchain.toml, Justfile, git hooks,
   AGENTS.md/CLAUDE.md routing skeleton — ADR-0016 in tarotene/dotfiles)
   into the target repository, substituting `__PLACEHOLDER__` values for your repo's specifics.
   The `ci.yml` template includes a `docs` job (`API docs`) that calls
   `tarotene/dotfiles/.github/actions/docs-rust@main` — the strict
   (`-D warnings` + doctest) stack-standard API doc build, ADR-640 in
   tarotene/dotfiles — and lists it in `ci-passed.needs`. GitHub Pages deploy
   (`docs-pages.yml`) and the weekly external link check
   (`docs-linkcheck.yml`) are opt-in templates under `repo-governance-common`
   — see `github-audit-triage` for when to add them.
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
| MSRV (short) | `--msrv` | `1.88` (default; used by `ci.yml`'s `msrv` job regardless of Renovate pin status) |
| MSRV (full) | `--msrv-full` | 適用先が実際に `Cargo.toml` の `rust-version` や CI で特定バージョンを固定している場合のみ渡す(例 `1.88.0`)。省略すると `renovate.json` の `constraints.rust` ブロックと対応する保護用 packageRule は丸ごと省略される(#222 — `dtolnay/rust-toolchain@stable` のようなチャンネル名運用に架空の MSRV pin を作り込まない) |
| Canonical crate | `--canonical-crate` | `my-lib-core` — the crate that owns the git tag |
| CLI crate | `--cli-crate` | `my-cli` — the excluded crate under `tools/` |
| Target repo path | `--dest` | `/home/user/src/my-lib` |
| Firmware? | (no flag — manual) | `ci.yml`'s `firmware` job is always copied; delete it (and its entry in `ci-passed`'s `needs:`) manually if the project has no embedded firmware — see `reference/manual-steps.md`. ADR-591 removed the old `--with-firmware` flag: required contexts no longer vary per job, so there is nothing left for the flag to gate |
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

# Create Rulesets only (files already copied — the generic apply script,
# not part of this skill, reads OWNER/REPO's own .github/rulesets/*.json;
# ADR-503):
apply-rulesets.sh OWNER/REPO --unverified-contexts

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
| `.github/workflows/ci.yml` — `host` job | `PATTERNS` regex — replace `telepath-(wire\|server\|...)` with your crate names |
| `.github/workflows/ci.yml` — `tools` job | `PATTERNS` regex; feature flags in `clippy-tools` and `mcp-test` Justfile recipes |
| `.github/workflows/ci.yml` — `msrv` job | `PATTERNS` regex — all workspace + excluded crate paths |
| `.github/workflows/ci.yml` — `firmware` job | Chip name, target triple, example path. **Delete this whole job** (and its entry in `ci-passed`'s `needs:`) if no embedded firmware |
| `.github/workflows/release-plz.yml` | `host-pty-server` git-only package name; additional excluded crates in TREE_PAYLOAD |
| `.github/workflows/release-binaries.yml` | License file names (`LICENSE-MIT`, `LICENSE-APACHE`), README path |
| `.github/workflows/release-reminder.yml` | AGENTS.md anchor URL |
| `renovate.json` | `cargo.managerFilePatterns` — add your excluded crate paths; adjust embedded HAL package list |
| `release-plz.toml` | `[[package]]` entries — add your workspace crates, remove `host-pty-server` if not applicable |
| `Justfile` | Smoke test assertions in `host-pty-smoke`; feature combos in `clippy-tools` and `mcp-test` |
| `.github/workflows/pr-title.yml` | Nothing to adjust — calls tarotene/dotfiles' composite action (ADR-0031/ADR-591); the reported check context is simply this job's own `name: PR title`, no manual confirmation needed |
| `.github/workflows/close-linked-issues.yml` | Nothing to adjust — closes the issues a merged PR names with `Closes #N` even when the PR was stacked on a non-default base (GitHub only acts on the keyword for PRs targeting the default branch, tarotene/dotfiles#609); needs no secrets, only `issues: write` |
| `.github/zizmor.yml` | Nothing to adjust — `"tarotene/*": ref-pin` covers the composite action's symbolic-ref `uses:` (#491); zizmor's own blanket default (hash-pin) still applies to every other `uses:` |

**Key invariant**: The `name:` field of each workflow job in `ci.yml`
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
(this skill's copy is a symlink to it). `tarotene/dotfiles/.github/
actions/pr-title` re-verifies the match at runtime on every PR via
`scripts/rulesets-context-check` (every declared and live
`required_status_checks` context) and `scripts/workflow-naming-check`
(the `ci.yml` `needs:` coverage and `name:` casing basis, ADR-591).

---

## Step 4: Manual steps (browser flows)

Follow `./reference/manual-steps.md` (in this Skill directory) for:

1. **GitHub App** — install the existing shared releaser App on this
   repository (do not create a new one — see
   `repo-governance-common/reference/releaser-app.md`), set
   `RELEASER_APP_CLIENT_ID` and `RELEASER_APP_PRIVATE_KEY` as repo secrets.
2. **crates.io Trusted Publishing** — register each published crate with
   owner/repo/workflow=`release-plz.yml`.
3. **Bootstrap first publish** — one-time `publish-new` token for crates that
   don't yet exist on crates.io.

Short version of the secrets:
```
gh secret set RELEASER_APP_CLIENT_ID --repo OWNER/REPO --body "<client-id>"
gh secret set RELEASER_APP_PRIVATE_KEY --repo OWNER/REPO --body "$(cat key.pem)"
```

---

## Step 5: Verification

### Local sanity
```bash
# Validate Ruleset JSONs
jq -e . ~/.claude/skills/rust-repo-governance/templates/.github/rulesets/*.json

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

Remove `.github/rulesets/review.json` from the repository first (the
declaration is the source of truth — ADR-503),
commit that, then run:

```bash
apply-rulesets.sh OWNER/REPO --delete-ruleset Review [--dry-run]
```

This refuses to run while `review.json` is still declared, so the order
above is enforced, not just recommended. It only handles the standalone
`Review` ruleset shape (this skill's own `review.json` layout) — it does not
recognize `copilot_code_review` or `required_review_thread_resolution: true`
bundled into some *other* active branch ruleset. An irregular layout like
that needs manual removal: `gh api repos/OWNER/REPO/rulesets/<id>` to
inspect, then a hand-built `PUT` with `copilot_code_review` dropped from
`rules` and `required_review_thread_resolution` set to `false` on every
`pull_request` rule (`crates/rulesets-write-guard` denies a raw `gh api`
write to this endpoint from a Claude session — run it yourself, or pass
`RULESETS_WRITE_GUARD_BYPASS=1` if you're deliberately doing this by hand).

### CI gates

The two required checks (`CI passed`, `PR title`) should turn green on the
first PR. `CI passed` aggregates every job in `ci.yml` (`fmt`, `host`,
`msrv`, `firmware`, `tools`) via `needs:` — if one of those individual jobs
fails, `CI passed` fails with it, so check the individual job's own logs
(not the Ruleset context string) to diagnose a red PR.

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
│   │       ├── ci.yml             required: CI passed (aggregate of fmt/
│   │       │                      host/msrv/firmware/tools via needs:,
│   │       │                      ADR-591 in tarotene/dotfiles)
│   │       ├── release-plz.yml    release: tag + crates.io publish
│   │       ├── release-binaries.yml  release: 4-target binary builds
│   │       ├── release-reminder.yml  maintenance: weekly stale PR nudge
│   │       └── pr-title.yml       required: PR title (calls tarotene/dotfiles'
│   │                              composite action, ADR-0031/ADR-591)
│   ├── .github/rulesets/            (declaration copied into the target repo —
│   │   │                             ADR-503)
│   │   ├── security.json    shared with repo-governance-common: deletion + non_fast_forward
│   │   ├── quality.json     symlink to repo-governance-common: signatures + linear
│   │   │                    history + the fixed pair CI passed/PR title (ADR-591)
│   │   ├── workflow.json    shared with repo-governance-common: squash-only (core)
│   │   └── review.json      shared with repo-governance-common: Copilot code review +
│   │                        required thread resolution (opt-in, --with-review only)
│   ├── .githooks/{commit-msg,pre-commit,pre-push}
│   ├── renovate.json  release-plz.toml  cog.toml
│   └── rust-toolchain.toml  Justfile  .gitignore-snippet
├── scripts/
│   ├── seed.sh           main orchestrator — copy-files.sh, setup-hooks.sh,
│   │                     apply-repo-settings.sh, then the generic
│   │                     apply-rulesets.sh (not part of this skill;
│   │                     home-manager deploys it to ~/.local/bin)
│   ├── copy-files.sh     template + .github/rulesets/*.json copy, placeholder
│   │                     substitution, verify_declaration safety check
│   ├── apply-repo-settings.sh  gh api PATCH repo merge settings
│   └── setup-hooks.sh    git config core.hooksPath
└── reference/
    ├── releasing.md      release runbook (retrigger, recovery, Trusted Publishing)
    └── manual-steps.md   browser-flow checklist (App creation, crates.io setup)
```
