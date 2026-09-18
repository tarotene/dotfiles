# Migration Guide: From Minimal/ESLint/Prettier to Standard Astro Governance Stack

This guide describes how to migrate an Astro site repository from a minimal
setup (or from ESLint + Prettier) to the standard governance stack introduced
by the `astro-site-governance` Skill.

---

## Why migrate?

The minimal approach (no external linter, zero devDependencies, hand-written
quality scripts) is excellent for bootstrapping but has gaps:

| Gap | How this stack fills it |
|---|---|
| No code formatter | Biome formats `.ts`/`.js`/`.json`/`.css` consistently |
| No import-order enforcement | Biome's `organizeImports` assist |
| No automated dependency updates | Renovate with grouped PRs |
| No commit-convention enforcement | cocogitto validates Conventional Commits |
| No unit tests for lint/utility logic | Vitest tests key pure-TS functions |
| No CHANGELOG / release automation | release-please generates CHANGELOG + GitHub Releases |
| No branch protection | GitHub Rulesets (security/quality/workflow) |

**What does NOT change:** MDX content quality remains fully in the custom
prose-lint scripts (`check:abbr`, `check:style`). Biome does not touch `.mdx`
files — this is by design.

---

## The Biome ↔ MDX boundary (central invariant)

This is the most important design decision in this stack:

```
Biome owns → .ts / .js / .mjs / .json / .jsonc / .css
astro check + custom scripts own → .astro / .mdx
```

**Why `.astro` is excluded from Biome:** Biome v2.x parses only the frontmatter
script block (`---…---`) of Astro files. The template body (HTML/JSX) is
invisible to Biome. This means variables used in the template body but defined
in the frontmatter appear as "unused" to Biome — `--unsafe` would delete them.
`astro check` already handles TypeScript checking in Astro files correctly.

**Why `.mdx` is excluded from Biome:** Biome has no MDX support (as of v2.4;
check [biomejs.dev/internals/language-support](https://biomejs.dev/internals/language-support/)
for updates). All MDX quality is handled by the custom prose-lint scripts that
understand the MDX structure (frontmatter, code fences, math delimiters, JSX tags).

**Practical rule:** Never add `**/*.mdx` or `src/**/*.astro` to `biome.json`'s
`files.includes`. If you do, Biome will silently misprocess them.

---

## Migration steps

### 1. Install Biome

```bash
npm install --save-dev @biomejs/biome
```

Copy `biome.json` from this Skill's `templates/` to your project root, then
run the initial format pass:

```bash
npx biome format --write .
npx biome lint --write --unsafe .
npx biome check --write --unsafe .   # also applies organizeImports
```

Review the diff carefully. Import-reordering and template-literal conversions
are safe; verify there are no unintended changes.

Add the new scripts to `package.json`:

```json
"lint":         "biome lint .",
"lint:fix":     "biome lint --write .",
"format":       "biome format --write .",
"format:check": "biome format .",
"ci:biome":     "biome ci ."
```

Also add `"private": true` and `"engines": { "node": ">=22" }`.

**If migrating from ESLint:** run `npx @biomejs/biome migrate eslint --write`
to auto-convert `.eslintrc` rules to Biome equivalents.

**If migrating from Prettier:** run `npx @biomejs/biome migrate prettier --write`
to auto-convert `.prettierrc` settings.

### 2. Add Vitest unit tests

```bash
npm install --save-dev vitest @vitest/coverage-v8
```

Copy `vitest.config.ts` from this Skill's `templates/`. Create test files in
`scripts/` (co-located with the modules they test). Minimum recommended tests:

- Pure prose-extraction functions (`maskNonProseExceptMath`, `extractProse`,
  `getParagraph`, `extractFrontmatter`) — these are regression-sensitive because
  any masking bug silently corrupts the lint output.
- Data-registry integrity (`ABBREVIATION_KEYS` matches object keys,
  alphabetical order, proper-noun allowlist populated).

Add `"test": "vitest run"` to `package.json`.

### 3. Set up cocogitto (cog)

```bash
# via mise (recommended):
# edit mise.toml and add: cocogitto = "latest"
mise install

# or directly:
cargo install cocogitto
```

Copy `cog.toml` from this Skill's `templates/`. Adjust `branch_whitelist` if
your default branch is not `main`.

Copy the `.githooks/` directory. Configure the hook path:

```bash
git config --local core.hooksPath .githooks
```

### 4. Add Renovate

Copy `renovate.json` from this Skill's `templates/`. Adjust the `packageRules`
grouping to match your project's dependency structure (Astro/Tailwind/math
rendering are grouped by default; add or remove groups as appropriate).

Renovate is self-service — install the [Renovate GitHub App](https://github.com/apps/renovate)
on your repository. No CI workflow is needed.

### 5. Add release-please

Copy:
- `release-please-config.json` (set `package-name` to your project name)
- `.release-please-manifest.json` (set version to current `package.json` version)
- `.github/workflows/release-please.yml`

Add `"private": true` to `package.json` to prevent accidental `npm publish`.

### 6. Update CI

Copy `.github/workflows/ci.yml` from this Skill's `templates/`. The 4-job
structure replaces any existing single-job CI.

**Key invariant:** the `name:` field of each job must exactly match the
`context` string in `rulesets/quality.json`. These strings are static (no
placeholders) — they are "Format & Lint (Biome)", "Content lint", "Unit tests",
"Build". Do not rename jobs without updating the Ruleset.

If your project has **no custom content-lint scripts** (no `npm run check`
beyond `astro check`), remove the "Content lint" job AND its Ruleset entry.

### 7. Apply GitHub settings and Rulesets

Run the Skill seed script or apply manually:

```bash
# Dry-run first:
~/.claude/skills/astro-site-governance/scripts/seed.sh \
  --owner OWNER --repo REPO --package-name NAME --dest /path/to/repo --dry-run

# Then apply:
~/.claude/skills/astro-site-governance/scripts/seed.sh \
  --owner OWNER --repo REPO --package-name NAME --dest /path/to/repo
```

The seed script applies:
- 3 GitHub Rulesets (Security / Quality / Workflow)
- Repository merge settings (squash-only, delete-on-merge, auto-merge)
- Dependabot security alerts

---

## What changes in practice

| Before | After |
|---|---|
| Code format is inconsistent/manual | `npm run format` normalises everything |
| Import order varies | Biome `organizeImports` enforces consistency |
| Dependencies update manually | Renovate opens PRs weekly |
| Commit messages are unenforced | `commit-msg` hook rejects non-Conventional |
| No unit tests for lint logic | 36+ Vitest tests give regression safety |
| No CHANGELOG | release-please generates it from Conventional Commits |
| PRs can be merged without checks | Quality Ruleset requires all 4 jobs green |
| Any merge strategy allowed | Workflow Ruleset enforces squash-only |

---

## Rollback

If you need to revert:

```bash
# Remove Biome
npm uninstall @biomejs/biome
rm biome.json

# Remove Vitest
npm uninstall vitest @vitest/coverage-v8
rm vitest.config.ts

# Remove hooks
git config --local --unset core.hooksPath
rm -rf .githooks/

# Remove cog config
rm cog.toml

# Remove release-please
rm release-please-config.json .release-please-manifest.json
rm .github/workflows/release-please.yml

# Delete Rulesets via gh api
gh api repos/OWNER/REPO/rulesets --jq '.[].id' | while read id; do
  gh api -X DELETE "repos/OWNER/REPO/rulesets/$id"
done
```
