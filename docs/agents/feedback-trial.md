# Bounded Codex feedback trial

A separate collector reads saved Codex conversations and stores observation
candidates locally. Normal SessionStart and Stop paths never inject trial
instructions, create per-turn checks, or force the conversation to continue.
The configured report conversation handles wording, owner approval, posting,
and synthesis. Collection never resumes that conversation automatically.
Claude and Copilot remain on their existing paths.

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
in Asia/Tokyo. State remains under `claude/wrapup/trial` in the wrap-up state
root (`WRAPUP_STATE_DIR`, then `XDG_STATE_HOME`, then `~/.local/state`), in a
mode-0700 directory. Destinations, thread identifiers and conversation content
stay in private runtime state. Keep the configuration until pending observations
and final production are complete.

The owner authorized ongoing observation posting for the current trial on
2026-10-28. Its private declaration records that authorization in
`posting_authorization`. Owner impressions must come from actual owner
statements, and final script and visual review still require owner approval.

## Differential collection

`feedback-trial collect --dry-run` reads the available source intervals without
calling the model or saving state. `feedback-trial collect` processes completed
or interrupted intervals from normal and archived Codex rollouts, bounded by
the trial start and exclusive end. It excludes the report conversation, its
own analysis, child agents and hook continuations. Short owner answers are read
with their surrounding context, rather than discarded by length.

Finite batches retain the unread remainder. Each accepted result saves both
its candidate (or explicit no-candidate decision) and source position together.
Existing checks and source references prevent duplicate collection. No new
input means no model call. Failures, invalid JSON and interrupted execution
leave the affected input available for retry; completed batches remain saved.
A separate collector lock prevents concurrent collection without holding the
shared trial-state lock during inference.

Each fragment includes numbered source records and a bounded preceding context
with its own source line list. References must belong to the current fragment or
that supplied context. Model-returned fragment IDs are constrained to the input
IDs in the schema and checked again before saving. Recovery under the same turn
ID preserves fragment numbering; an incomplete final JSONL record waits for its
newline. Malformed completed records produce a diagnostic without consuming input.

A well-formed analysis that cannot be used (fragment decisions omitted or duplicated, or an
observation citing lines outside its fragment and context) is *rejected*, not a service
failure (#1134). The collector keeps the pending input, records the reason in
`collector.last_error` (shown as `collector_error=true` in the trial report and blocking
finalization), prints `<reason>; pending state retained` to stderr (the journal), and exits 0,
so the periodic `feedback-trial-collect.service` does not turn `failed` and make the systemd
user session `degraded` on every tick. The next run retries the same input and clears
`last_error` once it succeeds. Failed model execution, timeouts and invalid JSON still exit
non-zero.

Finalization takes the collector lock and rejects unread or unfinished input and
unresolved collection errors. The collector also checks finalization before
committing results, so stopping the trial cannot silently discard its backlog.

Analysis uses the native `~/.local/bin/codex`, rather than the interactive
credential launcher, via `exec --ephemeral --output-schema`. The service sets
`FEEDBACK_TRIAL_CODEX_BIN` to that native path. A private analysis `CODEX_HOME`
links the existing authentication file; ordinary Codex configuration stays
writable and unchanged. The collector inherits model and reasoning effort,
while disabling shell, web, MCP, apps and plugins, with a read-only sandbox and
approval policy `never`. GitHub credentials are removed from the subprocess
environment. This is a restricted analysis invocation, not a guarantee that
Codex exposes no tools. Returned JSON is validated as candidate data and is
never executed.

A missing or disabled declaration causes no model invocation. Expiry stops
selection of new outside-period input but allows collection of unread
inside-period intervals. Finalization or disabling the trial stops collection.
Errors go to the service journal and collector status, without notifying normal
conversations.

## Report conversation and approval

`feedback-trial list` is a read-only view: it creates no lock, changes no file
permissions, and saves no state. The report conversation gets only counts and
a CLI hint on start/resume; `feedback-trial procedure` prints the full procedure
when needed. Plan mode permits reading and reviewing candidates. Switch to
Default for state changes or posting. Waiting for owner approval or encountering
a write failure never forces continuation.

Commands use the existing check IDs, including IDs created before collection
moved out of normal conversations:

- `none ID`: mark an unchecked legacy item as having no candidate.
- `dismiss ID 'verbatim owner rejection'`: close an owner-rejected draft.
- `draft ID /absolute/body.md`: store wording and invalidate earlier approval.
- `approve ID /absolute/body.md 'verbatim owner answer'`: approve that exact
  wording using an answer actually obtained from the owner.
- `post ID`: Default mode only, with the trial's standing authorization or
  exact-body approval. List
  issue comments, reuse the unique marker, post only if absent, then read back
  the exact body and URL. Failure retains pending state.
- `applied ID FULL_SHA`: report conversation only. Read the issue again and
  verify the exact comment, then verify the saved script commit contains both
  marker and URL before acknowledging application.
- `finalize FULL_SHA`: report conversation only, after expiry and completion of
  pending observations, script review and visual review. Then set `enabled` to
  false in the local declaration.

Only the report conversation edits its worktree. It re-reads all pending checks
before ending. No transport service or automatic report resume is required.
Ambiguous posting failures recover by listing comments before retry. Conflicting
bodies or duplicate remote markers require reconciliation.

Token snapshots can include intervening ordinary work, so they do not measure
pure collection overhead. The report rollout supplies report-only token use.
Missing usage is unknown, never zero; do not sum overlapping intervals or
convert tokens into money or quota percentages without supporting data.

## Deployment, migration and recovery

`home/modules/agents.nix` declares
`dotfiles.feedbackTrial.collector.enable`, default false. Only
`home/hosts/arcturus.nix` enables it. The Linux user timer runs at minute 00 and
30 with `Persistent = true`; the service timeout is ten minutes. The
`feedback-trial-collect` node and timer pointer register this automatic execution
in `pipeline-map.toml`.

Before applying, stop the collector and save a private snapshot of its current
state. This preserves old IDs, drafts, approvals, posted URLs, no-candidate
records and applied commits. The schema adds fields while accepting old state;
there is no reset or re-keying migration. On the target host:

```sh
systemctl --user stop feedback-trial-collect.timer feedback-trial-collect.service
```

On the first deployment the units do not exist yet; systemd's unit-not-found
message in this step means there was no collector to stop. Do not ignore other
errors. Then copy the existing state directory to a dated sibling backup:

```sh
trial_state_base="${WRAPUP_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}}"
trial_backup_path="${trial_state_base}/claude/wrapup/trial.backup-$(date -u +%Y%m%dT%H%M%SZ)"
cp -a -- "${trial_state_base}/claude/wrapup/trial" "$trial_backup_path"
```

Confirm the backup contains the same files with `diff -r` before continuing.
Use the newly built binary to run `collect --dry-run` and `list`; inspect the
selected intervals and current counts without changing state. Keep the backup
outside the public repository. Do not rely on counts recorded in an earlier
session.

Apply from this worktree with `hms .`. Read the installed units and timer state:

```sh
systemctl --user cat feedback-trial-collect.service feedback-trial-collect.timer
systemctl --user list-timers --all feedback-trial-collect.timer
systemctl --user start feedback-trial-collect.service
systemctl --user show feedback-trial-collect.service -p Result -p ExecMainStatus
journalctl --user -u feedback-trial-collect.service -n 50 --no-pager
~/.claude/hooks/feedback-trial list
```

The installed service must run `feedback-trial collect`, use native Codex and
show a ten-minute timeout. The timer must show the half-hour calendar and
persistence. A successful service exit alone does not prove useful collection:
read the candidate source references and state back, verify existing approvals
and posted records survived, and confirm the report conversation can read them.
Check an ordinary conversation through actual SessionStart and Stop to confirm
trial output and new per-turn checks are both absent. Changed hook commands may
require the owner's trust via Codex `/hooks`; never manufacture trust hashes.

After a failure, inspect `journalctl` and `list`, repair the reported cause,
then start the same service again. An unprocessed batch retries with its source
references; do not delete state to force a retry. To roll back, stop both units,
restore the backup to the same private state path, and apply the prior
home-manager generation. Read the restored records before restarting collection.
After finalization, disable the local declaration; disabling the Nix option
also removes the timer on the next activation.

Verify changes with `cargo test -p feedback-trial -p session-wrapup -p pipeline-map`,
`cargo clippy -p feedback-trial -p session-wrapup --all-targets -- -D warnings`,
`nix fmt`, and the target host's activation build. Isolated fixture tests and
native Codex PoC results are distinct from the live timer/report read-back.

On 2026-10-27, live `collect --dry-run` found 1,026 fragments from 32
conversations. The pre-migration read-back contained 86 checks (7 unchecked,
13 awaiting owner, 1 awaiting post, 17 verified posts, 48 no-candidate).
The first 24 accepted fragments saved 6 candidates; every existing check field
still matched the byte-preserved backup. The native analysis reported
`approval: never` and `sandbox: read-only`, shared the authentication symlink,
and saved zero rollouts. Normal deployed `session-wrapup start` and `stop`
both returned 0, emitted no trial context, and created zero checks.
`feedback-trial list` succeeded inside the normal read-only sandbox.

Live analysis also exercised two failure paths: an incorrectly copied fragment
ID and a citation to supplied preceding context. Rejected results advanced no
positions. Input-ID schema enums and explicit context source-line validation
fixed these causes, and retry processed the retained intervals. The remaining
backlog is intentionally drained over successive bounded runs. Timer/service
state was read back using `systemctl --user show` and `cat`: active half-hour
timer, persistent catch-up, native Codex path, and ten-minute service timeout.
Simultaneous normal model use succeeded; an actual simultaneous authentication
refresh was not observed. The isolated retry/credential tests cover failures,
but do not establish native cross-process refresh serialization.

## Sources and decisions

- Existing means: extend `crates/feedback-trial`, its shared pending state,
  approval transitions and remote read-back, at baseline
  `838dcadd7bb5096f77aa1e05bbf108fc1fff87a0`, retrieved 2026-10-27.
  Difference: source-position collection replaces per-turn hook collection;
  report-side operations retain old IDs and exact-body approval.
- dotfiles, [promotion-detect timer](../../home/modules/promotion-detect.nix),
  baseline `838dcadd7bb5096f77aa1e05bbf108fc1fff87a0`, retrieved 2026-10-27.
  Existing means: extend the home-manager user timer pattern and pipeline map.
  Difference: opt in on one host, every half hour, with a ten-minute timeout.
- OpenAI, Codex 0.164.0, fixed source
  `6aaac694803004a63c6f393d153c96d22521a6b9`, publication date unconfirmed,
  retrieved 2026-10-27:
  [ephemeral execution](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/exec/src/lib.rs#L1440)
  and [session persistence](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/core/src/session/session.rs#L1064).
  Existing means: native schema-constrained ephemeral exec, no independent
  inference client. Difference: use an isolated analysis home and validate
  candidate data before committing source positions. The 2026-10-27 PoC used
  synthetic input and returned schema-valid JSON with exit 0 and no saved
  rollout; live conversation collection and simultaneous auth refresh require
  deployment verification.
- OpenAI, *Hooks*, <https://learn.chatgpt.com/docs/hooks>, retrieved 2026-10-26.
  Fixed source:
  [Stop event](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/hooks/src/events/stop.rs#L294)
  and [hook runtime](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/core/src/hook_runtime.rs#L996).
  `permission_mode` describes approval policy; the rollout's
  `turn_context.collaboration_mode.mode` supplies Plan/Default state.
  Unknown mode blocks posting.
- OpenAI, Codex 0.164.0, same fixed commit, retrieved 2026-10-27:
  [same-turn recovery contract](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/protocol/src/turn_input.rs#L101)
  and [regular task start](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/core/src/tasks/regular.rs#L48).
  Difference: the collector retains fragment numbering across recovery rather
  than interpreting a repeated task-start record as a new source turn.
