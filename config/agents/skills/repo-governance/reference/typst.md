# Profile: typst (Typst / document repository)

The `typst` profile covers the same governance goals as the `rust` profile
(signing, linear history, squash-merge, a `CI passed`/`PR title`
required-check pair, dependency automation) using Typst-appropriate tooling
instead of Cargo/clippy. Bundle: `~/.claude/skills/repo-governance/typst/`.
`SKILL.md` has the shared procedure; this file has only what differs.

## What this profile adds (Step "What this Skill does", item 1 and 4)

Beyond the shared file set, it copies `release.yml`, `metrics-reminder.yml`,
`nav-docs.yml` + composite action, and the config files `cliff.toml`,
`.yamllint`, `Justfile`.

- No API-docs job is seeded: `github-audit`'s `docs` domain (ADR-640 in
  tarotene/dotfiles) is `not-applicable` for a Typst/document repository — the
  site/document is itself the build artifact. If the repo is also a TypeScript
  library (its `package.json` declares `exports`/`main`/`types`), the audit
  will ask for `tarotene/dotfiles/.github/actions/docs-typescript@main` in
  `ci.yml`.
- This template has no job whose measured p75 is over 2 minutes (`Build` of the
  example slides has a p75 of about 11 s in the template repository), so it
  carries no `stack-stage` job (see "Heavy jobs and stacked PRs" in `SKILL.md`).
- Browser flows (item 4): Mend Renovate App installation, commit-signing
  setup, first-PR green-check.

## Step 0: profile parameters

| Parameter | Flag | Example |
|-----------|------|---------|
| Typst version pin (CI) | `--typst-version` | `0.14.2` (default) |
| Minimum Typst version | `--min-typst` | `0.14.0` (default) |
| ATS contact email | `--ats-email` | `you@example.com` |

(Example values for the common parameters in this profile: owner `tarotene`,
repo `cv`, `--dest /home/user/src/cv`.)

Prerequisites beyond the shared ones:

```
command -v just
```

## Steps 1–2: commands

`<profile flags>` = `--ats-email EMAIL` (add `--typst-version` / `--min-typst`
only to override the defaults):

```bash
~/.claude/skills/repo-governance/typst/scripts/seed.sh \
  --owner OWNER --repo REPO \
  --ats-email EMAIL \
  --dest /path/to/repo \
  --dry-run
```

## Step 3: ADJUST checklist (profile rows)

| File | What to update |
|------|----------------|
| `Justfile` | `COMPILE_FLAGS`, `SRC_*`, `OUT_*`, and `verify` DOCS table |
| `.github/workflows/ci.yml` — `build` job | `PATTERNS` regex — match your source directory layout |
| `.github/workflows/ci.yml` — `fmt` job | `PATTERNS` regex; `inputs:` path passed to typstyle-action |
| `.github/workflows/ci.yml` — `min-typst` job | `PATTERNS` regex |
| `.github/workflows/release.yml` | PDF filenames in the `files:` block |
| `.github/workflows/metrics-reminder.yml` | Issue body; **delete this file** if not a CV project |
| `cliff.toml` | `tag_pattern` if your CalVer tag scheme differs |
| `renovate.json` | Typst toolchain group, custom manager — scheduling/grouping/automerge live in the shared preset it extends |

## Step 4: manual steps (browser flows)

Follow `typst/reference/manual-steps.md` (under `~/.claude/skills/repo-governance/`) for:

1. **Renovate** — nothing to do per repository; the App is installed account-wide (see `repo-governance-common/reference/renovate-app.md`).
2. **Commit signing** — SSH or GPG signing for `required_signatures` Ruleset.
3. **First PR** — push the bootstrapped branch, open PR, wait for both
   required checks (`CI passed`, `PR title`) to go green.
4. **Apply Rulesets + settings** — run apply-rulesets.sh and apply-repo-settings.sh
   after the first PR is green.

Release runbook (CalVer, retrigger, git-cliff): `typst/reference/releasing.md`.

## Step 5: verification (profile checks)

```bash
# Test commit-msg hook
echo "feat: test"   | just commit-check /dev/stdin  # should pass
echo "bad message"  | just commit-check /dev/stdin  # should exit 1

# Build + ATS gate
just build && just verify
```

CI gates: `CI passed` aggregates `build`, `fmt`, `lint`, `min-typst` via `needs:`.

If `Format check` fails with typstyle errors: run `just fmt` to auto-fix, commit, push.
If `Min Typst (X.Y.Z)` fails: the project uses features from a Typst version newer than
`__MIN_TYPST__`. Bump `--min-typst` and update `compiler` in `typst.toml`
(both are substituted into `ci.yml`'s `min-typst` job in sync by
`copy-files.sh`).

## Template structure reference

```
~/.claude/skills/repo-governance/typst/
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
│   ├── .github/rulesets/            (quality.json is a symlink to repo-governance-common)
│   ├── .githooks/{commit-msg,pre-commit,pre-push}
│   ├── Justfile                       build/verify/fmt/lint/commit-check/ci/release-notes
│   ├── renovate.json                  github-actions + typst/typst version tracking
│   ├── cliff.toml                     CalVer changelog (vYYYY.MM tags)
│   ├── .yamllint                      YAML style rules
│   └── .gitignore-snippet             dist/ + editor/OS (merge manually)
├── scripts/                           seed.sh, copy-files.sh (+ the shared two)
└── reference/
    ├── manual-steps.md            post-seed checklist (App install, signing, first PR)
    └── releasing.md               release runbook (CalVer, retrigger, git-cliff)
```
