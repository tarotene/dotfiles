# github-audit-charters: cross-repository README charter drift audit

Read-only audit: reports which of tarotene's owned repositories lack a
machine-checkable statement of "why this repository exists, and which
Issues belong in it" — the *charter* — without editing anything.
Sibling of [`github-audit-rulesets`](github-audit-rulesets.md); same
placement rationale (this tool lives in dotfiles, its ledger does not —
see that document's "Why this lives in dotfiles" section, which applies
here unchanged).

## Why this exists

A survey of the repositories open in Herdr (2026-09-16) found that every
one already had a GitHub description and a README purpose statement — the
part that looked missing at a glance was, in fact, present everywhere.
What was actually missing, across the board, was a **litmus test an agent
or a human could apply to a candidate Issue**: "does this belong in this
repository, or not". One private repository's open Issue tracker already
showed the cost of that gap: an Issue proposing an iterative,
multi-step automation loop directly contradicted that repository's own
README, which stated its one LLM-assisted step is consulted exactly once
and never re-enters a loop — the Issue tracker and the README had already
drifted apart, and nothing had ever caught it. (This repository is
private; per `docs/claude/public-publish-guard.md`, dotfiles does not
name private/company repositories in its own tree.)

The fix is not "write more prose" — prose was not the missing ingredient.
It is a **fixed, three-heading schema** that a script can check for
presence and a person or agent can apply to a specific Issue without
re-deriving the repository's intent from scratch every time.

## The charter schema

Enforced by the [`repo-charter`](claude/repo-charter.md) skill, checked by
this audit:

1. **Purpose sentence** — the first sentence of the first paragraph
   following the README's H1. This is the same text as the GitHub
   repository description; the description is a mirror, not an
   independent second copy. README is the source of truth (Web UI and any
   agent reading the repository both land on the same text).
2. **`## Scope`** — an In / Out bullet list.
3. **`## Issue litmus`** — one or two judging questions plus one or two
   accepted and one or two rejected examples. Realistic examples make the
   litmus test usable by pattern-matching, not just by re-reading the
   purpose sentence and guessing.
4. **At least one GitHub topic.**

`CONTEXT.md` / `vision.md` / other detail documents are not replaced —
they stay linked from the README as elaboration. The charter block is
deliberately short: long enough to decide a specific Issue, short enough
that nobody skips reading it.

## Why judgement is by literal presence/match, never an LLM call

Each of the four checks below is decidable without judgement:

- `readme-missing` — the README fetch (`gh api .../readme`) failed.
- `no-purpose-paragraph` — no non-blank paragraph follows the first `# `
  heading.
- `purpose-mismatch` — the README's purpose sentence, normalized
  (whitespace collapsed, markdown emphasis/links stripped, trailing
  `.`/`。` dropped), does not literally equal the normalized GitHub
  description. This is intentionally strict: the point is to force an
  explicit edit that keeps both in sync, not to approve near-misses. A
  repository whose polished description differs in wording from its
  README is reported as drifted until one is made to mirror the other —
  that friction is the mechanism, not a bug.
- `no-scope-section` / `no-issue-litmus-section` — the literal heading
  (`## Scope` / `## Issue litmus`) is absent.
- `no-topics` — `repositoryTopics` is empty.

None of this requires reading the litmus test's *content* for quality —
only for presence. A litmus test with a weak judging question still
passes this audit; catching a weak-but-present litmus test is a human
review problem (the repo-charter skill's interview step), not this
audit's job. This mirrors `github-audit-rulesets`' choice to judge by
rule-type union rather than semantic content.

## Usage

```console
$ github-audit-charters
verdict=ok repo=dotfiles lang=Shell
verdict=drifted repo=<private-repo> lang=Rust missing=purpose-mismatch,no-scope-section,no-issue-litmus-section,no-topics
verdict=drifted repo=<private-repo> lang=Python missing=readme-missing,no-topics
total: 34 repo(s) - ok=1 drifted=33 exempt=0
```

(Illustrative — `dotfiles` and the two `<private-repo>` placeholders
above are not a literal transcript of any one run; see
[Private-repository specifics](#private-repository-specifics) below for
why real output is not pasted here verbatim.)

Exit status is `1` if any non-exempt repository is `drifted`, `0`
otherwise. `--json` prints the same findings as a JSON array. A ledger
snapshot (`{generated_at, repos: [...]}`) is always written to
`$XDG_STATE_HOME/github-audit-charters/ledger.json`
(`GITHUB_AUDIT_CHARTERS_STATE_DIR` overrides the directory).

### Exempting a repository

Forks, coursework, and archived-in-spirit repositories are never expected
to carry a charter. List them, one per line, in
`$XDG_CONFIG_HOME/github-audit-charters/overrides.tsv`
(`GITHUB_AUDIT_CHARTERS_OVERRIDES_FILE` overrides the path):

```
# repo<TAB>exempt
example-coursework-repo	exempt
```

An exempt repository is reported as `exempt` and its charter is never
inspected.

## Scope

In scope: read-only inventory + drift detection over the four checks
above. Out of scope, tracked for a follow-up:

- **Applying** a charter to a drifted repository — a manual job for the
  `repo-charter` skill.
- Judging litmus-test *quality* (a present-but-weak judging question
  passes; see above).
- Cross-checking a specific open Issue against its repository's litmus
  test at triage time (an Issue-creation-time gate would need this; not
  built yet — see `docs/claude/repo-charter.md`'s Scope section).
- Per-repository CI self-check (each repo would need its own workflow
  step; this audit is the cross-repository substitute for now).
- Scheduled/periodic execution — a manual command today, like
  `github-audit-rulesets`.

As of this writing, this schema is brand new: every owned repository
reports `drifted` (no repository has adopted the charter yet). That is
the expected starting state, not a bug in the audit. Adoption is
incremental, repository by repository, as each is next touched — see
`docs/claude/repo-charter.md` for the pilot adoption's design notes.

## Private-repository specifics

Which repositories were found drifted, and any one repository's own
follow-up on its own issue tracker, are intentionally omitted from this
document — see `docs/claude/public-publish-guard.md` for why dotfiles
never names private/company repositories in its own tree.
