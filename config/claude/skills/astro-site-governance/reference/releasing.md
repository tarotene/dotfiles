# Release Runbook: release-please

This document covers day-to-day release operations for the release-please setup.

## How releases work

1. **Conventional Commits on `main`** trigger release-please on every push.
2. release-please maintains an open "Release PR" that bumps `package.json`
   version and `CHANGELOG.md` based on accumulated commits.
3. **Merging the Release PR** creates a git tag (`v1.2.3`) and a GitHub Release.
4. The GitHub Pages deploy fires automatically on the post-merge push to `main`
   (the `deploy.yml` workflow is decoupled from releases).

## Commit types → version bumps

| Commit type | Version bump |
|---|---|
| `feat:` or `feat(scope):` | **minor** (until first 1.0.0 release if `bump-minor-pre-major: true`) |
| `fix:`, `docs:`, `perf:`, `refactor:` | **patch** |
| `feat!:` or `BREAKING CHANGE:` footer | **major** |
| `chore:`, `ci:`, `test:` | no bump (hidden from CHANGELOG) |

## Common operations

### Re-trigger after failed run

```bash
gh workflow run release-please.yml --repo OWNER/REPO
```

### Override the next version (rare)

If the automatic bump is wrong (e.g. you need to force a major release),
add a `Release-As: 2.0.0` footer to any commit on `main`. release-please
will use that version for the next Release PR.

```
feat(something): big change

Release-As: 2.0.0
```

### Close a stale Release PR

release-please reopens the PR on the next push. To discard accumulated changes:

```bash
gh pr close <number> --repo OWNER/REPO --comment "Superseded"
```

Then make a commit on `main` to trigger a fresh Release PR.

### Recover from a bad tag

**Never reuse a deleted tag.** If you need to roll back a release:

1. Delete the GitHub Release and tag via the UI.
2. Bump the patch version to skip the bad version:
   add a `Release-As: X.Y.Z` footer (one higher than the bad one) to a fix commit.

## GITHUB_TOKEN vs GitHub App

This setup uses `GITHUB_TOKEN` for release-please. This means:

- The Release PR is opened by `github-actions[bot]`.
- GitHub suppresses `pull_request` events for PRs opened by `GITHUB_TOKEN`,
  so the required CI checks do NOT run automatically on the Release PR.
- The Quality Ruleset has an admin `bypass_actor` (actor_id 5, RepositoryRole Admin)
  so the maintainer can merge the Release PR without waiting for checks.
- The Pages deploy runs after the merge via the regular main-push trigger.

### Upgrading to a GitHub App (optional)

If you want Release PRs to trigger full CI:

1. Install the shared releaser App on the repository — see
   [`repo-governance-common/reference/releaser-app.md`](../../repo-governance-common/reference/releaser-app.md).
   Do not create a new App; this one is shared across every repository.
2. Set two repository secrets:
   ```bash
   gh secret set RELEASER_APP_ID          --repo OWNER/REPO --body "<numeric-id>"
   gh secret set RELEASER_APP_PRIVATE_KEY --repo OWNER/REPO --body "$(cat key.pem)"
   ```
3. Update `release-please.yml` to generate a token from the App:
   ```yaml
   - uses: actions/create-github-app-token@v1
     id: app-token
     with:
       app-id: ${{ secrets.RELEASER_APP_ID }}
       private-key: ${{ secrets.RELEASER_APP_PRIVATE_KEY }}
   
   - uses: googleapis/release-please-action@v4
     with:
       token: ${{ steps.app-token.outputs.token }}
       ...
   ```
4. Remove the admin `bypass_actors` entry from `rulesets/quality.json` if
   you want release PRs to be enforced like any other PR.
