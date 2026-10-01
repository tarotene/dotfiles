//! `github-audit [--json] [domain...]`(#414、ADR-0015)。判定は lib 側。

use github_audit::{any_drift, audit, parse_domains, render_report, write_ledger, Config};
use std::process::ExitCode;

const USAGE: &str = r#"usage: github-audit [--json|--selftest] [domain...]

Read-only cross-repository audit (ADR-0015). For every non-archived,
non-fork repository owned by $GITHUB_AUDIT_OWNER (default: tarotene),
judges the requested domains (default: all of them):

  rulesets   default-branch GitHub ruleset rule-type coverage (#130).
             required_status_checks is derived from CI presence
             (ADR-0020) — repos with no .github/workflows are reported
             drifted with missing=ci-absent instead of silently excused
  charters   README + CONTRIBUTING.md schema + root doc allowlist + CJK
             presence + CLAUDE.md/AGENTS.md routing (ADR-0013/0016/0017)
  naming     repository naming class declaration + pattern match
             (ADR-0014); repos created after ADR-0020's cutoff also face
             its closed vocabularies (descriptive species set, codename
             registry, site domain set) — the codename registry check
             applies to every repo regardless of cutoff
  settings   squash-only merge, delete-branch-on-merge, default branch,
             wiki/projects disabled, squash commit title=PR_TITLE/
             message=BLANK (ADR-0031)
  renovate   Renovate config presence, for repos with a dependency manifest
             and a CI workflow
  titles     PR-title commit-message contract enforcement presence
             (ADR-0031): pr-title.yml caller workflow + "PR title" required
             status check (exact-match: "PR title" self-applied, or
             "PR Title / PR title" workflow_call-connected), plus a
             ground-truth check against the latest run's actual job name
             (pr-title-context-mismatch, Amendment 2026-09-26). Presence-
             detection only, not individual open PR titles (the client
             guard and the required check judge those).
             not-applicable for repos with no .github/workflows
  lifecycle  dormancy candidate scoring (#275, ADR-0023): days since last
             push, days since last tagged release (if any), days since the
             most recently updated open issue (if any open issues exist),
             CI absence/latest-run failure. verdict=dormancy-candidate is
             a candidate list, NOT drift — it never fails this command's
             exit code, and the final Maintain/Archive/Delete call stays
             a human decision (docs/repo-lifecycle.md)
  releaser   releaser GitHub App secret wiring (release-plz/release-please),
             for repos that claim the releaser App: a release-plz.yml or
             release-please.yml workflow, OR any workflow whose
             actions/create-github-app-token step reads a RELEASE*-named
             secret/var (#613 — the claim follows the workflow's content,
             not just its file name). Checks RELEASER_APP_CLIENT_ID/
             RELEASER_APP_PRIVATE_KEY repo secret presence only — install status is not checked (a
             missing install fails loudly on the next release push, so
             this domain does not duplicate that detection)
  routines   Claude Code routine (scheduled cloud agent) declarations
             (ADR-519): for repos with a
             `.claude/routines/*.json`, checks the repo is listed in the
             self-audit routine's declared `sources` — otherwise its
             declarations are invisible to the weekly reconcile.
             not-applicable for repos with no `.claude/routines/` entries
  workflows  CI workflow naming basis (grill セッション由来 ADR): reserved
             `ci.yml` presence, a single `ci-passed` aggregate job whose
             `needs:` covers every other job in ci.yml, `name:` present and
             not lowercase-initial on both the workflow and every job,
             `.yml`/kebab-case filenames, and `.github/rulesets/quality.json`
             required_status_checks matching the single canonical source
             (this repository's own repo-governance-common template, read
             at runtime). Unlike titles/renovate/rulesets, a repo with NO
             `.github/workflows` at all is drifted here (ci-yml-missing),
             not not-applicable — every repo should eventually carry a
             ci.yml. Also flags a still-live legacy reusable-workflow call
             to `tarotene/dotfiles/.github/workflows/pr-title.yml@` in
             pr-title.yml (superseded by the composite-action caller form)
  docs      stack-standard API docs build (ADR-640): for repos whose
             manifests match the closed table (Cargo.toml -> rust,
             pyproject.toml -> python, a package.json declaring
             exports/main/types -> typescript), ci.yml must call the matching
             tarotene/dotfiles/.github/actions/docs-<stack>@main composite
             action (missing=docs-absent:<stack>, or docs-wrong-ref:<stack>
             when pinned to something other than @main). A PRIVATE repo with
             GitHub Pages enabled is drifted too (private-pages-enabled): on a
             personal account the site is public. That the docs job is in
             ci-passed.needs is judged by the workflows domain, not here.
             not-applicable for stacks absent from the table

Without options, prints a human-readable report and exits 1 if any
non-exempt repository is drifted/ungoverned in any judged domain
(lifecycle's dormancy-candidate verdict is exempt from this — see above).
--json
prints the machine-readable findings array instead. A ledger snapshot is
always written to $XDG_STATE_HOME/github-audit/ledger.json (override with
GITHUB_AUDIT_STATE_DIR).

This audit only reports drift — it never applies a fix. Use the
github-audit-triage skill (or the relevant *-repo-governance skill / gh
directly) to fix what it finds.

Running against an owner this account does not administer (a company org,
say) commonly needs two knobs (#533): GITHUB_AUDIT_VIEWER_PERMISSION
narrows the repo list to a single `viewerPermission` value (e.g. `ADMIN`)
before any domain is judged, and GITHUB_AUDIT_REPO_LIMIT raises the
`gh repo list --limit` past its 500 default for orgs with more repos than
that (gh itself paginates up to the given limit, so no manual loop is
needed here). Example:
  GITHUB_AUDIT_OWNER=<other-org> GITHUB_AUDIT_VIEWER_PERMISSION=ADMIN \
    GITHUB_AUDIT_REPO_LIMIT=2000 github-audit

Exempt a repository from one domain (or all of them) by adding a line
"<repo><TAB><domain-or-*><TAB>exempt" to
$XDG_CONFIG_HOME/github-audit/overrides.tsv (override with
GITHUB_AUDIT_OVERRIDES_FILE). Lines starting with # are ignored. ".github"
is permanently exempt from the naming and charters domains (GitHub reserves
the name).

ADR-0020's closed vocabularies (naming domain) live in
$XDG_CONFIG_HOME/github-audit/{codename-registry,descriptive-species,
site-domains}.tsv (repo-tracked, PUBLIC repos only) plus a
{codename-registry,site-domains}.local.tsv sibling for PRIVATE repos
(never committed). Override the paths with GITHUB_AUDIT_CODENAME_REGISTRY_FILE
/ GITHUB_AUDIT_CODENAME_REGISTRY_LOCAL_FILE /
GITHUB_AUDIT_DESCRIPTIVE_SPECIES_FILE / GITHUB_AUDIT_SITE_DOMAINS_FILE /
GITHUB_AUDIT_SITE_DOMAINS_LOCAL_FILE.

The routines domain's auditor-sources list lives in
$XDG_CONFIG_HOME/github-audit/routines-auditor-sources.tsv (PUBLIC repos,
repo-tracked; in practice always empty) plus a
routines-auditor-sources.local.tsv sibling (PRIVATE repos, never
committed) — same PUBLIC/PRIVATE pair shape as the naming domain's closed
vocabularies. Override with GITHUB_AUDIT_ROUTINES_AUDITOR_SOURCES_FILE /
GITHUB_AUDIT_ROUTINES_AUDITOR_SOURCES_LOCAL_FILE.
"#;

fn main() -> ExitCode {
    let mut json = false;
    let mut domain_args: Vec<String> = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--json" => json = true,
            "--selftest" => {
                // bash 版の --selftest は cargo test -p github-audit
                // (crates/github-audit/tests/)に移った。
                eprintln!(
                    "github-audit: --selftest is now `cargo test -p github-audit` (crates/github-audit/tests/)"
                );
                return ExitCode::from(2);
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            a if a.starts_with('-') => {
                eprint!("{USAGE}");
                return ExitCode::from(2);
            }
            _ => domain_args.push(arg),
        }
    }

    let domains = match parse_domains(&domain_args) {
        Ok(d) => d,
        Err(unknown) => {
            eprintln!("unknown domain: {unknown}");
            return ExitCode::from(2);
        }
    };

    let cfg = Config::from_env();
    let Ok(findings) = audit(&cfg, &domains) else {
        eprintln!("github-audit: aborting — audit() failed, not writing ledger or findings");
        return ExitCode::from(1);
    };
    if let Err(e) = write_ledger(&cfg.state_dir, &findings) {
        eprintln!(
            "github-audit: cannot write ledger under {}: {e}",
            cfg.state_dir.display()
        );
        return ExitCode::from(1);
    }
    if json {
        println!("{}", github_audit::findings_to_json(&findings));
    } else {
        print!("{}", render_report(&findings));
    }
    if any_drift(&findings) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
