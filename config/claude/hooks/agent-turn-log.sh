#!/usr/bin/env bash
# agent-turn-log.sh — appends one JSONL line per Claude Code turn boundary
# (UserPromptSubmit / Stop) for a separate personal work-time evidence
# pipeline (a different repo, `daily-report`) to ingest later. This repo's
# only obligation is the output contract (docs/adr/0011) — the exact path
# and the exact field names below are load-bearing for that other tool.
#
# Registered under both UserPromptSubmit and Stop (home/modules/claude.nix);
# this script tells the two apart from `.hook_event_name` in the JSON on
# stdin rather than from argv, so one script + one command string serves
# both registrations (same "read hook_event_name, branch in a case" shape
# as herdr-claude-metadata.sh).
#
# Fail-open like every other hook here (ADR-0005 binary-existence gating):
# no jq on PATH, unreadable stdin, or a write failure (disk full, EACCES,
# missing dir) must never block the harness — always exit 0. Best-effort,
# not a gate.
#
# Privacy: the prompt text is written to exactly one place (OUT_FILE below)
# and nowhere else — never echoed, never put in a shell variable, never
# passed as a command-line argument. It stays inside jq's own JSON pipeline
# end to end (read from the stdin file, `--slurpfile`'d back in, embedded
# via jq's own escaping) for exactly that reason.
set -uo pipefail

command -v jq >/dev/null 2>&1 || exit 0

hook_input_file="$(mktemp "${TMPDIR:-/tmp}/agent-turn-log.XXXXXX" 2>/dev/null)" || exit 0
trap 'rm -f "$hook_input_file"' EXIT HUP INT TERM
cat >"$hook_input_file" 2>/dev/null || true

event="$(jq -r '.hook_event_name // empty' "$hook_input_file" 2>/dev/null)" || exit 0
case "$event" in
  UserPromptSubmit | Stop) ;;
  *) exit 0 ;;
esac

out_dir="${XDG_STATE_HOME:-$HOME/.local/state}/daily-report"
out_file="$out_dir/agent-events.jsonl"

mkdir -p "$out_dir" 2>/dev/null || exit 0
chmod 700 "$out_dir" 2>/dev/null || true
if [ ! -e "$out_file" ]; then
  (umask 077 && : >"$out_file") 2>/dev/null || exit 0
fi
chmod 600 "$out_file" 2>/dev/null || true

ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)" || exit 0
session_id="$(jq -r '.session_id // empty' "$hook_input_file" 2>/dev/null)"
cwd="$(jq -r '.cwd // empty' "$hook_input_file" 2>/dev/null)"

case "$event" in
  UserPromptSubmit)
    prompt_id="$(jq -r '.prompt_id // empty' "$hook_input_file" 2>/dev/null)"
    if [ -z "$prompt_id" ]; then
      prompt_id="$(date +%s%N 2>/dev/null || date +%s)-$$"
    fi
    jq -nc \
      --arg ts "$ts" \
      --arg session_id "$session_id" \
      --arg prompt_id "$prompt_id" \
      --arg cwd "$cwd" \
      --slurpfile payload "$hook_input_file" \
      '{
         kind: "prompt",
         agent: "claude-code",
         ts: $ts,
         session_id: $session_id,
         prompt_id: $prompt_id,
         cwd: $cwd,
         prompt: ($payload[0].prompt // "")
       }' >>"$out_file" 2>/dev/null || true
    ;;
  Stop)
    jq -nc \
      --arg ts "$ts" \
      --arg session_id "$session_id" \
      '{kind: "turn_end", agent: "claude-code", ts: $ts, session_id: $session_id}' \
      >>"$out_file" 2>/dev/null || true
    ;;
esac

exit 0
