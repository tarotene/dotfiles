#!/usr/bin/env bash
# apply-rulesets.sh — Create the 3 GitHub Rulesets (Security / Quality / Workflow).
# Skips rulesets that already exist by name.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILL_DIR="$(dirname "$SCRIPT_DIR")"
RULESETS_DIR="$SKILL_DIR/rulesets"

OWNER=""
REPO=""
DEFAULT_BRANCH="main"
NODE_VERSION="22"
PACKAGE_NAME=""
SITE_BASE=""
PAGES_URL=""
DRY_RUN=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --owner)            OWNER="$2";           shift 2 ;;
    --repo)             REPO="$2";            shift 2 ;;
    --default-branch)   DEFAULT_BRANCH="$2";  shift 2 ;;
    --node-version)     NODE_VERSION="$2";    shift 2 ;;
    --package-name)     PACKAGE_NAME="$2";    shift 2 ;;
    --site-base)        SITE_BASE="$2";       shift 2 ;;
    --pages-url)        PAGES_URL="$2";       shift 2 ;;
    --package-version)                        shift 2 ;;  # accepted, not used in rulesets
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
E_NODE="$(sed_escape "$NODE_VERSION")"
E_PKG="$(sed_escape "$PACKAGE_NAME")"

# Apply placeholder substitutions to a ruleset JSON file, output to stdout.
process_ruleset() {
  local file="$1"
  sed \
    -e "s/__OWNER__/${E_OWNER}/g" \
    -e "s/__REPO__/${E_REPO}/g" \
    -e "s/__DEFAULT_BRANCH__/${E_BRANCH}/g" \
    -e "s/__NODE_VERSION__/${E_NODE}/g" \
    -e "s/__PACKAGE_NAME__/${E_PKG}/g" \
    "$file"
}

echo "Applying Rulesets to: $OWNER/$REPO"
echo ""

# Fetch existing ruleset names to detect duplicates.
EXISTING_NAMES=$(gh api "repos/$OWNER/$REPO/rulesets" --jq '.[].name' 2>/dev/null || echo "")

for ruleset_file in \
    "$RULESETS_DIR/security.json" \
    "$RULESETS_DIR/quality.json" \
    "$RULESETS_DIR/workflow.json"; do

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
echo "Key invariant: The job name: fields in templates/.github/workflows/ci.yml"
echo "must exactly match the context strings in rulesets/quality.json."
echo "If you rename a CI job, update the Ruleset context string at the same time."
