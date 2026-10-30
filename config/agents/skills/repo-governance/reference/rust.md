# Profile: rust (Rust workspace repository)

The `rust` profile replicates the telepath reference implementation into any
Rust workspace repository. Bundle: `~/.claude/skills/repo-governance/rust/`.
`SKILL.md` has the shared procedure; this file has only what differs.

## What this profile adds (Step "What this Skill does", item 1 and 4)

Beyond the shared file set, it copies `release-plz.yml`,
`release-binaries.yml`, `release-reminder.yml`, `nav-docs.yml` + composite
action, and the config files `release-plz.toml`, `cog.toml`,
`rust-toolchain.toml`, `Justfile`.

- The `ci.yml` template includes a `docs` job (`API docs`) that calls
  `tarotene/dotfiles/.github/actions/docs-rust@main` — the strict
  (`-D warnings` + doctest) stack-standard API doc build, ADR-640 in
  tarotene/dotfiles — and lists it in `ci-passed.needs`. GitHub Pages deploy
  (`docs-pages.yml`) and the weekly external link check
  (`docs-linkcheck.yml`) are opt-in templates under `repo-governance-common`
  — see `github-audit-triage` for when to add them.
- The `ci.yml` template also has a `stack-stage` job
  (`tarotene/dotfiles/.github/actions/stack-stage@main`, ADR-1005 D8) that
  tells whether a PR is a middle stage of a stacked PR; the `msrv` job (p75 of 68 s
  and 127 s in the two real repositories — the only template job that reaches
  the 2-minute guideline) takes it in `needs:` and skips itself in a middle stage. To skip another job that grows
  past 2 minutes, give it the same `needs: stack-stage` and
  `if: ${{ !cancelled() && needs.stack-stage.outputs.intermediate != 'true' }}`.
- Browser flows (item 4): GitHub App creation, and — only for repositories that
  actually distribute on crates.io — the bootstrap publish and Trusted
  Publishing registration. Publishing is opt-in: the `release-plz.toml`
  template ships `publish = false`, so a seeded repository tags and releases on
  GitHub but does not touch crates.io until someone deliberately turns it on.

## Step 0: profile parameters

| Parameter | Flag | Example |
|-----------|------|---------|
| MSRV (short) | `--msrv` | `1.88` (default; used by `ci.yml`'s `msrv` job regardless of Renovate pin status) |
| MSRV (full) | `--msrv-full` | 適用先が実際に `Cargo.toml` の `rust-version` や CI で特定バージョンを固定している場合のみ渡す(例 `1.88.0`)。省略すると `renovate.json` の `constraints.rust` ブロックと対応する保護用 packageRule は丸ごと省略される(#222 — `dtolnay/rust-toolchain@stable` のようなチャンネル名運用に架空の MSRV pin を作り込まない) |
| Canonical crate | `--canonical-crate` | `my-lib-core` — the crate that owns the git tag |
| CLI crate | `--cli-crate` | `my-cli` — the excluded crate under `tools/` |
| Firmware? | (no flag — manual) | `ci.yml`'s `firmware` job is always copied; delete it (and its entry in `ci-passed`'s `needs:`) manually if the project has no embedded firmware — see `rust/reference/manual-steps.md`. ADR-591 removed the old `--with-firmware` flag: required contexts no longer vary per job, so there is nothing left for the flag to gate |

Prerequisites beyond the shared ones:

```
command -v just
```

## Steps 1–2: commands

`<profile flags>` = `--canonical-crate CANONICAL --cli-crate CLI`:

```bash
~/.claude/skills/repo-governance/rust/scripts/seed.sh \
  --owner OWNER --repo REPO \
  --canonical-crate CANONICAL --cli-crate CLI \
  --dest /path/to/repo \
  --dry-run
```

## Step 3: ADJUST checklist (profile rows)

| File | What to update |
|------|----------------|
| `.github/workflows/ci.yml` — `host` job | `PATTERNS` regex — replace `telepath-(wire\|server\|...)` with your crate names |
| `.github/workflows/ci.yml` — `tools` job | `PATTERNS` regex; feature flags in `clippy-tools` and `mcp-test` Justfile recipes |
| `.github/workflows/ci.yml` — `msrv` job | `PATTERNS` regex — all workspace + excluded crate paths. Keep `needs: stack-stage` and the `if:` (ADR-1005 D8); `stack-stage` itself stays in `ci-passed`'s `needs:` |
| `.github/workflows/ci.yml` — `firmware` job | Chip name, target triple, example path. **Delete this whole job** (and its entry in `ci-passed`'s `needs:`) if no embedded firmware |
| `.github/workflows/release-plz.yml` | `host-pty-server` git-only package name; additional excluded crates in TREE_PAYLOAD |
| `.github/workflows/release-binaries.yml` | License file names (`LICENSE-MIT`, `LICENSE-APACHE`), README path |
| `.github/workflows/release-reminder.yml` | AGENTS.md anchor URL |
| `renovate.json` | `cargo.managerFilePatterns` — add your excluded crate paths; adjust embedded HAL package list |
| `release-plz.toml` | `[[package]]` entries — add your workspace crates, remove `host-pty-server` if not applicable |
| `Justfile` | Smoke test assertions in `host-pty-smoke`; feature combos in `clippy-tools` and `mcp-test` |

**`pr-title.yml`**: `tarotene/dotfiles/.github/actions/pr-title` re-verifies the
context match at runtime on every PR via `rulesets-context-check` (every
declared and live `required_status_checks` context) and `workflow-naming-check`
(both in `tarotene/dotfiles`' `crates/pr-contract-check`) (the `ci.yml`
`needs:` coverage and `name:` casing basis, ADR-591).

## Step 4: manual steps (browser flows)

Follow `rust/reference/manual-steps.md` (under `~/.claude/skills/repo-governance/`) for:

1. **GitHub App** — install the existing shared releaser App on this
   repository (do not create a new one — see
   `repo-governance-common/reference/releaser-app.md`), set
   `RELEASER_APP_CLIENT_ID` and `RELEASER_APP_PRIVATE_KEY` as repo secrets.
2. **crates.io publishing (opt-in, skip by default)** — only for repositories
   that distribute crates on crates.io. In this order: bootstrap first publish
   with a one-time `publish-new` token, register the Trusted Publishing entry
   (owner/repo/workflow=`release-plz.yml`), *then* drop `publish = false` from
   `release-plz.toml`. Out of order, every release run fails and a green CI run
   hides it (tarotene/dotfiles#659).

Short version of the secrets:
```
gh secret set RELEASER_APP_CLIENT_ID --repo OWNER/REPO --body "<client-id>"
gh secret set RELEASER_APP_PRIVATE_KEY --repo OWNER/REPO --body "$(cat key.pem)"
```

Release runbook (retrigger, recovery, Trusted Publishing): `rust/reference/releasing.md`.

## Step 5: verification (profile checks)

```bash
# Try a Conventional Commit (should pass)
cd /path/to/repo && echo "feat: test" | just commit-check /dev/stdin
```

CI gates: `CI passed` aggregates `fmt`, `host`, `msrv`, `firmware`, `tools`
(and the `docs` and `stack-stage` jobs) via `needs:`.

## Template structure reference

```
~/.claude/skills/repo-governance/rust/
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
│   ├── .github/rulesets/            (quality.json is a symlink to repo-governance-common)
│   ├── .githooks/{commit-msg,pre-commit,pre-push}
│   ├── renovate.json  release-plz.toml  cog.toml
│   └── rust-toolchain.toml  Justfile  .gitignore-snippet
├── scripts/                      seed.sh, copy-files.sh (+ the shared two)
└── reference/
    ├── releasing.md      release runbook (retrigger, recovery, Trusted Publishing)
    └── manual-steps.md   browser-flow checklist (App creation, crates.io setup)
```
