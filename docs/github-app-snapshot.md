# github-app-snapshot

`scripts/github-app-snapshot` is the only script in this repository allowed
to hold GitHub App PEMs. It writes a read-only snapshot of each owned App's
registration and install targets to
`$XDG_STATE_HOME/github-audit/app-snapshot.json` — the file `scripts/
github-audit` (`releaser` domain) and `scripts/
github-app-registry-check` read lazily. Neither of those two ever sees a
secret; only this script does (ADR-590 D3, preserving
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
   | `GITHUB_APP_<NAME>_CLIENT_ID` | The App's Client ID (`Iv…`; not read by `github-app-snapshot run` itself — it is what `gh secret set RELEASER_APP_CLIENT_ID` distributes, since `actions/create-github-app-token`'s `app-id` input is deprecated in favour of `client-id`, #615) |

2. Create a machine account with **read-only** access to that one project
   and no other. This is a **separate** machine account from
   `obsidian-backup`'s (2026-09-30 decision, ADR-590
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
<name>.json` (ADR-590 D2). To create the App GitHub-side
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
`0600` file under `$XDG_RUNTIME_DIR`, and prints exactly which three secrets
to store where (`GITHUB_APP_RELEASER_ID`/`GITHUB_APP_RELEASER_PEM`/`GITHUB_APP_RELEASER_CLIENT_ID` in the
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
full per-repo checklist). Then distribute the two repo secrets (`RELEASER_APP_CLIENT_ID`, `RELEASER_APP_PRIVATE_KEY`) straight
from Secrets Manager, without ever landing the PEM on disk outside the
`convert` step above:

```bash
github-app-snapshot exec -- sh -c \
  'gh secret set RELEASER_APP_CLIENT_ID --repo tarotene/telepath --body "$GITHUB_APP_RELEASER_CLIENT_ID"'
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
repositories`, paginated). No PEM or installation token ever lands in this
file — only names, IDs, and permission/event strings.

Then run the detectors that read it:

```bash
github-app-registry-check
github-audit releaser
```

An App whose Secrets Manager secrets aren't set yet is skipped with a
warning on stderr, not a hard failure — `run` still writes a snapshot for
every App it *can* observe.

## Claude's cloud sandbox reach (not managed by this script)

How far Claude's cloud sandbox (`claude --cloud`, cloud routines) reaches is
decided by how GitHub was connected, not by anything in this repository
(Claude Code docs, "Use Claude Code in the cloud", 取得 2026-10-01):

- **Claude GitHub App** (connect in the browser at claude.ai/code): sessions
  reach any public repository, and private repositories the App is installed
  on. Install it with **Only select repositories** to keep the reach narrow.
  This is the recommended way (ADR-590 Amendment 2026-10-01).
- **`/web-setup`**: sends your `gh` CLI token to Anthropic; sessions then
  reach every repository that token can access. A fine-grained PAT cannot be
  used here — `/web-setup` requires the classic `repo` scope and fails with
  "GitHub token could not be validated" (verified 2026-10-01). Do not run it
  with a broader token than you intend to grant.

`github-audit` does not detect this reach yet: the Claude App's install
targets are not readable with a `gh` OAuth token, and the classic-PAT route
is untested (see the follow-up Issue referenced from ADR-590's Amendment).

## Rotation

- **Owned App key** (25-key ceiling, no REST endpoint for key
  generation/deletion, GitHub Docs "Managing private keys for GitHub Apps",
  cited by `docs/adr/436-single-releaser-github-app.md` D3): generate a new
  key in the App's settings, update `GITHUB_APP_<NAME>_PEM` in Secrets
  Manager, re-run the `exec -- gh secret set` distribution above against
  every installed repository, confirm with `github-audit releaser`, then
  delete the old key from the App's settings.
- **Machine token**: `github-app-snapshot clear-token`, revoke in
  Bitwarden, issue a replacement, `github-app-snapshot configure-token`.
  Set the Bitwarden access token's own Expiration to roughly one year —
  same annual cadence as `obsidian-backup`'s own machine token
  (`docs/operations.md`).
