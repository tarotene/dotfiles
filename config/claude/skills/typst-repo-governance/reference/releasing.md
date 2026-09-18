# Release Runbook

This project uses **CalVer**: tags follow `vYYYY.MM` (first release of a month)
or `vYYYY.MM.P` (subsequent releases within the same month, P ≥ 1).

**Never use** `-beta`, `-rc`, `-alpha`, or `b`/`c` suffixes — these violate the convention
and will be excluded from `cliff.toml` changelog generation by the `ignore_tags` filter.

---

## Normal release flow

### Via GitHub Actions UI (recommended)

1. Go to **Actions** → **Release** → **Run workflow**.
2. Enter the tag in the `tag` input field (e.g. `v2026.07`).
3. Click **Run workflow**.

The workflow will:
- Build all PDFs from the latest commit on the default branch
- Run the ATS verification gate (`just verify`)
- Create and push the tag
- Generate changelog with git-cliff (`cliff.toml`)
- Publish a GitHub Release with the PDFs attached

### Via CLI (tag-push trigger)

```bash
# Ensure your local main is up to date:
git fetch origin && git rebase origin/main

# Create and push the tag:
git tag v2026.07
git push origin v2026.07
```

The `push: tags: ["v*"]` trigger fires the same release workflow.

---

## Patch release (same month)

Use `vYYYY.MM.P` where P starts at 1:

```bash
git tag v2026.07.1
git push origin v2026.07.1
```

Update `typst.toml` `version` field to match before tagging:
```toml
version = "2026.07.1"
```

---

## Retriggering a failed release

If the release workflow fails after the tag was already pushed:

1. Delete the failed GitHub Release (if any): `gh release delete v2026.07 --cleanup-tag`
   **Warning**: deleting a released tag is destructive — never reuse a tag once a release
   has been publicly visible.
2. Fix the underlying issue (e.g. a Typst source error).
3. Re-run via UI (tag-push path won't re-fire for the same tag):
   **Actions** → **Release** → **Run workflow** → enter the same tag name.

---

## Manual changelog generation

Preview changelog for the next release:
```bash
git cliff --unreleased --tag v2026.07
```

Generate full changelog file:
```bash
git cliff -o CHANGELOG.md
```

Generate notes for a specific tag range:
```bash
git cliff v2026.06..v2026.07
```

---

## Tag hygiene

If you find a tag violating the convention (e.g. `v2026.05-beta`), surface it to the
project owner before deleting — tag deletion is destructive and cannot be undone once
collaborators have fetched.

```bash
# List all tags with release info:
gh release list

# Delete a bad pre-release tag and its release (confirm with owner first!):
gh release delete v2026.05-beta --cleanup-tag
```

---

## Renovate: Typst version bump

When Renovate opens a PR to bump the Typst pin (e.g. `0.14.2 → 0.14.3`):

1. Check the Typst changelog for breaking changes: https://typst.app/docs/changelog/
2. The `Build` CI check will run with the new version automatically.
3. If `Build` passes, squash-merge the PR.
4. If `Min Typst` also needs bumping (e.g. the new version changes behaviour visible at
   the minimum version), update `--min-typst`, `typst.toml` `compiler`, and the Ruleset
   context string `Min Typst (X.Y.Z)` to match — all three must stay in sync.
