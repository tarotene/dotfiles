# github-app-snapshot

`scripts/github-app-snapshot` is the only script in this repository allowed
to hold GitHub App PEMs or fine-grained personal access tokens (PATs). It
writes a read-only snapshot of each owned App's registration and install
targets, plus each fine-grained PAT's reachable-repository set, to
`$XDG_STATE_HOME/github-audit/app-snapshot.json` — the file `scripts/
github-audit` (`releaser`/`routines` domains) and `scripts/
github-app-registry-check` read lazily. Neither of those two ever sees a
secret; only this script does (ADR-0000-github-app-as-code D3, preserving
`docs/adr/436-single-releaser-github-app.md` D4 — github-audit requires no
secret).

## One-time Bitwarden setup

1. In Bitwarden Secrets Manager, create one project named `github-apps`.
   Add, per owned App `<name>` you declare a manifest for (currently just
   `releaser`, `config/github-app-manifests/releaser.json`):

   | Secret | Value |
   |---|---|
   | `GITHUB_APP_<NAME>_ID` | The App's numeric ID (`<NAME>` = `<name>` upper-cased, `-` → `_`, e.g. `RELEASER`) |
   | `GITHUB_APP_<NAME>_PEM` | The App's private key (PEM, full contents) |

   Plus, per row in `config/github-app-snapshot/pat-probes.tsv`:

   | Secret | Value |
   |---|---|
   | `CLAUDE_WEB_PAT` | A fine-grained PAT, selected repositories = every repo Claude's cloud sandbox should reach |

2. Create a machine account with **read-only** access to that one project
   and no other. This is a **separate** machine account from
   `obsidian-backup`'s (2026-09-30 decision, ADR-0000-github-app-as-code
   D5) — least privilege per Secrets Manager project, not a shared token.
   Generate an access token for this host.
3. Store only that revocable machine token in the login keyring:

   ```bash
   github-app-snapshot configure-token
   ```

   The prompt does not echo the token. It is not a recovery secret — do not
   duplicate it into a file or another vault. The Home Manager module
   (`home/modules/bitwarden.nix`, shared with `obsidian-backup`) configures
   the US Bitwarden service and disables bws state files, so a revoked
   token stops working immediately rather than up to an hour later.

## Creating an owned App from its Manifest

The App's registration source of truth is `config/github-app-manifests/
<name>.json` (ADR-0000-github-app-as-code D2). To create the App GitHub-side
from that declaration:

```bash
github-app-snapshot manifest-form releaser
```

This writes a self-submitting HTML page. Open it in a browser, click
"Create GitHub App", then copy the `code` query parameter from the resulting
`https://github.com/settings/apps?code=...&state=...` URL (the manifest's
`redirect_url` points at that fixed page, not a listener — there is nothing
else to click through). Hand that code back, then:

```bash
github-app-snapshot convert releaser <code>
```

This prints the new App's `id`/`slug`/`client_id`, writes its PEM to a
`0600` file under `$XDG_RUNTIME_DIR`, and prints exactly which two secrets
to store where (`GITHUB_APP_RELEASER_ID`/`GITHUB_APP_RELEASER_PEM` in the
`github-apps` Secrets Manager project). Discard the local PEM copy with
`shred -u <path>` once it's stored — the `code` itself expires after one
hour (GitHub Docs, "Registering a GitHub App from a manifest", 取得
2026-09-29), so this whole step must be re-run if you wait too long between
`manifest-form` and `convert`.

GitHub Docs, "Modifying a GitHub App registration" (取得 2026-09-29) — an
App's permissions/events can only be changed through the UI afterward, no
REST endpoint exists. `manifest-form`/`convert` only ever apply at
creation; `scripts/github-app-registry-check` (docs/github-audit.md)
detects drift between the Manifest and the live registration afterward but
cannot auto-repair it.

## Installing and distributing secrets

Install the App on a repository through `https://github.com/settings/apps`
(**Install App**, not a new App creation — see `config/claude/skills/
repo-governance-common/reference/releaser-app.md` for the releaser App's
full per-repo checklist). Then distribute the two repo secrets straight
from Secrets Manager, without ever landing the PEM on disk outside the
`convert` step above:

```bash
github-app-snapshot exec -- sh -c \
  'gh secret set RELEASER_APP_ID --repo tarotene/telepath --body "$GITHUB_APP_RELEASER_ID"'
github-app-snapshot exec -- sh -c \
  'gh secret set RELEASER_APP_PRIVATE_KEY --repo tarotene/telepath --body "$GITHUB_APP_RELEASER_PEM"'
```

(`exec -- <cmd>` runs `<cmd>` with every Secrets Manager secret injected as
an environment variable via `bws run --no-inherit-env`, the same
never-materialize-an-env-file pattern `scripts/obsidian-backup` uses — a
bare `gh secret set ... --body "$GITHUB_APP_RELEASER_PEM"` at the outer
shell would not see the variable, hence the `sh -c` wrapper.)

## Taking a snapshot

```bash
github-app-snapshot run
```

Writes `$XDG_STATE_HOME/github-audit/app-snapshot.json`: for each owned
App's manifest present in `config/github-app-manifests/`, its live
`permissions`/`events`/`slug` and every installation's `repository_selection`
+ repository list (via JWT → installation token → `GET /installation/
repositories`, paginated); for each `config/github-app-snapshot/
pat-probes.tsv` row, the repositories that PAT can push to (`GET /repos/
{owner}/{repo}`'s `permissions.push`, not a bare 200 — a fine-grained PAT
always reads public repos regardless of its selected-repositories scope).
No PEM, PAT, or installation token ever lands in this file — only names,
IDs, and permission/event strings.

Then run the detectors that read it:

```bash
github-app-registry-check
github-audit releaser routines
```

An App whose Secrets Manager secrets aren't set yet (or a PAT probe with no
matching secret) is skipped with a warning on stderr, not a hard failure —
`run` still writes a snapshot for every App/probe it *can* observe.

## `/web-setup` and the Claude cloud sandbox

<!-- Filled in from the 段4 M8 実機検証(2026-09-30 セッション以降)結果:
     GH_TOKEN injection via github-app-snapshot exec, whether claude picks
     it up for /web-setup, and the non-PAT-scope repo clone-failure check. -->

## Rotation

- **Owned App key** (25-key ceiling, no REST endpoint for key
  generation/deletion, GitHub Docs "Managing private keys for GitHub Apps",
  cited by `docs/adr/436-single-releaser-github-app.md` D3): generate a new
  key in the App's settings, update `GITHUB_APP_<NAME>_PEM` in Secrets
  Manager, re-run the `exec -- gh secret set` distribution above against
  every installed repository, confirm with `github-audit releaser`, then
  delete the old key from the App's settings.
- **Fine-grained PAT**: GitHub Docs, "Managing your personal access
  tokens" (取得 2026-09-29) caps a fine-grained PAT's lifetime at 366 days
  unless an org policy shortens it further. Create a replacement PAT before
  expiry, update `CLAUDE_WEB_PAT` in Secrets Manager, re-verify
  `/web-setup` (above), then revoke the old PAT.
- **Machine token**: `github-app-snapshot clear-token`, revoke in
  Bitwarden, issue a replacement, `github-app-snapshot configure-token`.
  Set the Bitwarden access token's own Expiration to roughly one year —
  same annual cadence as `obsidian-backup`'s own machine token
  (`docs/operations.md`).
