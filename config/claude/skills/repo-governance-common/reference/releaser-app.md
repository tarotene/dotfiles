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

The App's registration (permissions/events) source of truth is
`config/github-app-manifests/releaser.json` (ADR-590 D2) — not a note
anywhere in the GitHub UI. `scripts/github-app-registry-check` detects
drift between that Manifest and the App's live registration.
`github-audit`'s `releaser` domain (`crates/github-audit`, `docs/github-audit.md`)
detects repositories whose `RELEASER_APP_CLIENT_ID`/`RELEASER_APP_PRIVATE_KEY`
secrets are missing or still under a pre-consolidation tool-specific name,
**and** — once `scripts/github-app-snapshot` has been run — whether the
App is actually installed on the repository. Run both after onboarding a
repository to confirm the wiring took.

## Creating the App (once, from its Manifest)

Only needed if the shared App doesn't exist yet, or is being recreated
from scratch. Full walkthrough: `docs/github-app-snapshot.md`. In short:

```bash
github-app-snapshot manifest-form releaser
# open the resulting HTML page in a browser, click "Create GitHub App",
# then copy the `code` query parameter from the redirect URL
github-app-snapshot convert releaser <code>
# store the two secrets it prints in the "github-apps" Secrets Manager
# project, then shred the local PEM copy it names
```

## Install the existing App on a new repository

1. Go to `https://github.com/settings/apps` and open the shared releaser
   App (App ID and PEM live in the Bitwarden Secrets Manager `github-apps`
   project — see below, not on this page).
2. **Install App** → select the repository to add. Do not create a new
   App.

Required permissions (already set on the App per its Manifest — nothing to
configure per repository): Contents R/W · Issues R/W (for the `release`
label on PRs) · Pull requests R/W · Webhook disabled.

## Add secrets to the repository

Distribute the two repo secrets straight from Secrets Manager — never land
the PEM on disk outside `github-app-snapshot convert`'s own step:

```bash
github-app-snapshot exec -- sh -c \
  'gh secret set RELEASER_APP_CLIENT_ID --repo OWNER/REPO --body "$GITHUB_APP_RELEASER_CLIENT_ID"'
github-app-snapshot exec -- sh -c \
  'gh secret set RELEASER_APP_PRIVATE_KEY --repo OWNER/REPO --body "$GITHUB_APP_RELEASER_PEM"'
```

Then confirm the wiring took:

```bash
github-app-snapshot run
github-audit releaser
```

## Reference this from a release workflow

```yaml
- name: Generate GitHub App token
  id: generate-token
  uses: actions/create-github-app-token@<pinned-sha> # vN
  with:
    client-id: ${{ secrets.RELEASER_APP_CLIENT_ID }}
    private-key: ${{ secrets.RELEASER_APP_PRIVATE_KEY }}
```

Omit the `owner`/`repositories` inputs — leaving them empty scopes the
minted token to the current repository only, which is what every
`*-repo-governance` release workflow template relies on.

## Key rotation

GitHub allows at most 25 private keys per App (they never expire on their
own; deletion is manual), and key generation/deletion is UI-only — there
is no REST endpoint for it.

To rotate: generate a new key in the App's settings, update
`GITHUB_APP_RELEASER_PEM` in the Secrets Manager `github-apps` project,
then re-run the `github-app-snapshot exec -- gh secret set
RELEASER_APP_PRIVATE_KEY` distribution above against every repository the
App is installed on (`github-app-snapshot run` then `github-audit --json
releaser` lists which repositories currently carry the secrets, so you can
enumerate the targets from its output rather than guessing). Confirm with
`github-app-registry-check` that the registration itself hasn't drifted,
then delete the old key from the App's settings once every repository is
confirmed on the new one.
