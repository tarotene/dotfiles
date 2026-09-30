# Manual Steps Checklist

These steps cannot be automated by `seed.sh` because they involve
browser-based GitHub UI flows, secret management, or crates.io one-time
operations. Complete them after running `seed.sh`.

---

## 1. GitHub App for release-plz

See [`repo-governance-common/reference/releaser-app.md`](../../repo-governance-common/reference/releaser-app.md)
(in this Claude skills directory) — the releaser App is shared across
every repository, not created per repository. Install the existing App on
this repository and set `RELEASER_APP_CLIENT_ID`/`RELEASER_APP_PRIVATE_KEY`.

---

## 2. crates.io Trusted Publishing

Trusted Publishing lets the CI workflow publish to crates.io via short-lived
OIDC tokens — no long-lived `CARGO_REGISTRY_TOKEN` needed.

### First publish (bootstrap — one time per crate)

Trusted Publishing requires the crate to already exist on crates.io. For the
first publish of each new crate, use a temporary API token:

1. Go to `https://crates.io/settings/tokens` → generate a token with scope
   `publish-new`, 7-day expiry.
2. Publish crates in dependency order:
   ```
   # Example for a workspace with 4 publishable crates:
   cargo publish -p my-wire-crate && sleep 30
   cargo publish -p my-macros-crate && sleep 30
   cargo publish -p my-server-crate && sleep 30
   cargo publish -p my-client-crate && sleep 30
   (cd tools/my-cli && cargo publish)
   ```
3. Revoke the token immediately after.

### Register Trusted Publishing entries

For each published crate, go to  
`https://crates.io/crates/<CRATE_NAME>/settings` → **Trusted Publishers** →
**Add publisher**:

| Field | Value |
|-------|-------|
| GitHub owner | `OWNER` |
| Repository name | `REPO` |
| Workflow filename | `release-plz.yml` |
| Environment name | *(leave blank)* |

Repeat for every crate that the workflow publishes (workspace crates published
by release-plz + the excluded CLI crate published by the separate step).

---

## 3. Post-copy adjustments

Renovate: nothing to install per repository — the Mend Renovate App is
account-wide (see
[`repo-governance-common/reference/renovate-app.md`](../../repo-governance-common/reference/renovate-app.md)).
`renovate.json` extends the shared automerge policy preset; only this
ecosystem's `packageRules` (below) are repo-specific.

After `seed.sh --dry-run` or `seed.sh` completes, open each file that has a
`# ADJUST:` comment and edit as needed. Key locations:

| File | What to adjust |
|------|----------------|
| `.github/workflows/ci.yml` — `host` job | PATTERNS regex — crate directory names |
| `.github/workflows/ci.yml` — `tools` job | PATTERNS regex — crate directory names |
| `.github/workflows/ci.yml` — `msrv` job | PATTERNS regex — all workspace paths |
| `.github/workflows/ci.yml` — `firmware` job | Chip name, target triple, example path — or delete the whole job (and its entry in `ci-passed`'s `needs:`) if your project has no embedded firmware |
| `.github/workflows/release-plz.yml` | `host-pty-server` git-only package name; additional excluded crates |
| `.github/workflows/release-binaries.yml` | License file names, README path |
| `.github/workflows/release-reminder.yml` | AGENTS.md anchor URL |
| `renovate.json` | `cargo.managerFilePatterns` — add excluded crate paths; embedded HAL package list. `cargo.rangeStrategy: "bump"` needs no adjustment — it exists so an in-range dependency update changes `Cargo.toml` itself (not just `Cargo.lock`), keeping it distinct from the monthly `lockFileMaintenance` PR instead of duplicating it (#474) |
| `release-plz.toml` | `[[package]]` list — add your crates, remove `host-pty-server` if not applicable |
| `Justfile` | Feature flag combos in `clippy-tools` and `mcp-test`; smoke test assertions |
| `.github/rulesets/quality.json` | Nothing to adjust — required contexts are the fixed pair `CI passed`/`PR title` (ADR-591 in tarotene/dotfiles), shared with every other governed repository regardless of workspace layout |

---

## 4. Git hooks

After files are copied (or if you skipped `--dest`):

```
git -C /path/to/repo config --local core.hooksPath .githooks
```

Prerequisites the hooks require (install once per machine):

```
cargo install just
cargo install --locked cocogitto
```

---

## 5. First PR

```
git -C /path/to/repo add .github .githooks renovate.json release-plz.toml cog.toml rust-toolchain.toml Justfile
git -C /path/to/repo commit -m "chore: apply rust-repo-governance templates"
git -C /path/to/repo push -u origin <branch>
gh pr create --repo OWNER/REPO --title "chore: apply rust-repo-governance templates"
```

The PR will trigger all 6 (or 5, without firmware) CI checks. If any fail,
check the `# ADJUST:` items — most failures trace back to uncustomized paths
or feature flags.
