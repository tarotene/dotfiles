#!/usr/bin/env bash
# apply-rulesets.sh — Create the GitHub Rulesets: Security / Quality / Workflow
# (core layer, always applied) and Review (review layer, opt-in addin —
# ADR-0020 in tarotene/dotfiles). Skips rulesets that already exist by name.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILL_DIR="$(dirname "$SCRIPT_DIR")"
RULESETS_DIR="$SKILL_DIR/rulesets"

OWNER=""
REPO=""
DEFAULT_BRANCH="main"
MSRV="1.88"
MSRV_FULL="1.88.0"
CANONICAL_CRATE=""
CLI_CRATE=""
DRY_RUN=false
WITH_REVIEW=false
REMOVE_REVIEW=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --owner)            OWNER="$2";           shift 2 ;;
    --repo)             REPO="$2";            shift 2 ;;
    --default-branch)   DEFAULT_BRANCH="$2";  shift 2 ;;
    --msrv)             MSRV="$2";            shift 2 ;;
    --msrv-full)        MSRV_FULL="$2";       shift 2 ;;
    --canonical-crate)  CANONICAL_CRATE="$2"; shift 2 ;;
    --cli-crate)        CLI_CRATE="$2";       shift 2 ;;
    --with-firmware)                          shift ;;  # accepted, not used here
    --with-review)       WITH_REVIEW=true;    shift ;;
    --remove-review)     REMOVE_REVIEW=true;  shift ;;
    --dry-run)          DRY_RUN=true;         shift ;;
    *)                  echo "Unknown option: $1"; exit 1 ;;
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
E_MSRV="$(sed_escape "$MSRV")"
E_MSRV_FULL="$(sed_escape "$MSRV_FULL")"
E_CANONICAL="$(sed_escape "$CANONICAL_CRATE")"
E_CLI="$(sed_escape "$CLI_CRATE")"

# Apply placeholder substitutions to a ruleset JSON file, output to stdout.
process_ruleset() {
  local file="$1"
  sed \
    -e "s/__OWNER__/${E_OWNER}/g" \
    -e "s/__REPO__/${E_REPO}/g" \
    -e "s/__DEFAULT_BRANCH__/${E_BRANCH}/g" \
    -e "s/__MSRV_FULL__/${E_MSRV_FULL}/g" \
    -e "s/__MSRV__/${E_MSRV}/g" \
    -e "s/__CANONICAL_CRATE__/${E_CANONICAL}/g" \
    -e "s/__CLI_CRATE__/${E_CLI}/g" \
    "$file"
}

# Remove the review layer (ADR-0020) from a repository: delete the
# standalone Review ruleset if present, and strip copilot_code_review /
# required_review_thread_resolution out of any other active branch ruleset
# still carrying them (the pre-ADR-0020 layout, where Workflow bundled the
# review layer in). Fetches each ruleset's full detail and PUTs back a
# filtered payload — the update endpoint takes the same shape as create,
# not a partial patch.
remove_review_layer() {
  echo "Removing review layer from: $OWNER/$REPO"
  local rulesets review_id ids id detail has_review new_body name

  rulesets=$(gh api "repos/$OWNER/$REPO/rulesets" 2>/dev/null || echo '[]')

  review_id=$(jq -r '.[] | select(.name=="Review") | .id' <<<"$rulesets" | head -1)
  if [[ -n "$review_id" ]]; then
    if [[ "$DRY_RUN" == "true" ]]; then
      echo "  DRY-RUN: would DELETE Ruleset 'Review' (id=$review_id)"
    else
      gh api -X DELETE "repos/$OWNER/$REPO/rulesets/$review_id" >/dev/null
      echo "  ✓  Deleted Ruleset 'Review' (id=$review_id)"
    fi
  fi

  ids=$(jq -r '.[] | select(.target=="branch" and .enforcement=="active") | .id' <<<"$rulesets")
  while IFS= read -r id; do
    [[ -n "$id" ]] || continue
    detail=$(gh api "repos/$OWNER/$REPO/rulesets/$id" 2>/dev/null) || continue
    [[ -n "$detail" ]] || continue

    has_review=false
    jq -e '
      ([.rules[]?.type] | index("copilot_code_review"))
      or (any(.rules[]?; .type=="pull_request" and (.parameters.required_review_thread_resolution // false) == true))
    ' <<<"$detail" >/dev/null 2>&1 && has_review=true
    [[ "$has_review" == "true" ]] || continue

    name=$(jq -r '.name' <<<"$detail")
    new_body=$(jq '
      {name, target, enforcement, conditions, bypass_actors,
       rules: [.rules[] | select(.type != "copilot_code_review")
               | if .type == "pull_request"
                 then .parameters.required_review_thread_resolution = false
                 else . end]}
    ' <<<"$detail")

    if [[ "$DRY_RUN" == "true" ]]; then
      echo "  DRY-RUN: would PUT Ruleset '$name' (id=$id) stripped of the review layer:"
      echo "$new_body" | jq .
    else
      echo "$new_body" | gh api -X PUT "repos/$OWNER/$REPO/rulesets/$id" --input - >/dev/null
      echo "  ✓  Updated Ruleset '$name' (id=$id) — review layer removed"
    fi
  done <<<"$ids"
}

if [[ "$REMOVE_REVIEW" == "true" ]]; then
  remove_review_layer
  exit 0
fi

echo "Applying Rulesets to: $OWNER/$REPO"
echo ""

# Fetch existing ruleset names to detect duplicates.
EXISTING_NAMES=$(gh api "repos/$OWNER/$REPO/rulesets" --jq '.[].name' 2>/dev/null || echo "")

RULESET_FILES=(
  "$RULESETS_DIR/security.json"
  "$RULESETS_DIR/quality.json"
  "$RULESETS_DIR/workflow.json"
)
[[ "$WITH_REVIEW" == "true" ]] && RULESET_FILES+=("$RULESETS_DIR/review.json")

for ruleset_file in "${RULESET_FILES[@]}"; do

  name=$(jq -r '.name' "$ruleset_file")
  processed=$(process_ruleset "$ruleset_file")

  # Validate JSON after substitution
  if ! echo "$processed" | jq -e . >/dev/null 2>&1; then
    echo "  ERROR: Invalid JSON after substitution for '$name' — aborting."
    exit 1
  fi

  if echo "$EXISTING_NAMES" | grep -qF "$name"; then
    echo "  ⚠   '$name' already exists — skipping."
    echo "      To update: gh api repos/$OWNER/$REPO/rulesets/<id> -X PUT --input <file>"
    continue
  fi

  if [[ "$DRY_RUN" == "true" ]]; then
    echo "  DRY-RUN: would POST Ruleset '$name':"
    echo "$processed" | jq .
    echo ""
  else
    result=$(echo "$processed" | gh api -X POST "repos/$OWNER/$REPO/rulesets" --input -)
    id=$(echo "$result" | jq -r '.id')
    echo "  ✓  Created Ruleset '$name' (id=$id)"
  fi
done

echo ""
echo "NOTE: After Rulesets are created, verify required status check contexts in the"
echo "Quality Ruleset exactly match your workflow job names (see rulesets/quality.json)."
if [[ "$WITH_REVIEW" != "true" ]]; then
  echo "NOTE: Review layer (Copilot code review + required conversation resolution)"
  echo "was not applied — pass --with-review to opt in once this repository is past"
  echo "its early-development phase (ADR-0020 in tarotene/dotfiles)."
fi
