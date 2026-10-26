# Bounded Codex feedback trial

The existing wrap-up hooks optionally collect observations independently of the
PR retrospective. No trial is enabled by the distributed configuration. Claude
and Copilot remain on their existing paths.

## Local declaration

`$XDG_CONFIG_HOME/codex/feedback-trial.json` (default `~/.config/...`) owns the
private values. `WRAPUP_TRIAL_CONFIG` overrides the path for isolated tests.

```json
{
  "enabled": true,
  "start": "2026-10-25T15:00:00Z",
  "end_exclusive": "2026-11-01T15:00:00Z",
  "repository": "OWNER/REPO",
  "issue": 1,
  "report_thread": "THREAD-ID",
  "report_worktree": "/absolute/report/worktree",
  "script": "relative/script.md"
}
```

Dates are UTC instants: the example represents October 26–November 1 inclusive
in Asia/Tokyo. State is local, under `claude/wrapup/trial` in the existing wrap-up
state root, in a mode-0700 directory. Keep one trial configuration until its
pending items and final report are complete. Never copy feedback into public
configuration or issue bodies.

For a trial whose owner has requested automatic observation posting, add
`"posting_authorization": "<verbatim owner authorization>"` to the local
configuration. Omission preserves per-observation approval; an empty value is
invalid. This authorization is scoped to the configured trial, not Slack or
other outbound messages. Observations may be posted without another question;
owner impressions must still come from the owner, and final script/visual review
still requires approval. Plan mode continues to prohibit posting.

## Execution contract

`feedback-trial list` shows checks. Stop creates one check per normal
turn; an open candidate takes precedence over new collection. Hook continuations
reuse the check. Session start/resume recovers pending checks. Each attempt is
bounded to three forced continuations per normal turn/resume; exhaustion is
reported, not acknowledged. An owner-approval reminder is emitted once per new
input/resume; waiting then ends normally without revising the draft or approval.
After the end, existing checks and final production
remain due. No timer runs while every session is stopped.

Once `post` has verified the remote body and marker, the observation remains in
the shared pending state for the report thread. The originating thread can stop,
resume and collect later observations without waiting for a thread-message tool
or the report's saved commit. Report start/resume and Stop recover the entire
pending set. An interrupted report remains pending until that report resumes;
this never marks its work complete.

The procedure printed by the hook owns the agent instructions. Commands are:

- `none ID`: no candidate, only from unchecked.
- `dismiss ID 'verbatim owner rejection'`: close an owner-rejected draft without
  posting. This is distinct from inventing approval or silently forgetting it.
- `draft ID /absolute/body.md`: store proposed wording and clear earlier approval.
  With configured posting authorization, record that authorization and queue the
  draft for posting; otherwise wait for the owner's per-observation answer.
- `approve ID /absolute/body.md 'verbatim owner answer'`: accept only the same
  wording. The agent must actually obtain this answer; the CLI cannot decide
  whether natural-language approval was sincere or applicable.
- `post ID`: Default mode only. A previously waiting draft can use newly configured
  posting authorization. Otherwise require the owner's exact-body approval.
  List all issue comments, reuse the unique marker,
  post if absent, then verify the exact body and URL. Failure retains state.
- `applied ID FULL_SHA`: report thread only. Read the issue again and verify
  the exact comment. Verify the saved script in the configured worktree has
  both marker and URL before acknowledgement. Report synthesis is the agent's
  responsibility, not substring validation's responsibility.
- `finalize FULL_SHA`: report thread only, after expiry and pending completion.
  Complete script/storyboard/visual review before invoking it. Then set enabled
  to false in the local declaration.

Only the report thread edits the report worktree. The local pending set is the
handoff: no message acknowledgement or second producer is required. If a
thread-message tool is available, an optional notification sends only opaque
check IDs, at most once per normal input/resume. The receiver resolves URLs and
paths from local state/config, so private references never cross that message.
A report already processing a batch re-reads the pending set before ending.
Duplicate requests reuse the marker already in the script.

The state transition and remote read-back run under the existing file lock, so
parallel senders cannot lose state or create duplicate comments. An ambiguous
post failure is recovered by listing comments before retry. A conflicting body
or duplicate remote marker requires reconciliation and is never auto-completed.

Token snapshots bracket collection work. They can include intervening ordinary
work and are not a pure-overhead measurement. The report thread's rollout is the
source for report-only token use. Missing usage is unknown, never zero; do not
sum overlapping intervals or equate tokens with money/quota percentages.

## Sources and decisions

- Existing means: extend `crates/wrapup-stop-gate`, at baseline `df5893c`:
  `src/lib.rs` lock/SessionStart/Stop and `src/retro.rs` bounded retries. No
  independent hook or duplicated retrospective state machine. Retrieved
  2026-10-26. Difference: per-normal-input collection without a PR prerequisite.
- Handoff decision: use the existing shared pending state and report-side
  recovery at baseline `5d499a9a06d5c4c4e3c76ec870f5760be57164ab`
  (`src/trial.rs:213–220`, `docs/claude/feedback-trial.md:62–65`), retrieved
  2026-10-26. Existing means: extend that state owner; add no transport service.
  Difference: a verified post no longer blocks the originating thread on a
  delivery hint. Report acknowledgement still requires the saved commit and
  remote read-back. Source: [Issue #1037](https://github.com/tarotene/dotfiles/issues/1037).
- OpenAI, *Hooks*, <https://learn.chatgpt.com/docs/hooks>, retrieved 2026-10-26.
  Fixed source: OpenAI Codex `6aaac694803004a63c6f393d153c96d22521a6b9`,
  [Stop](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/hooks/src/events/stop.rs#L294)
  and [hook runtime](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/core/src/hook_runtime.rs#L996).
  `permission_mode` describes approval policy, not Plan mode. We inspect the
  rollout's `turn_context.collaboration_mode.mode` (observed locally on
  2026-10-26); unknown mode blocks posting. Stop's `turn_id` and
  `stop_hook_active` bound continuation; isolated CLI tests exercise that contract.

## Verification and deployment

Run `cargo test -p wrapup-stop-gate` and
`cargo clippy -p issue-inbox --all-targets -- -D warnings`.
Apply with the repository's `hms .` flow. Read registrations and trust back
afterward; the command changes need the owner's trust if Codex reports them
untrusted. Do not manufacture trust hashes. A real cross-session round trip
still requires an owner-approved observation and is distinct from isolated
transport/commit tests.
