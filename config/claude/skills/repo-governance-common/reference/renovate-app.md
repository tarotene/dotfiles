# Renovate (Mend) GitHub App

`renovate.json` in a repository is inert without the Mend Renovate
GitHub App actually running against that repository — the config file
and the App installation are two independent things (`docs/adr/568-renovate-automerge-shared-preset.md` Context: a 2026-09-29 audit found
10 repositories with `renovate.json` but zero Renovate PRs ever, because
the App was never installed on them).

**Install the App once, account-wide.** Do not install it per
repository. As of ADR-568 D5, the App's repository access is set to
"All repositories" — a newly created repository is covered automatically,
with nothing to remember per repository.

## One-time setup (already done; this is the runbook if it ever needs redoing)

1. Go to `https://github.com/settings/installations` and open the Mend
   Renovate App (<https://github.com/apps/renovate>).
2. **Configure** → **Repository access** → **All repositories**.

There is no secret to provision (SaaS, no App private key on this
account — `docs/adr/0010-retire-sops-runtime-secrets.md` retired the
prior self-hosted `RENOVATE_APP_ID`/`RENOVATE_APP_PRIVATE_KEY` App).

## Confirming the App is actually running on a repository

The GitHub API cannot tell a user's own OAuth token which repositories a
GitHub App is installed on (`/user/installations` needs a user-to-server
token; the usual `gh` token gets a 403). The observable proxy is the
**Dependency Dashboard issue**: Renovate opens one unconditionally at the
end of every repository run, before any schedule gating
(`github-audit`'s `renovate` domain, `crates/github-audit`, #465). If a repository has
`renovate.json` but no Dependency Dashboard issue has ever been opened,
either the App does not (yet) have access, or its first run has not
happened yet.

## Every repository's `renovate.json` extends the shared policy preset

Do not copy automerge/schedule/label settings into a repository's own
`renovate.json` — they live in one place, `renovate/policy.json` in this
repository, referenced as:

```json
{ "extends": ["github>tarotene/dotfiles//renovate/policy"] }
```

A repository's own `renovate.json` should contain only ecosystem-specific
`packageRules` (dependency groups, an MSRV-protect rule, a custom
manager) on top of that `extends`. See
`docs/adr/568-renovate-automerge-shared-preset.md` D4 for why.

## Dependabot stays alert-only

`apply-repo-settings.sh` enables Dependabot vulnerability alerts (they
feed Renovate's `vulnerabilityAlerts`) and disables Dependabot security
updates (its own fix-PR generation) on every repository it touches — see
D5b in the same ADR. Renovate's `vulnerabilityAlerts` is the single
fix-PR channel; Dependabot never opens a competing PR for the same
advisory.
