# github-audit-rulesets: cross-repository GitHub ruleset drift audit

Read-only audit for #130: reports which of tarotene's owned repositories have
drifted from the account-wide ruleset baseline, without applying or modifying
anything. Corrective action (re-applying a standard ruleset) stays out of
scope — see [Scope](#scope) below.

## Why this lives in dotfiles, not a dedicated inventory repo

A separate personal repository already called itself the "canonical
cross-repository governance rules" meta repository, which made it the
obvious first candidate. It was ruled out on inspection (2026-09-10): its
CI/Renovate/weekly data-release automation had been failing daily for
unrelated reasons, and it already had an open plan to fold its
responsibilities *into* dotfiles — building a new feature into a repo that
is planning to retire itself would run backwards against that plan. Its own
drift-detection issue is effectively superseded by this tool and should be
closed pointing here.

Given that, the tool (this script + its home-manager deployment) lives in
dotfiles. The **ledger data does not** — dotfiles is a `PUBLIC` repository,
and the ledger necessarily lists private repository names alongside their
governance state. Ledger output goes to
`$XDG_STATE_HOME/github-audit-rulesets/ledger.json`, a purely local file.

## Why judgement is by rule-type union, not ruleset name/count

The `*-repo-governance` skills' `rulesets/*.json` templates describe a
3-file layout — Security, Quality, Workflow — with byte-identical
Security/Quality rule types across skills. It was tempting to judge a
repository by "does it have exactly these 3 rulesets, by name". Checking
the actual account (2026-09-10) showed that would produce false positives:
some repositories split `copilot_code_review` into its own ruleset instead
of bundling it into Workflow as the skill templates do; others cover every
baseline rule type below except `required_status_checks` through a single
catch-all ruleset; others have no ruleset at all.

The skill templates and the live reference repos have themselves drifted
from each other (tracked separately — see [Scope](#scope)). The only thing
that stayed constant across every legitimately-governed shape observed is
the **union of rule types** active on the default branch. So judgement here
never looks at ruleset names or count — only at that union, plus the
`pull_request` rule's parameters (checked separately because its shape
mismatches are easy to miss by eye: `required_review_thread_resolution` and
`allowed_merge_methods` specifically).

Baseline rule types (`BASELINE_RULE_TYPES` in the script):

```
deletion, non_fast_forward, required_signatures, required_linear_history,
required_status_checks, pull_request, copilot_code_review
```

`required_status_checks`' actual check-name list (e.g. `MSRV (1.88)`,
`Format check`) is inherently project-specific (CI job titles) — the audit
reports those names for a human to read, but never machine-judges them
beyond confirming the rule type itself is present.

Separately, note that "applies to the default branch" must accept both the
`~DEFAULT_BRANCH` and `~ALL` special ref-name values — a ruleset scoped to
`~ALL` still constrains the default branch, and at least one governed repo
in the account carries `copilot_code_review` exclusively through such a
ruleset. A `~DEFAULT_BRANCH`-only check silently drops that rule type.

## Usage

```console
$ github-audit-rulesets
verdict=ok repo=telepath lang=Rust
verdict=drifted repo=dotfiles lang=Shell missing=required_signatures,required_linear_history,copilot_code_review,pull_request.required_review_thread_resolution
verdict=ungoverned repo=<private-repo> lang=Rust
verdict=exempt repo=sci-tech-coursework lang=TeX
total: 32 repo(s) - ok=12 drifted=6 ungoverned=9 exempt=5
```

Exit status is `1` if any non-exempt repository is `drifted` or
`ungoverned`, `0` otherwise — usable in a script, not just interactively.

`--json` prints the same findings as a JSON array instead of the table.
A ledger snapshot (`{generated_at, repos: [...]}`) is always written to
`$XDG_STATE_HOME/github-audit-rulesets/ledger.json`
(`GITHUB_AUDIT_RULESETS_STATE_DIR` overrides the directory).

### Exempting a repository

Coursework/paper/playground repos are never expected to carry the baseline
(most have zero rulesets and would otherwise report `ungoverned` noise
forever). List them, one per line, in
`$XDG_CONFIG_HOME/github-audit-rulesets/overrides.tsv`
(`GITHUB_AUDIT_RULESETS_OVERRIDES_FILE` overrides the path):

```
# repo<TAB>exempt
sci-tech-coursework	exempt
masters-thesis	exempt
```

An exempt repository is reported as `exempt` and is never judged — its own
ruleset content (even if it happens to look drifted) is not inspected for
the baseline.

## Scope

In scope: read-only inventory + drift detection (#130's "まず現状の棚卸しか
ら始める" framing). Explicitly out of scope, tracked in the wrap-up inbox
for a follow-up issue:

- **Applying** a standard ruleset to a drifted/ungoverned repository —
  still a manual job for the relevant `*-repo-governance` skill.
- A "new repository created → standard ruleset auto-applied" hook.
- Reconciling the skill templates' 3-ruleset (`rulesets/*.json`) layout
  against the 4-ruleset shape actually live on some reference repos
  (documented above as a known, real drift — the skills themselves are
  unversioned outside `~/.claude/skills/`, a separate piece of debt).
- Scheduled/periodic execution (a systemd user timer, mirroring
  `git-audit-worktrees` — `docs/worktree-lifecycle.md`). This is a manual
  command today.

Private-repository specifics (which repos were found drifted/ungoverned,
follow-up on any one repo's own issue tracker) are intentionally omitted
from this document — see `docs/claude/public-publish-guard.md` for why
dotfiles never names private/company repositories in its own tree.
