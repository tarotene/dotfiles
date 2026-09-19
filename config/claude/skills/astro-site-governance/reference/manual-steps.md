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
