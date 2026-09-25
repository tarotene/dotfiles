# Manual Steps Checklist

These are the steps that `seed.sh` cannot automate because they require
browser flows, GitHub UI actions, or one-time operations.

---

## 1. Enable GitHub Pages

After the first push with the `deploy.yml` workflow:

1. Go to **Settings → Pages** in your GitHub repository.
2. Under **Build and deployment**, set **Source** to **GitHub Actions**.
3. Save. The next `main` push will deploy the site.

Note: the deploy workflow uses GitHub's OIDC-based Pages deployment
(`actions/deploy-pages@v4`), which requires the Pages source to be set to
"GitHub Actions" rather than a branch.

---

## 2. Verify Rulesets

After running `seed.sh` or `apply-rulesets.sh`:

```bash
# All three rulesets should appear:
gh api repos/OWNER/REPO/rulesets --jq '.[].name'
# → Security
# → Quality
# → Workflow

# Required status check contexts in Quality:
gh api repos/OWNER/REPO/rulesets \
  --jq '.[] | select(.name=="Quality") | .rules[] | select(.type=="required_status_checks") | .parameters.required_status_checks[].context'
# → Format & Lint (Biome)
# → Content lint
# → Unit tests
# → Build
# → PR Title / PR title   (fixed string — see the Exception note below)

# Merge settings:
gh api repos/OWNER/REPO --jq '{allow_squash_merge, allow_merge_commit, allow_rebase_merge, delete_branch_on_merge}'
# → {"allow_squash_merge":true,"allow_merge_commit":false,"allow_rebase_merge":false,"delete_branch_on_merge":true}
```

**Key invariant:** The `name:` strings in `templates/.github/workflows/ci.yml`
and the `context` strings in `rulesets/quality.json` must be byte-identical.
The four static strings are:
- `"Format & Lint (Biome)"`
- `"Content lint"`
- `"Unit tests"`
- `"Build"`

If you rename a CI job, update the Ruleset context string in the same edit.

**Exception: `pr-title.yml`.** It has no local job `name:` of its own — it
calls tarotene/dotfiles' reusable workflow via `workflow_call`, and the
reported check context is GitHub's own concatenation of the **caller
job's** `name:` and the called job's `name:` ("PR Title / PR title"). The
`repo-governance-common/templates/.github/workflows/pr-title.yml`
template (this skill's copy is a symlink to it) pins the caller job's
`name: PR Title`, so this string is a fixed value, not a best-effort
guess — no manual confirmation against the Checks tab is needed. (An
earlier version of this note said to confirm the string on the first real
PR; that assumed the wrong half of the concatenation was fixed and missed
that #337's rollout had seeded a context the then-unnamed caller job
could never satisfy — ADR-0031's 2026-09-26 Amendment.)
`.github/workflows/pr-title.yml` (dotfiles' reusable workflow)
re-verifies the match at runtime on every PR via
`scripts/pr-title-context-check`.

### Adapting for projects without MDX content lint

If your Astro project does NOT have custom MDX content-lint scripts
(no `npm run check` beyond `astro check`):

1. Remove the `content-lint` job from `ci.yml`.
2. Remove the `"Content lint"` entry from `rulesets/quality.json`.
3. Re-run `apply-rulesets.sh` to update the Ruleset (delete the old one first
   if it already exists).

---

## 3. Verify git hooks

After running `setup-hooks.sh` (or `git config --local core.hooksPath .githooks`):

```bash
git -C /path/to/repo config --local core.hooksPath
# → .githooks

# Hooks should be executable:
ls -la /path/to/repo/.githooks/
# → commit-msg, pre-commit, pre-push (all executable)
```

Also verify `mise install` has been run so `cog` is on PATH:

```bash
command -v cog && cog --version
```

---

## 4. First PR walkthrough

After all files are committed and pushed, open a test PR to verify CI:

1. `Format & Lint (Biome)` should turn green.
2. `Content lint` should turn green (or is absent if you removed it).
3. `Unit tests` should turn green.
4. `Build` should turn green.

If `context not found` appears in the Quality Ruleset, the job `name:` in
`ci.yml` does not match the context string in `quality.json`. Fix both in
the same commit.

---

## 5. (Optional) GitHub App for release-please

See `reference/releasing.md` for when this is worth doing and how.

Short version: with `GITHUB_TOKEN`, Release PRs opened by release-please
do NOT trigger CI automatically. The admin `bypass_actors` entry in
`quality.json` lets the maintainer merge them anyway. For a project where
you want Release PRs to run full CI, create a GitHub App.
