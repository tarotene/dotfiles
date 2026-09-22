# github-audit: unified cross-repository GitHub audit

Read-only audit across six domains — reports drift for every one of
tarotene's owned repositories without applying or modifying anything.
Unifies the former sibling scripts `github-audit-rulesets` (#130) and
`github-audit-charters` (ADR-0013), and adds four domains: naming
(ADR-0014), settings, renovate (ADR-0015), and titles (ADR-0031). Its
findings feed the `github-audit-triage` skill
(`docs/claude/github-audit-triage.md`), which is the only place an LLM
enters this loop — this script never calls one.

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
`$XDG_STATE_HOME/github-audit/ledger.json`, a purely local file.

## Why one script, not five siblings (ADR-0015)

Each new domain (naming, settings, renovate, titles) shares the same skeleton as
the original two — list repositories, judge, write a ledger, support
`--selftest` and an overrides exempt file. Five sibling scripts would mean
five copies of that skeleton and five ledgers for an LLM triage step to
merge. Instead, `github-audit` is a single command that takes a list of
domains (default: all), shares one repository listing and one GraphQL
batch fetch across whichever domains need file content, and writes one
ledger keyed by repository → domain → finding.

The old two-script overrides files (`$XDG_CONFIG_HOME/github-audit-
rulesets/overrides.tsv`, `$XDG_CONFIG_HOME/github-audit-charters/
overrides.tsv`) are not migrated automatically — the new format
(`<repo><TAB><domain-or-*><TAB>exempt`, in
`$XDG_CONFIG_HOME/github-audit/overrides.tsv`) needs a domain column the
old files didn't have, so re-adding a handful of exempt lines by hand
after the cutover is simpler than writing a one-shot migration script for
a single-user tool.

## Domains

### rulesets (#130, ADR-0020)

Reports which repositories' default-branch rulesets have drifted from the
account-wide baseline.

**`required_status_checks` is derived from CI presence, not required
unconditionally (ADR-0020).** A repository with no `.github/workflows`
has no status check to require, so this one rule type is excluded from the
baseline loop for it — but that exclusion is never silent. Such a
repository is still reported `drifted`, with `ci-absent` in `missing`
instead of `required_status_checks`. This matters because a plain
`not-applicable` (as used by the `renovate` domain below) would remove the
repository from view entirely, and CI never gets a reason to exist —
`ci-absent` keeps it visible so `github-audit-triage` can propose {a
minimal CI-seeding PR / a CI-seeding Issue / an exempt} for a human to pick.

**Why judgement is by rule-type union, not ruleset name/count.** The
`*-repo-governance` skills' `rulesets/*.json` templates describe a 3-file
core layout — Security, Quality, Workflow — with byte-identical
Security/Quality rule types across skills (plus a 4th, opt-in `review.json`
file — see below). It was tempting to judge a repository by "does it have
exactly these rulesets, by name". Checking the actual account (2026-09-10)
showed that would produce false positives: some repositories split
`copilot_code_review` into its own ruleset instead of bundling it into
Workflow as the skill templates used to; others cover every baseline rule
type below except `required_status_checks` through a single catch-all
ruleset; others have no ruleset at all.

The only thing that stayed constant across every legitimately-governed
shape observed is the **union of rule types** active on the default
branch. So judgement here never looks at ruleset names or count — only at
that union, plus the `pull_request` rule's parameters
(`required_review_thread_resolution`, `allowed_merge_methods`).

**Two layers, not one baseline (ADR-0021).** Prior to ADR-0021, the
baseline bundled a mandatory review-approval workflow — Copilot code
review auto-requested plus required conversation resolution before
merge — into the same single baseline as basic branch protection. For an
early-stage or pre-release repository, forcing that review round trip on
every commit was judged excessive and noisy, without a way to opt out
short of exempting the whole `rulesets` domain (losing coverage of
`deletion`/`non_fast_forward`/etc. too). The baseline is now two layers:

- **Core layer** — always required, unconditionally. `BASELINE_RULE_TYPES`
  in the script:

  ```
  deletion, non_fast_forward, required_signatures, required_linear_history,
  required_status_checks, pull_request
  ```

  (`pull_request`'s `allowed_merge_methods` must resolve to squash-only —
  see the aggregation note below.)

- **Review layer** — an opt-in addin, judged only if a repository has
  opted into it. Presence is detected by either half showing up: the
  `copilot_code_review` rule type, or any active `pull_request` rule with
  `required_review_thread_resolution: true`. A repository with neither is
  reported `review_layer=absent` — informational, never drift (mirroring
  the existing `required_status_checks` check-name precedent below: this
  audit collects and reports what it can't or shouldn't force a judgement
  on). A repository with one half but not the other is `review_layer=
  partial-drift` — `missing` carries `review_layer.copilot_code_review` and/
  or `review_layer.required_review_thread_resolution` as needed. Both
  halves present is `review_layer=complete`.

  A phase-tracking mechanism (a declared "development stage" signal, e.g.
  via GitHub topics or Release presence) was considered and rejected — see
  ADR-0021 for why. The addin is presence-based, not declaration-based.

**Aggregating `pull_request` across two rulesets.** Once the review layer
lives in its own ruleset (`review.json`, applied independently of the core
3-file layout — see the `*-repo-governance` skills), a repository that has
opted in carries *two* active `pull_request` rules on its default branch:
the core one (squash-only) and the review layer's (squash-only + thread
resolution). GitHub aggregates same-type rules across active rulesets by
applying the most restrictive value per parameter (GitHub Docs, "About
rulesets", rule-layering section). The audit mirrors that: thread
resolution is an OR across every `pull_request` rule found, and
`allowed_merge_methods` is an intersection across every rule that
specifies it.

`required_status_checks`' actual check-name list (e.g. `MSRV (1.88)`,
`Format check`) is inherently project-specific (CI job titles) — the audit
reports those names for a human to read, but never machine-judges them
beyond confirming the rule type itself is present.

"Applies to the default branch" must accept both the `~DEFAULT_BRANCH` and
`~ALL` special ref-name values — a ruleset scoped to `~ALL` still
constrains the default branch, and at least one governed repo in the
account carries `copilot_code_review` exclusively through such a ruleset.

### charters (ADR-0013 + ADR-0016 + ADR-0017)

Reports which repositories lack a machine-checkable "why this repository
exists, and which Issues belong in it" charter, and (since ADR-0016/ADR-0017)
which repositories' documentation drifts from the fixed document canon.

**Why the schema changed (ADR-0016).** The original schema's `In:`/`Out:`
Scope labels and README-embedded Issue litmus had no precedent in
established README practice (standard-readme, GitHub's own docs, Art of
README) — a mechanical bulk application of that schema to a private
repository (see `docs/claude/public-publish-guard.md` for why this
document doesn't name it) produced README content those sources actively
warn against (timestamped history, Issue-number citations, long litmus
prose). ADR-0016 replaces it with a schema grounded in those sources.

**Why CONTRIBUTING.md got its own fixed schema, and "Issue litmus" is
retired (ADR-0017).** ADR-0016 only moved the litmus section out of README
and into CONTRIBUTING.md; it never grounded CONTRIBUTING.md's own structure
in anything, and the file that shipped as ADR-0016's own self-application
(`## Issue litmus` with `判定問:`/`採用例:`/`棄却例:` labels) mixed Japanese
and English in a file ADR-0016 Decision 4 fixes to a single English
original. A primary-source survey (GitHub Docs, opensource.guide,
`nayafia/contributing-template`; see ADR-0017) found no standard-readme-
equivalent spec for CONTRIBUTING.md, but converged on two headings across
every source: how to file an issue, and how to send a pull request. ADR-0017
fixes CONTRIBUTING.md to that pair plus an optional third, and retires the
self-invented "Issue litmus" vocabulary in favor of the judging-
question-plus-examples *content* under a plain `## Issues` heading — that
content shape has no precedent either (contributors are increasingly AI
agents, a case no source above addresses), so it stays, just renamed.

Judged items:

- `readme-missing` / `contributing-missing` — the file could not be
  fetched.
- `no-purpose-paragraph` — no non-blank paragraph follows the README's
  first `# ` heading.
- `purpose-mismatch` — the README's purpose sentence, normalized
  (whitespace collapsed, markdown emphasis/links stripped, trailing
  `.`/`。` dropped), does not literally equal the normalized GitHub
  description. Intentionally strict: the point is to force an explicit
  edit that keeps both in sync, not to approve near-misses.
- `readme-schema-incomplete-or-out-of-order` — the required headings
  (`## Install`, `## Usage`, `## Scope`, `## Development`, `## License`)
  are missing or not in that relative order. `## Background` is optional
  and, if present, must precede `## Install`.
- `readme-stray-heading:<names>` — a `##` heading outside the allowed set
  (`Background`, `Install`, `Usage`, `Scope`, `Development`, `License`).
- `litmus-not-migrated` — README still has a `## Issue litmus` heading
  (it belongs in CONTRIBUTING.md now, under the current `## Issues` name).
- `contributing-schema-incomplete-or-out-of-order` (ADR-0017) — the
  required headings (`## Issues`, `## Pull requests`) are missing or not in
  that relative order. `## Expectations` is optional and, if present, comes
  last.
- `contributing-stray-heading:<names>` (ADR-0017) — a `##` heading outside
  the allowed set (`Issues`, `Pull requests`, `Expectations`). A leftover
  `## Issue litmus` heading is caught here, not by a dedicated check — the
  retired vocabulary is simply not in the allowed set.
- `readme-cjk-present` / `contributing-cjk-present` (ADR-0017) — the
  document, after stripping fenced/inline code and URLs, has a CJK-script
  character count at or above `CJK_PRESENCE_THRESHOLD` (5). ADR-0016
  requires README/CONTRIBUTING to be a single English original, so unlike
  the retired two-script "mixing" heuristic this predecessor superseded, CJK
  presence alone is drift — Latin content does not need to co-occur. This
  only judges the two files this audit fetches, not every markdown file in
  the repository — the rest is each repository's own per-repo CI's job.
- `no-topics` — `repositoryTopics` is empty.
- `root-doc-not-allowlisted:<names>` — a root-level `*.md` file outside
  `README.md` / `CONTRIBUTING.md` / `CHANGELOG.md` / `AGENTS.md` /
  `CLAUDE.md` / `LICENSE*`.
- `claude-md-not-routed` / `claude-md-routed-but-agents-md-missing` —
  `CLAUDE.md` exists but does not start with an `@AGENTS.md` import line,
  or does but `AGENTS.md` itself is missing.
- `skills-not-routed:<names>` — an entry directly under `.claude/skills/`
  is a real directory (git mode other than `120000`/`40960` — REST's
  `git/trees` API returns the symlink mode as the octal string `"120000"`,
  GraphQL's `TreeEntry.mode` returns the same mode as the decimal `40960`;
  both are accepted, #259) instead of a symlink into
  `../.agents/skills/<name>`.

**Why judgement is by literal presence/match, never an LLM call.** Every
item above is decidable without reading for quality — a litmus test with a
weak judging question still passes; catching a weak-but-present litmus
test is a human review problem (the `repo-charter` skill's interview
step), not this audit's job.

### naming (ADR-0014 + ADR-0020)

Reports naming-class declaration and pattern conformance. Every repository
should carry exactly one `naming-*` GitHub topic (`naming-codename` /
`naming-descriptive` / `naming-pj` / `naming-site` — see ADR-0014 for the
class definitions), and the repository name should match that class's
pattern.

- `class-undeclared` — zero `naming-*` topics.
- `class-ambiguous` — two or more `naming-*` topics.
- `pattern-mismatch:<class>` — exactly one class declared, but the name
  doesn't match its pattern (e.g. `naming-pj` requires a `pj-` prefix).
- `class-unknown:<class>` — a `naming-*` topic outside the four defined
  classes.

`.github` is permanently exempt (GitHub reserves the name; there's no
class it could meaningfully declare). This audit never decides *which*
class a repository should declare — that's a human judgement call the
`github-audit-triage` skill surfaces from these findings (as of ADR-0020,
via **blind re-derivation**: a class + canonical name is derived from the
repository's contents with its actual name hidden, then compared back
against the real name).

**ADR-0020 closed vocabularies.** Beyond the lexical pattern above, three
classes draw their variable slot from a closed, repo-tracked vocabulary:

- `codename-not-registered` — a `naming-codename` repository whose name is
  not in the codename registry (`config/github-audit/codename-registry.tsv`
  plus a `~/.config/github-audit/codename-registry.local.tsv` overlay for
  PRIVATE repositories, which cannot be named in this PUBLIC repository's
  tracked files). Applies to **every** `naming-codename` declaration,
  regardless of when the repository was created — the registry's
  default-deny gate is meant to start now, not only for future repos.
- `species-unrecognized:<token>` — a `naming-descriptive` repository whose
  trailing `-<token>` is not in the species closed set
  (`config/github-audit/descriptive-species.tsv`). Only checked for
  repositories created after ADR-0020's cutoff (`created_at` grandfather,
  same idea as ADR-0007's no-retroactive-rename rule).
- `domain-unrecognized` — a `naming-site` repository whose name is not in
  the site domain closed set (`config/github-audit/site-domains.tsv` +
  local overlay). Same cutoff rule as species above.

The species closed set intentionally excludes action nouns (e.g.
`cleanup`, `migration`) — a completable action belongs to `naming-pj`, not
`naming-descriptive`. See ADR-0020's Context for the incident that
motivated this. It also excludes research-subject nouns (a phenomenon or
field name) — one word per subject would make the set grow without bound,
defeating the point of a closed vocabulary; a completed/frozen research
record uses `archive` regardless of subject (ADR-0020's Amendment).

### settings

Reports GitHub repository-settings drift against six baseline
expectations:

- `merge-not-squash-only` — squash merges not exclusively allowed (`gh repo
  list`).
- `delete-branch-on-merge-disabled` (`gh repo list`).
- `default-branch-not-main` (`gh repo list`).
- `wiki-enabled` / `projects-enabled` — unused GitHub features left on (`gh
  repo list`).
- `squash-title-not-pr-title` / `squash-message-not-blank` (ADR-0031) —
  the merge-title contract's foundation: `squash_merge_commit_title` must
  be `PR_TITLE` and `squash_merge_commit_message` must be `BLANK`, so the
  squash commit landing on the default branch is exactly the PR title with
  no extra body text. These two fields are **not** exposed by the
  Repository GraphQL type that `gh repo list --json` uses (confirmed
  2026-09-22 — `gh repo list --json squashMergeCommitTitle` errors with
  "Unknown JSON field"), so they come from one extra REST call per
  repository (`gh api repos/OWNER/REPO`), made only when the `settings`
  domain is actually requested. A repository whose REST call fails is
  reported drifted on these two tokens (fail-open to drift, the same
  convention `judge_rulesets()`'s per-ruleset REST fetch uses) — it is
  never silently treated as compliant.

### titles (ADR-0031)

Presence-detection for the PR-title commit-message contract's enforcement
mechanism — **not** a re-check of any individual open PR's title. That
distinction matters: the client guard (`config/claude/hooks/
pr-title-guard.sh`) and the required check (`.github/workflows/
pr-title.yml`) already judge individual titles; if this domain re-judged
them too, a repository could show `drifted` here while every open PR is
green, or vice versa, with no way to tell which layer to trust
(`docs/claude/pr-title-contract.md`).

Two checks, both informational about whether the *mechanism* exists:

- `pr-title-workflow-missing` — no `.github/workflows/pr-title.yml` caller
  workflow (checked by filename against the same `workflowsDir` GraphQL
  data the `renovate` domain reads, not by content — every repository is
  expected to name its caller `pr-title.yml` per the Stage 5 rollout
  template).
- `pr-title-check-not-required` — the active default-branch ruleset's
  `required_status_checks` does not include the `PR title` context (reuses
  `default_branch_rulesets()`, the same helper the `rulesets` domain uses).

Repositories with no `.github/workflows` at all are `not-applicable` —
there is no CI to register a required check against, the same convention
`renovate` uses.

### renovate

Reports Renovate config presence, but only for repositories where it would
plausibly matter: a dependency manifest (`Cargo.toml`, `package.json`,
`pyproject.toml`, or `go.mod`) *and* at least one `.github/workflows/`
file both present. Repositories without both are reported as
`not-applicable`, a fourth verdict alongside `ok`/`drifted`/`exempt` — they
are excluded from applicability, not silently judged and passed.

`flake.nix` does not count as a manifest: dotfiles' own Nix dependency
updates go through the separate `nix flake update` cadence (`docs/
operations.md`; ADR update-flake-lock tracking is #3), which is
orthogonal to Renovate. Mend App installation status is not checked —
GitHub's API does not expose it deterministically; that stays a manual
step documented in the relevant `*-repo-governance` skill.

## Usage

```console
$ github-audit
verdict=ok domain=rulesets repo=telepath
verdict=drifted domain=charters repo=<private-repo> missing=readme-schema-incomplete-or-out-of-order,litmus-not-migrated
verdict=drifted domain=naming repo=<private-repo> missing=class-undeclared
total: 170 finding(s) across 34 repo(s)
  ok=18
  drifted=100
  ungoverned=16
  exempt=2
  not-applicable=34
```

(Illustrative, not a literal transcript — see
[Private-repository specifics](#private-repository-specifics).)

Run a subset of domains: `github-audit naming settings`. Exit status is
`1` if any non-exempt/non-not-applicable finding is not `ok`, `0`
otherwise. `--json` prints the findings as a JSON array (one object per
repository, with a `domains` map) instead of the table. A ledger snapshot
(`{generated_at, repos: [...]}`) is always written to
`$XDG_STATE_HOME/github-audit/ledger.json`
(`GITHUB_AUDIT_STATE_DIR` overrides the directory).

### Exempting a repository

```
# repo<TAB>domain-or-*<TAB>exempt
example-coursework-repo	*	exempt
example-paper-repo	rulesets	exempt
```

in `$XDG_CONFIG_HOME/github-audit/overrides.tsv`
(`GITHUB_AUDIT_OVERRIDES_FILE` overrides the path). `*` exempts every
domain; a specific domain name exempts only that one. An exempt
repository/domain pair is reported as `exempt` and is never judged.

## Scope

In scope: read-only inventory + drift detection across the six domains
above. Out of scope, tracked for a follow-up (wrap-up inbox / #153):

- **Applying** a fix to a drifted/ungoverned repository — that's the
  `github-audit-triage` skill's job (decision loop: this audit → triage
  → human GO → applied → re-audit, ADR-0015), or the relevant
  `*-repo-governance` skill for settings/renovate/ruleset application.
- A "new repository created → baseline auto-applied" hook (#153).
- Scheduled/periodic execution (a systemd user timer, mirroring
  `git-audit-worktrees` — `docs/worktree-lifecycle.md`). This is a manual
  command today.
- Judging litmus-test or Scope-text *quality* — a present-but-weak litmus
  test still passes charters. Catching that is a human review problem
  (the `repo-charter` skill's interview step).
- The full-repository (every markdown file, not just README/CONTRIBUTING)
  language-mixing check — that's each repository's own per-repo CI
  (seeded by the `*-repo-governance` skill templates).

As of the ADR-0016 schema change, every owned repository reports
`drifted` under `charters` again — expected re-initialization, not a bug.
Adoption is incremental via `github-audit-triage`, repository by
repository.

## Private-repository specifics

Which repositories were found drifted, and any one repository's own
follow-up on its own issue tracker, are intentionally omitted from this
document — see `docs/claude/public-publish-guard.md` for why dotfiles
never names private/company repositories in its own tree.
