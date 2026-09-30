# Manual Steps (browser / one-time setup)

After `seed.sh` completes, the following steps require browser or one-time interactive flows
that cannot be scripted from a CI context.

---

## 1. Renovate

Nothing to do here per repository. The Mend Renovate App is installed
account-wide ("All repositories") — see
`config/claude/skills/repo-governance-common/reference/renovate-app.md`
for the one-time setup, how to confirm the App is actually running on
this repository, and the shared automerge policy preset that
`seed.sh`'s copy of `renovate.json` extends.

**What Renovate will do here:**
- Pin all `uses: foo/bar@vX.Y` actions to their SHA digest (`# vX.Y` comment preserved)
- Open PRs for GitHub Actions updates (grouped, automerge on green checks per the shared policy)
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
| `.github/workflows/ci.yml` — `build` job | `PATTERNS` regex — add your source directories |
| `.github/workflows/ci.yml` — `fmt` job | `PATTERNS` regex; `inputs:` path to typstyle-action |
| `.github/workflows/ci.yml` — `min-typst` job | `PATTERNS` regex |
| `.github/workflows/pr-title.yml` | Nothing to adjust — calls tarotene/dotfiles' composite action (ADR-0031/ADR-591); the reported check context is simply this job's own `name: PR title`, no manual confirmation needed |
| `.github/workflows/release.yml` | PDF filenames in `files:` block |
| `.github/workflows/metrics-reminder.yml` | Issue body checklist; remove if not a CV |
| `cliff.toml` | `tag_pattern` if your CalVer scheme differs from `vYYYY.MM[.P]` |
| `renovate.json` | Scheduling preferences; add repo-specific package rules if needed |

**Key invariant**: the `name:` field of each workflow job in `ci.yml`
**must exactly match** the entries in `ci-passed`'s `needs:` (job *id*, not
`name:`) — the Ruleset only ever requires `CI passed`/`PR title`
(ADR-591), so individual job `name:` values are display labels and no
longer need to match anything in `.github/rulesets/quality.json`.
`workflow-naming-check` (tarotene/dotfiles, a required check on every PR)
verifies this `needs:` coverage automatically.

**`pr-title.yml`** has no `workflow_call` concatenation to worry about
(ADR-591 replaced the old reusable-workflow form with a composite action,
docs/adr/591-ci-workflow-naming.md D3 in tarotene/dotfiles) — the reported
check context is simply this job's own `name: PR title`, pinned by the
`repo-governance-common/templates/.github/workflows/pr-title.yml` template
(this skill's copy is a symlink to it). `tarotene/dotfiles/.github/
actions/pr-title` re-verifies the match at runtime on every PR via
`scripts/rulesets-context-check` (every declared and live
`required_status_checks` context) and `scripts/workflow-naming-check`
(the `ci.yml` `needs:` coverage and `name:` casing basis, ADR-591).

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
  --body "Bootstrap typst-repo-governance: 2 required CI checks, 3 GitHub Rulesets, Renovate, git hooks."
```

Wait for both required status checks to go green:
- `CI passed` (aggregates `build`/`fmt`/`lint`/`min-typst` via `needs:`,
  ADR-591 — check the individual job's own logs, not this context, to
  diagnose a red PR)
- `PR title`

If `Format check` fails, run `just fmt` to auto-fix, commit, push.
If `Min Typst` fails, bump `compiler` in `typst.toml` + `--min-typst` flag + `quality.json` context.

---

## 5. Apply Rulesets and repo settings (after first PR is green)

```bash
# Apply repo settings (squash-only, delete-on-merge, etc.)
~/.claude/skills/typst-repo-governance/scripts/apply-repo-settings.sh \
  --owner OWNER --repo REPO

# Create the GitHub Rulesets — the generic apply script (not part of this
# skill) reads OWNER/REPO's own .github/rulesets/*.json declaration and
# verifies every context against the PR that just went green, so
# --reconcile (not --unverified-contexts) is appropriate here:
apply-rulesets.sh OWNER/REPO --reconcile
```

Squash-merge the PR. GitHub signs the squash commit, satisfying `required_signatures`.

---

## 6. Verify everything

```bash
# Rulesets created:
gh api repos/OWNER/REPO/rulesets --jq '.[].name'
# → Security, Quality, Workflow

# Required checks registered:
gh api repos/OWNER/REPO/rulesets \
  --jq '.[]|select(.name=="Quality")|.rules[]|select(.type=="required_status_checks")|.parameters.required_status_checks[].context'
# → CI passed
# → PR title

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
