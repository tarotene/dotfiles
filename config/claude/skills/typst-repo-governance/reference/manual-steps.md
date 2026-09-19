# Manual Steps (browser / one-time setup)

After `seed.sh` completes, the following steps require browser or one-time interactive flows
that cannot be scripted from a CI context.

---

## 1. Install the Mend Renovate GitHub App

Renovate tracks GitHub Actions version pins **and** the Typst compiler version pin
(`typst-version: X.Y.Z` annotated with `# renovate: datasource=github-releases depName=typst/typst`).
This is the only automated tool that covers both.

**Steps:**
1. Navigate to https://github.com/apps/renovate and click **Install**.
2. Choose your personal account or organisation.
3. Under "Repository access", select **Only select repositories** → choose `OWNER/REPO`.
4. Click **Save**.

Renovate will open an onboarding PR within minutes. Merge it (or let it auto-close if you
already have a `renovate.json` — seed.sh copies one).

**What Renovate will do:**
- Pin all `uses: foo/bar@vX.Y` actions to their SHA digest (`# vX.Y` comment preserved)
- Open weekly PRs for GitHub Actions updates (grouped)
- Open PRs when `typst/typst` cuts a new release (annotation in workflow YAML required)

---

## 2. Set up commit signing (for `required_signatures` Ruleset)

The Quality Ruleset requires all commits on the default branch to be signed.
GitHub-created squash-merge commits are automatically verified, so PR-merged commits
will satisfy this rule without local config. However, local signing is recommended for
any direct commits (e.g. hotfixes, initial setup).

**SSH signing (recommended — simpler than GPG):**
```bash
# Generate an SSH key (if you don't have one):
ssh-keygen -t ed25519 -C "your-email@example.com"

# Add it as a signing key in GitHub Settings → SSH keys → Add new SSH key → Signing key

# Configure git to use SSH signing:
git config --global gpg.format ssh
git config --global user.signingkey ~/.ssh/id_ed25519.pub
git config --global commit.gpgsign true
```

**GPG signing (alternative):**
```bash
gpg --full-generate-key
gpg --armor --export YOUR_KEY_ID | gh gpg-key add -
git config --global user.signingkey YOUR_KEY_ID
git config --global commit.gpgsign true
```

---

## 3. Review `# ADJUST:` comments

After `copy-files.sh` runs, open each copied file and address every `# ADJUST:` comment.
Key locations:

| File | What to adjust |
|------|----------------|
| `Justfile` | `COMPILE_FLAGS`, `SRC_*`, `OUT_*` variables; `verify` DOCS table |
| `.github/workflows/build.yml` | `PATTERNS` regex — add your source directories |
| `.github/workflows/fmt.yml` | `PATTERNS` regex; `inputs:` path to typstyle-action |
| `.github/workflows/min-typst.yml` | `PATTERNS` regex |
| `.github/workflows/pr-title.yml` | Allowed commit types if you want a custom set |
| `.github/workflows/release.yml` | PDF filenames in `files:` block |
| `.github/workflows/metrics-reminder.yml` | Issue body checklist; remove if not a CV |
| `cliff.toml` | `tag_pattern` if your CalVer scheme differs from `vYYYY.MM[.P]` |
| `renovate.json` | Scheduling preferences; add repo-specific package rules if needed |

**Key invariant**: the `name:` field of each workflow job in `quality.json` `required_status_checks`
MUST exactly equal the `context` string. seed.sh substitutes `__MIN_TYPST__` in both files
simultaneously. If you rename a job manually, update the Ruleset context too.

---

## 4. First PR and CI verification

After committing all files:

```bash
# Create branch and commit
git checkout -b feat/governance-bootstrap
git add .github/ .githooks/ Justfile renovate.json cliff.toml .yamllint CODEOWNERS
git commit -m "feat(governance): add CI workflows, Rulesets, and developer tooling"
git push -u origin feat/governance-bootstrap
gh pr create --title "feat(governance): add CI workflows, Rulesets, and developer tooling" \
  --body "Bootstrap typst-repo-governance: 5 CI checks, 3 GitHub Rulesets, Renovate, git hooks."
```

Wait for all 5 status checks to go green:
- `Build`
- `Format check`
- `Lint`
- `Min Typst (X.Y.Z)`
- `PR title (Conventional Commits)`

If `Format check` fails, run `just fmt` to auto-fix, commit, push.
If `Min Typst` fails, bump `compiler` in `typst.toml` + `--min-typst` flag + `quality.json` context.

---

## 5. Apply Rulesets and repo settings (after first PR is green)

```bash
# Apply repo settings (squash-only, delete-on-merge, etc.)
~/.claude/skills/typst-repo-governance/scripts/apply-repo-settings.sh \
  --owner OWNER --repo REPO

# Create the 3 GitHub Rulesets
~/.claude/skills/typst-repo-governance/scripts/apply-rulesets.sh \
  --owner OWNER --repo REPO --min-typst X.Y.Z
```

Squash-merge the PR. GitHub signs the squash commit, satisfying `required_signatures`.

---

## 6. Verify everything

```bash
# Rulesets created:
gh api repos/OWNER/REPO/rulesets --jq '.[].name'
# → Security, Quality, Workflow

# Required checks registered (contexts must match job names exactly):
gh api repos/OWNER/REPO/rulesets \
  --jq '.[]|select(.name=="Quality")|.rules[]|select(.type=="required_status_checks")|.parameters.required_status_checks[].context'
# → Build
# → Format check
# → Lint
# → Min Typst (X.Y.Z)
# → PR title (Conventional Commits)

# Repo merge settings:
gh api repos/OWNER/REPO --jq '{allow_squash_merge,allow_merge_commit,delete_branch_on_merge}'
# → { "allow_squash_merge": true, "allow_merge_commit": false, "delete_branch_on_merge": true }

# Hooks wired:
git -C /path/to/repo config --local core.hooksPath
# → .githooks

# Justfile valid and recipes work:
just --list
just build
just verify
echo "feat: x" | just commit-check /dev/stdin  # should pass
echo "bad msg"  | just commit-check /dev/stdin  # should exit 1
```
