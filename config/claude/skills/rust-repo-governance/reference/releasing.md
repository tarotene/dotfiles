# Release Runbook

See `AGENTS.md § How releases work` for the normal release cycle overview.
This document covers debugging, recovery, and one-time setup procedures.

Replace `__OWNER__`, `__REPO__`, `__CANONICAL_CRATE__` with your actual values.

---

## Setup: GitHub App token

**Why**: The `release-plz-pr` and `release-plz-release` jobs create GitHub
objects (release PRs, tags, Releases). If created via `GITHUB_TOKEN`, GitHub's
anti-recursion guard suppresses the resulting `pull_request` and `release:published`
events — breaking required CI on release PRs and preventing `release-binaries.yml`
from firing.

**App permissions needed**: Contents R/W · Issues R/W · Pull requests R/W · Webhook disabled.

**Repository secrets**:

| Secret | Value |
|--------|-------|
| `RELEASE_PLZ_APP_ID` | Numeric App ID from app settings page |
| `RELEASE_PLZ_APP_PRIVATE_KEY` | PEM-encoded private key |

```
gh secret set RELEASE_PLZ_APP_ID --repo __OWNER__/__REPO__ --body "<id>"
gh secret set RELEASE_PLZ_APP_PRIVATE_KEY --repo __OWNER__/__REPO__ --body "$(cat key.pem)"
```

---

## Retriggering the release workflow

If `release-plz-pr` or `release-plz-release` fails or stalls:

```
gh workflow run release-plz.yml --repo __OWNER__/__REPO__ --ref main
```

Or: Actions → Release-plz → Run workflow.

---

## Overriding the next version

```
# Preview what release-plz would do:
just release-preview

# Pin the next version:
release-plz set-version X.Y.Z --package __CANONICAL_CRATE__
git add Cargo.toml && git commit -m "chore: pin next release to X.Y.Z"
git push
```

release-plz will pick up the pin on the next run.

---

## Recovering from a duplicate or stale release PR

```
gh pr list --repo __OWNER__/__REPO__ --label release --state open
gh pr close <NUMBER> --repo __OWNER__/__REPO__
```

Then retrigger to get a fresh PR.

---

## Recovering from a bad release (deleted tag)

```
gh release delete __CANONICAL_CRATE__-vX.Y.Z \
  --repo __OWNER__/__REPO__ --cleanup-tag --yes
```

**Do not re-use a deleted tag.** Increment the patch instead
(e.g. v0.1.0 deleted → next release is v0.1.1).

---

## Excluded crates: bump-excluded requirement

`tools/__CLI_CRATE__` is under `exclude` in the root `Cargo.toml` and is
invisible to release-plz. Its `version` is bumped automatically by the
`release-plz-pr` CI step via `just bump-excluded`.

**Recovery if the automatic bump fails** (manual fallback):

```
git fetch origin <release-pr-branch>
git checkout <release-pr-branch>
just bump-excluded X.Y.Z
git add tools/__CLI_CRATE__/Cargo.toml tools/__CLI_CRATE__/Cargo.lock
git commit -m "chore(release): bump excluded crates to X.Y.Z"
git push
```

---

## Trusted Publishing

The workflow uses **Trusted Publishing (OIDC)** — no long-lived
`CARGO_REGISTRY_TOKEN` is stored. The `release-plz-release` job has
`id-token: write` so GitHub Actions can exchange a short-lived OIDC token
with crates.io at publish time.

**Setup per crate** (after first bootstrap publish):

1. Open `https://crates.io/crates/<CRATE_NAME>/settings`
2. Trusted Publishers → Add publisher:
   - Owner: `__OWNER__`
   - Repository: `__REPO__`
   - Workflow: `release-plz.yml`
   - Environment: *(blank)*
3. Save.

After setup, the workflow handles publishing — no manual intervention needed.

For the first-ever publish of a crate, see `reference/manual-steps.md § First publish`.

---

## Release scheduling model

| Change type | Version bump | When to merge the Release PR |
|-------------|-------------|------------------------------|
| Bug fix / non-breaking | Patch | As soon as ready |
| Feature addition | Minor | When target Milestone is 100% closed |
| Breaking change | Minor (pre-1.0) | Always bundle into a Minor Milestone |

Wire-protocol breaking changes require firmware and host to update
simultaneously — never release them in isolation.
