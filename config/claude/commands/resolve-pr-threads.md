# GitHub PR Review Thread Resolution Command Specification

## Command Configuration

```yaml
custom_commands:
  resolve-pr-threads:
    description: "Query and resolve GitHub PR review threads via GraphQL API with commit traceability and per-thread triage"
```

## RFC 2119 Compliance

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this command specification are to be interpreted as described in RFC 2119.

## Command Behavior Requirements

This command MUST actively interact with GitHub to:

1. **List** review threads for the target PR, grouped by file path, with offending line snippet inline.
2. **Triage** each unresolved thread interactively via `AskUserQuestion` before any mutation.
3. **Apply** code changes for accepted threads and capture the commit SHA.
4. **Resolve** each non-deferred thread with a reply citing the commit SHA for full audit traceability.

The GitHub REST API does NOT support resolving or unresolving review threads. Claude MUST use the GraphQL API exclusively for all thread mutations.

## Required Usage Patterns

Claude MUST support the following usage patterns:

- `/resolve-pr-threads` — Auto-detect PR for the current branch; enter List mode only (no mutations).
- `/resolve-pr-threads 123` — List mode for PR #123 (no mutations).
- `/resolve-pr-threads 123 resolve` — Full triage + traceable resolve flow for PR #123.
- `/resolve-pr-threads 123 resolve THREAD_ID` — Single-thread triage + resolve (by `PRRT_`-prefixed node ID).
- `/resolve-pr-threads 123 verify` — Verify that cited commit SHAs exist in branch history before any mutations.

## Four-Phase Workflow

### Phase 1: List

Query all review threads (resolved and unresolved) via GraphQL:

```graphql
{
  repository(owner: "$OWNER", name: "$REPO") {
    pullRequest(number: $PR_NUMBER) {
      reviewThreads(first: 100) {
        nodes {
          id
          isResolved
          comments(first: 1) {
            nodes { body path line originalLine }
          }
        }
      }
    }
  }
}
```

**Rendering MUST:**
- Group threads by `path`, sorted alphabetically.
- Within each path, sort by `line` ascending.
- Display the thread node ID, `isResolved` status, and the first comment's body.
- Attempt to show the file:line excerpt using `git show HEAD:<path>` or `sed -n '<line>p' <path>`.

### Phase 2: Per-Thread Triage

For each **unresolved** thread (or a specific thread if `THREAD_ID` is provided), Claude MUST call `AskUserQuestion` with four options:

```
Question: "How do you want to handle this review comment?"
  (path:line — comment excerpt)

Options:
  accept  — Adopt the suggestion as-is; I will make the code change and commit.
  partial — Adopt part of the suggestion; I will describe what to change.
  reject  — Do not change code; add a reply explaining the rationale and resolve.
  defer   — Leave this thread unresolved; skip in this run.
```

For `partial`, Claude MUST ask a follow-up free-text question: "Describe which part of the suggestion to adopt."

For `defer`, Claude MUST ask a follow-up `AskUserQuestion` classifying the reason:

```
Question: "Why are you deferring this thread?"

Options:
  out-of-scope — The fix is valid but touches files/modules outside this PR's diff.
  other        — Just deferring for now; no issue should be filed.
```

`out-of-scope` and `other` are handled differently in Phase 4 (see below) — the
former files a GitHub issue automatically with no further confirmation, the
latter leaves the thread untouched. This split exists because "the fix is
right but out of scope" has no judgment call left to make — filing is the only
sane outcome, so it should not cost another round-trip through
`AskUserQuestion`.

Results MUST be collected before any mutations begin.

### Phase 3: Apply (accept / partial only)

For threads triaged as `accept` or `partial`:

1. Read the relevant source file(s).
2. Apply the code change following the suggestion (or the partial scope).
3. Stage and commit with a conventional commit message.
4. Capture the short SHA: `git rev-parse --short HEAD`.

If the change is already committed (user ran `/resolve-pr-threads` after fixing manually), Claude MUST ask: "Has this already been committed? If so, provide the commit SHA." and use the supplied SHA.

### Phase 4: Resolve

For each thread with decision `accept`, `partial`, or `reject`, run the idempotent 3-step mutation:

**Step 1 — Unresolve (unconditional; idempotent):**
```bash
gh api graphql -f query='mutation {
  unresolveReviewThread(input: {threadId: "$THREAD_ID"}) {
    thread { isResolved }
  }
}'
```

**Step 2 — Reply:**
```bash
gh api graphql -f query='mutation {
  addPullRequestReviewThreadReply(input: {
    pullRequestReviewThreadId: "$THREAD_ID",
    body: "$REPLY_BODY"
  }) {
    comment { id url }
  }
}'
```

Reply body MUST follow one of two templates (no freeform):
- **accept / partial:** `Addressed in <SHORT_SHA> — <one-line description of what changed>`
- **reject:** `Won't fix — <one-line rationale>`

**Step 3 — Resolve:**
```bash
gh api graphql -f query='mutation {
  resolveReviewThread(input: {threadId: "$THREAD_ID"}) {
    thread { isResolved }
  }
}'
```

`defer` threads with reason `other` receive no mutations.

`defer` threads with reason `out-of-scope` receive an automatic issue-filing
step — no additional `AskUserQuestion` confirmation, since the disposition
(file it) is mechanical once `out-of-scope` was chosen:

1. **File an issue** in the target repo: `gh issue create --title "<summary>" --body "<body>"`. The body MUST include the thread's permalink URL (`https://github.com/OWNER/REPO/pull/PR#discussion_rDDDDDDD`, derivable from the thread node), a one-paragraph summary of the original comment, and the defer rationale the user gave (if `partial`-style free text was provided when classifying). The body MUST end with the attribution footer required by this repo's `attribution-guard` (or the target repo's equivalent policy) unless `No-Attribution: <reason>` applies.
2. **Reply to the thread** (no unresolve/resolve — the thread was never resolved, so there is nothing to undo):
   ```bash
   gh api graphql -f query='mutation {
     addPullRequestReviewThreadReply(input: {
       pullRequestReviewThreadId: "$THREAD_ID",
       body: "$REPLY_BODY"
     }) {
       comment { id url }
     }
   }'
   ```
   Reply body template: `Filed as a separate issue: <ISSUE_URL> — <one-line reason>`.
3. The thread itself is **not** resolved — it remains `unresolved`/deferred, same as `other`. Only the reply and the new issue are new state.

## Implementation Requirements

### Auto-Detection

1. **Owner/repo** MUST be parsed from `git remote get-url origin`. Support both HTTPS (`https://github.com/OWNER/REPO`) and SSH (`git@github.com:OWNER/REPO.git`) forms. If not parseable, ask the user to supply `OWNER/REPO` explicitly.

2. **PR number** MUST be detected using the exact form:
   ```bash
   gh pr list --repo OWNER/REPO --head BRANCH --json number --jq '.[0].number'
   ```
   Where `BRANCH` is the output of `git rev-parse --abbrev-ref HEAD`. Reject early with an actionable message if the output is empty (no open PR for this branch).

### Reply Template Enforcement

Claude MUST NOT emit freeform replies. The reply body MUST match one of:
- `Addressed in <SHORT_SHA> — <description>` (accept / partial)
- `Won't fix — <rationale>` (reject)

Claude SHOULD generate the description by summarising the diff of the commit that addressed the thread.

### Idempotent Unresolve

`unresolveReviewThread` MUST be called unconditionally before every `addPullRequestReviewThreadReply`. If the thread is already unresolved it is a no-op. This removes a conditional branch and ensures replies are always visible (replies to already-resolved threads may be collapsed in the GitHub UI).

### Verify Mode

When invoked with `verify`, Claude MUST:
1. For each thread in scope, identify the cited commit SHA (from a previous run's reply or from user input).
2. Run `git log --oneline <SHA>` to confirm the SHA is reachable from the current branch.
3. If any SHA is missing or not reachable, abort the entire run — do NOT partial-resolve. Report all missing SHAs before aborting.

### Scope Policy

`/resolve-pr-threads` MUST NOT silently expand scope beyond the review threads listed for the target PR. If related but unflagged issues are observed during code inspection, Claude SHOULD surface them in the post-run summary as "Related (not in scope)", clearly separated from the threads being resolved. A `--scan-related` flag is planned for a future version.

This is a distinct concern from the `out-of-scope` defer reason above: this
policy covers issues **not flagged by any review comment** that Claude
happens to notice while inspecting code — those go to the summary only,
never auto-filed. The `out-of-scope` defer reason instead covers a review
comment that **was** flagged, where the triage itself concluded the valid
fix exceeds this PR's diff — that case has no judgment call left, so it
auto-files (see Phase 4).

## Required Response Format

### List Output

```
PR #123: <PR title>
<N> review threads (<M> unresolved, <K> resolved)

crates/telepath-host/src/lib.rs
  line 183  [PRRT_kwDO...WsyL]  ⬤ UNRESOLVED
    "Consider adding a MAX_FRAME_SIZE guard in the receive loop."
    ↳ `let buf = vec![0u8; size];  // line 183`

  line 221  [PRRT_kwDO...WsyO]  ⬤ UNRESOLVED
    "Validate resp.kind and payload size before deserialization."
    ↳ `let resp = postcard::from_bytes(raw)?;  // line 221`

crates/telepath-wire/src/lib.rs
  line 9    [PRRT_kwDO...Hd3p]  ✓ RESOLVED
    "Doc comment says rzCOBS but implementation uses plain COBS."
    ↳ `// Uses rzCOBS framing for upstream packets  // line 9`
```

### Triage Summary (after Phase 2)

```
Triage decisions:
  PRRT_...WsyL  crates/telepath-host/src/lib.rs:183  → accept
  PRRT_...WsyO  crates/telepath-host/src/lib.rs:221  → partial  (validate kind only, not size)
  PRRT_...Hd3p  crates/telepath-wire/src/lib.rs:9    → reject   (already corrected in #8)
  PRRT_...Kq2m  crates/telepath-host/src/lib.rs:340  → defer    (out-of-scope: touches telepath-wire, not this PR)
```

### Resolution Log (after Phase 4)

```
Resolving with commit traceability...

  ✓ PRRT_...WsyL  crates/telepath-host/src/lib.rs:183
    → Addressed in c491f70 — added MAX_FRAME_SIZE guard in receive loop

  ✓ PRRT_...WsyO  crates/telepath-host/src/lib.rs:221
    → Addressed in c491f70 — validate resp.kind before deserialization

  ✓ PRRT_...Hd3p  crates/telepath-wire/src/lib.rs:9
    → Won't fix — corrected in PR #8 (commit 0bf7994); doc updated there

  ⬤ PRRT_...Kq2m  crates/telepath-host/src/lib.rs:340  (still unresolved)
    → Filed as a separate issue: https://github.com/OWNER/REPO/issues/42 — touches telepath-wire, not this PR

3/4 threads resolved with commit references. 1 deferred (1 filed as #42, 0 left untouched).

Note: --scan-related (surface related but unflagged issues) is planned for a future version.
```

## Error Handling Requirements

Claude MUST handle the following scenarios:

| Scenario | Required Behaviour |
|---|---|
| No open PR for current branch | Print actionable message; suggest `gh pr create` or specify PR number explicitly. |
| Owner/repo not parseable from remote | Ask user to supply `OWNER/REPO` explicitly before proceeding. |
| GraphQL mutation error | Log the full API response; abort the loop immediately (do not silently skip remaining threads). |
| `verify` mode: SHA not reachable | Abort the entire run; list all missing SHAs; do not partial-resolve. |
| Empty thread list | Report "0 threads found for PR #N" and exit cleanly. |
| API rate limit or network error | Report the error with the rate-limit reset time if available; do not retry silently. |

## Integration with Related Commands

Claude SHOULD coordinate with:
- `/review` and `/code-review` — to identify issues before running this command.
- `/pr-review-toolkit:review-pr` — comprehensive review including test and type analysis; run before triage.
- `/commit-commands:commit` — for committing code changes in Phase 3 following project commit conventions.
