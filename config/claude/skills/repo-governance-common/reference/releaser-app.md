# releaser GitHub App

`release-plz` / `release-please` need a GitHub App installation token (not
`GITHUB_TOKEN`) so that release PRs they create can trigger required CI
checks. GitHub's anti-recursion guard silently suppresses events from
`GITHUB_TOKEN`-created objects.

**There is one App for this, shared across every repository.** Do not
create a new App per repository — a bot identity is a permission set, not
a per-repository resource, and `actions/create-github-app-token` already
scopes the token it mints to the current repository when `owner`/
`repositories` are omitted (which every workflow template in this repo
does), so a per-repo App buys no extra blast-radius containment over a
single shared one. This consolidated the 4 App registrations that had
accumulated on the personal account before each `*-repo-governance` skill
minted its own (`docs/adr/436-single-releaser-github-app.md`).

`scripts/github-audit`'s `releaser` domain (`docs/github-audit.md`)
detects repositories whose `RELEASER_APP_ID`/`RELEASER_APP_PRIVATE_KEY`
secrets are missing or still under a pre-consolidation tool-specific name
— run it after onboarding a repository to confirm the wiring took.

## Install the existing App on a new repository

1. Go to `https://github.com/settings/apps` and open the shared releaser
   App (App ID and PEM live in the Bitwarden vault item for it — see
   below, not on this page).
2. **Install App** → select the repository to add. Do not create a new
   App.

Required permissions (already set on the App — nothing to configure per
repository): Contents R/W · Issues R/W (for the `release` label on PRs) ·
Pull requests R/W · Webhook disabled.

## Add secrets to the repository

Retrieve the App ID and private key from the Bitwarden vault item (Secure
Note) named for this App, then:

```
gh secret set RELEASER_APP_ID          --repo OWNER/REPO --body "<numeric-app-id>"
gh secret set RELEASER_APP_PRIVATE_KEY --repo OWNER/REPO --body "$(cat <path-to>.pem)"
```

Discard the local copy of the PEM file once both secrets are set — the
vault item is the single source of truth, not a working copy on disk.

## Reference this from a release workflow

```yaml
- name: Generate GitHub App token
  id: generate-token
  uses: actions/create-github-app-token@<pinned-sha> # vN
  with:
    app-id: ${{ secrets.RELEASER_APP_ID }}
    private-key: ${{ secrets.RELEASER_APP_PRIVATE_KEY }}
```

Omit the `owner`/`repositories` inputs — leaving them empty scopes the
minted token to the current repository only, which is what every
`*-repo-governance` release workflow template relies on.

## Key rotation

GitHub allows at most 25 private keys per App (they never expire on their
own; deletion is manual), and key generation/deletion is UI-only — there
is no REST endpoint for it.

To rotate: generate a new key in the App's settings, update the Bitwarden
vault item, then re-run `gh secret set RELEASER_APP_PRIVATE_KEY --repo
OWNER/REPO --body "$(cat <path-to>.pem)"` against every repository the App
is installed on (`scripts/github-audit --json releaser` lists which
repositories currently carry the secrets, so you can enumerate the
targets from its output rather than guessing). Delete the old key from
the App's settings once every repository is confirmed on the new one.
