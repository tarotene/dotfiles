#!/usr/bin/env bash
# apply-rulesets.sh — Create the GitHub Rulesets: Security / Quality / Workflow
# (core layer, always applied) and Review (review layer, opt-in addin —
# ADR-0021 in tarotene/dotfiles) for a repository that does NOT belong to
# any of the three ecosystem-specific *-repo-governance skills (#337).
#
# Language-agnostic: unlike rust/typst/astro-site-governance's own
# apply-rulesets.sh, this one substitutes only __OWNER__/__REPO__/
# __DEFAULT_BRANCH__ — no MSRV/NODE_VERSION/TYPST_VERSION-style ecosystem
# placeholder, because a repository dispatched here (#337's github-
# rulesets-apply `core` type) is by definition not one of the three known
# ecosystems, so this script cannot assume anything about its CI job names.
# Its Quality ruleset (rulesets/quality.json, sibling of this file) reflects
# that: the only required status check is `PR Title / PR title` (ADR-0031,
# the two-part concatenation a workflow_call-triggered job reports — see
# rulesets/quality.json's own note) — no
# build/test job name is assumed. A repository that later gains a matching
# ecosystem skill should migrate to that skill's own apply-rulesets.sh
# (which can safely require its language-specific CI jobs) instead of
# staying on this generic baseline forever.
#
# Everything downstream of the substitution (review-layer removal, the
# create/reconcile loop) is identical to the three ecosystem skills and
# lives in the shared _rulesets-apply-core.sh (#388) in this same
# directory — no relative `../../` hop needed since this file already
# lives in repo-governance-common/scripts/.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILL_DIR="$SCRIPT_DIR/.."
RULESETS_DIR="$SKILL_DIR/rulesets"

# shellcheck source=./_rulesets-apply-core.sh
source "$SCRIPT_DIR/_rulesets-apply-core.sh"

OWNER=""
REPO=""
DEFAULT_BRANCH="main"
DRY_RUN=false
WITH_REVIEW=false
REMOVE_REVIEW=false
RECONCILE=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --owner)           OWNER="$2";           shift 2 ;;
    --repo)            REPO="$2";            shift 2 ;;
    --default-branch)  DEFAULT_BRANCH="$2";  shift 2 ;;
    --with-review)     WITH_REVIEW=true;     shift ;;
    --remove-review)   REMOVE_REVIEW=true;   shift ;;
    --reconcile)       RECONCILE=true;       shift ;;
    --dry-run)         DRY_RUN=true;         shift ;;
    *)                 echo "Unknown option: $1"; exit 1 ;;
  esac
done

[[ -z "$OWNER" ]] && echo "ERROR: --owner is required" && exit 1
[[ -z "$REPO"  ]] && echo "ERROR: --repo is required"  && exit 1

for cmd in gh jq; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "ERROR: '$cmd' not found"; exit 1; }
done

# Escape for sed replacement (handles &, /, \)
sed_escape() { printf '%s' "$1" | sed 's/[&/\]/\\&/g'; }

E_OWNER="$(sed_escape "$OWNER")"
E_REPO="$(sed_escape "$REPO")"
E_BRANCH="$(sed_escape "$DEFAULT_BRANCH")"

# Apply placeholder substitutions to a ruleset JSON file, output to stdout.
process_ruleset() {
  local file="$1"
  sed \
    -e "s/__OWNER__/${E_OWNER}/g" \
    -e "s/__REPO__/${E_REPO}/g" \
    -e "s/__DEFAULT_BRANCH__/${E_BRANCH}/g" \
    "$file"
}

governance_apply_rulesets_main "$RULESETS_DIR"
