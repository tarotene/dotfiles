# Profile: astro-site (Astro site repository)

The `astro-site` profile replicates the Biome/Vitest CI, cog.toml, Renovate and
release-please set-up into any Astro site repository. Bundle:
`~/.claude/skills/repo-governance/astro-site/`. `SKILL.md` has the shared
procedure; this file has only what differs. For an explanation of why each
layer was chosen and how to migrate from an existing setup, see
`astro-site/reference/migration-guide.md` in the bundle.

## What this profile adds (Step "What this Skill does", item 1 and 4)

Beyond the shared file set, it copies `release-please.yml`, `nav-docs.yml` +
composite action, and the config files `biome.json`, `cog.toml`,
`release-please-config.json`, `.release-please-manifest.json`,
`vitest.config.ts`, plus the merge snippets `mise.toml-snippet` and
`package.scripts.jsonc-snippet`. The Quality ruleset (`quality.json`) is this
profile's own file; it differs from the shared one in `bypass_actors` only.

- No API-docs job is seeded: `github-audit`'s `docs` domain (ADR-640 in
  tarotene/dotfiles) is `not-applicable` for an Astro site — the
  site/document is itself the build artifact. If the repo is also a TypeScript
  library (its `package.json` declares `exports`/`main`/`types`), the audit
  will ask for `tarotene/dotfiles/.github/actions/docs-typescript@main` in
  `ci.yml`.
- This template has no job whose measured p75 is over 2 minutes (`Build` has a
  p75 of about 95 s in a real site repository), so it carries no `stack-stage`
  job (see "Heavy jobs and stacked PRs" in `SKILL.md`).
- Browser flows (item 4): GitHub Pages setup, optional GitHub App for
  release-please.

## Step 0: profile parameters

| Parameter | Flag | Example |
|-----------|------|---------|
| npm package name | `--package-name` | `my-astro-site` |
| Current version | `--package-version` | `0.1.0` (from package.json) |
| Node.js version | `--node-version` | `22` (default) |
| Astro base path | `--site-base` | `/my-astro-site` (docs only, optional) |
| Pages URL | `--pages-url` | `https://owner.github.io/my-astro-site/` (docs only, optional) |

(Example values for the common parameters in this profile: owner `tarotene`,
repo `my-astro-site`, `--dest /home/user/src/my-astro-site`.)

Prerequisites beyond the shared ones:

```
command -v npm
mise install  # (or ensure cog is on PATH)
```

## Steps 1–2: commands

`<profile flags>` = `--package-name NAME` (add the optional flags above as needed):

```bash
~/.claude/skills/repo-governance/astro-site/scripts/seed.sh \
  --owner OWNER --repo REPO \
  --package-name NAME --dest /path/to/repo \
  --dry-run
```

## Step 3: ADJUST checklist (profile rows)

| File | What to update |
|------|----------------|
| `package.json` | Merge scripts/devDependencies snippet (shown by copy-files.sh); run `npm install` |
| `mise.toml` | Add `cocogitto = "latest"` to `[tools]`; run `mise install` |
| `.github/workflows/ci.yml` | If project has NO MDX content-lint scripts, remove the `content-lint` job AND its entry in `ci-passed`'s `needs:` (they are a pair) |
| `release-please-config.json` | Verify `package-name` is correct |
| `.release-please-manifest.json` | Verify version matches current `package.json` |
| `renovate.json` | Adjust `packageRules` grouping for your actual dependencies |
| `biome.json` | Check `files.includes` globs match your TS/CSS paths; **never add `.mdx` or `.astro`** |

The four individual job `name:` values in `ci.yml` (`Format & Lint (Biome)`,
`Content lint`, `Unit tests`, `Build`) are display labels and no longer need to
match anything in `.github/rulesets/quality.json` (key invariant in `SKILL.md`,
ADR-591). If you remove `"content-lint"` (no MDX scripts), remove it from
`ci.yml` (the job) and from `ci-passed`'s `needs:` in the same file.

## Step 4: manual steps (browser flows)

Follow `astro-site/reference/manual-steps.md` (under `~/.claude/skills/repo-governance/`) for:

1. **Enable GitHub Pages** — Settings → Pages → Source: GitHub Actions.
2. **Verify Rulesets** — `gh api` commands to confirm contexts match.
3. **First PR walkthrough** — all 4 CI jobs should turn green.
4. **(Optional) GitHub App** — for release PRs that trigger full CI.

Release-please runbook: `astro-site/reference/releasing.md`.

## Step 5: verification (profile checks)

```bash
# Try a Conventional Commit (should pass)
echo "feat: test" | cog verify -

# Run full local quality gate
npm run ci:biome
npm run check
npm test
npm run build
```

CI gates: `CI passed` aggregates `biome`, `content-lint`, `unit-tests`, `build`
via `needs:`.

## Template structure reference

```
~/.claude/skills/repo-governance/astro-site/
├── templates/
│   ├── biome.json                        Biome formatter + linter config
│   ├── cog.toml                          cocogitto Conventional Commits config
│   ├── renovate.json                     Renovate dependency update config
│   ├── release-please-config.json        release-please config (node type)
│   ├── .release-please-manifest.json     initial version manifest
│   ├── vitest.config.ts                  Vitest test runner config
│   ├── mise.toml-snippet                 node + cocogitto merge snippet
│   ├── package.scripts.jsonc-snippet     scripts/devDeps/private/engines merge snippet
│   ├── .github/
│   │   ├── CODEOWNERS                    * @__OWNER__
│   │   ├── rulesets/quality.json         astro-specific (bypass_actors only)
│   │   └── workflows/
│   │       ├── ci.yml                    required: CI passed (aggregate of biome/
│   │       │                             content-lint/unit-tests/build via
│   │       │                             needs:, ADR-591 in tarotene/dotfiles)
│   │       ├── release-please.yml        automated CHANGELOG + Release
│   │       └── pr-title.yml              required: PR title (calls tarotene/dotfiles'
│   │                                     composite action, ADR-0031/ADR-591)
│   └── .githooks/
│       ├── commit-msg                    cog verify (Conventional Commits)
│       ├── pre-commit                    biome check --staged (fast)
│       └── pre-push                      npm run check + npm test
├── scripts/                              seed.sh, copy-files.sh (+ the shared two)
└── reference/
    ├── releasing.md                      release-please runbook
    ├── manual-steps.md                   browser-flow checklist
    └── migration-guide.md                ★ how to migrate from minimal/ESLint/Prettier
```

## Placeholder reference

| Placeholder | Meaning | Example |
|---|---|---|
| `__OWNER__` | GitHub owner login | `tarotene` |
| `__REPO__` | Repository name | `my-astro-site` |
| `__DEFAULT_BRANCH__` | Default branch | `main` |
| `__NODE_VERSION__` | Node.js version | `22` |
| `__PACKAGE_NAME__` | npm package name | `my-astro-site` |
| `__PACKAGE_VERSION__` | Current version | `0.1.0` |
| `__SITE_BASE__` | Astro base path (docs only) | `/my-astro-site` |
| `__PAGES_URL__` | Deployed Pages URL (docs only) | `https://tarotene.github.io/my-astro-site/` |
