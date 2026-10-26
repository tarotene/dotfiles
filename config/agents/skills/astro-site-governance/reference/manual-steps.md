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
# → CI passed
# → PR title

# Merge settings:
gh api repos/OWNER/REPO --jq '{allow_squash_merge, allow_merge_commit, allow_rebase_merge, delete_branch_on_merge}'
# → {"allow_squash_merge":true,"allow_merge_commit":false,"allow_rebase_merge":false,"delete_branch_on_merge":true}
```

**Key invariant:** The `name:` field of each job in
`templates/.github/workflows/ci.yml` **must exactly match** the entries in
`ci-passed`'s `needs:` (job *id*, not `name:`) — the Ruleset only ever
requires `CI passed`/`PR title` (ADR-591), so individual job `name:`
values (`Format & Lint (Biome)`, `Content lint`, `Unit tests`, `Build`) are
display labels and no longer need to match anything in `.github/rulesets/
quality.json`. `workflow-naming-check` (tarotene/dotfiles, a required
check on every PR) verifies this `needs:` coverage automatically.

**`pr-title.yml`** has no `workflow_call` concatenation to worry about
(ADR-591 replaced the old reusable-workflow form with a composite action,
docs/adr/591-ci-workflow-naming.md D3 in tarotene/dotfiles) — the reported
check context is simply this job's own `name: PR title`, pinned by the
`repo-governance-common/templates/.github/workflows/pr-title.yml` template
(this skill's copy is a symlink to it).

### Adapting for projects without MDX content lint

If your Astro project does NOT have custom MDX content-lint scripts
(no `npm run check` beyond `astro check`):

1. Remove the `content-lint` job from `ci.yml`.
2. Remove its entry (`content-lint`) from `ci-passed`'s `needs:` in the
   same file.
3. Commit, then re-verify (`workflow-naming-check` will flag a missed
   `needs:` entry if you forget step 2).

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

1. `CI passed` should turn green (this aggregates `biome`, `content-lint`
   — if present — `unit-tests`, and `build` via `needs:`; check the
   individual job's own logs, not this context, to diagnose a red PR).
2. `PR title` should turn green.

If `context not found` appears in the Quality Ruleset, `ci-passed`'s
`needs:` in `ci.yml` is probably missing an entry for a job you added, or
`quality.json` doesn't match the canonical `CI passed`/`PR title` pair —
`workflow-naming-check` (a required check on every PR) flags both cases.

---

## 5. (Optional) GitHub App for release-please

See `reference/releasing.md` for when this is worth doing and how.

Short version: with `GITHUB_TOKEN`, Release PRs opened by release-please
do NOT trigger CI automatically. The admin `bypass_actors` entry in
`quality.json` lets the maintainer merge them anyway. For a project where
you want Release PRs to run full CI, create a GitHub App.
