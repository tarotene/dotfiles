#!/usr/bin/env bash
# apply-rulesets.sh — Create the GitHub Rulesets: Security / Quality / Workflow
# (core layer, always applied) and Review (review layer, opt-in addin —
# ADR-0021 in tarotene/dotfiles). Skips rulesets that already exist by name.
#
# Rust-specific: substitutes __MSRV__/__MSRV_FULL__/__CANONICAL_CRATE__/
# __CLI_CRATE__ placeholders into quality.json. Everything downstream of
# that substitution (review-layer removal, the create-only POST loop) is
# identical across the rust/typst/astro-site skills and lives in the
# shared _rulesets-apply-core.sh (#388) instead of being copied here.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILL_DIR="$(dirname "$SCRIPT_DIR")"
RULESETS_DIR="$SKILL_DIR/rulesets"

# shellcheck source=../../repo-governance-common/scripts/_rulesets-apply-core.sh
source "$SKILL_DIR/../repo-governance-common/scripts/_rulesets-apply-core.sh"

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

governance_apply_rulesets_main "$RULESETS_DIR"
