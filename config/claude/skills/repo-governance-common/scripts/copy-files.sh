#!/usr/bin/env bash
# copy-files.sh — seed the "core" type (#337): a repository that matches
# none of the rust/typst/astro ecosystems above but still needs governance
# (ADR-0021 core layer). Copies templates/.github/workflows/pr-title.yml
# and templates/.github/rulesets/{security,quality,workflow}[,review].json
# with placeholder substitution — Quality's required_status_checks is
# `PR Title / PR title` only, no build/test job name is assumed (this
# repository's CI shape is unknown to the skill).
#
# Unlike rust/typst/astro, this type has no seed.sh: the caller runs this
# script, commits + pushes the result, then applies the ruleset with
# `apply-rulesets.sh <owner>/<repo> --from-dir <dest>/.github/rulesets
# --unverified-contexts` (ADR-0000-rulesets-declaration-in-repo).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILL_DIR="$(dirname "$SCRIPT_DIR")"
TEMPLATES="$SKILL_DIR/templates"

OWNER=""
REPO=""
DEFAULT_BRANCH="main"
DEST=""
WITH_REVIEW=false
DRY_RUN=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --owner)            OWNER="$2";           shift 2 ;;
    --repo)             REPO="$2";            shift 2 ;;
    --default-branch)   DEFAULT_BRANCH="$2";  shift 2 ;;
    --dest)             DEST="$2";            shift 2 ;;
    --with-review)      WITH_REVIEW=true;     shift ;;
    --dry-run)          DRY_RUN=true;         shift ;;
    *)                  echo "Unknown option: $1"; exit 1 ;;
  esac
done

for v in OWNER REPO DEST; do
  [[ -z "${!v}" ]] && echo "ERROR: --${v,,} is required (got empty)" && exit 1
done

# Escape a string for use in a sed replacement (escapes &, /, \)
sed_escape() { printf '%s' "$1" | sed 's/[&/\]/\\&/g'; }

E_OWNER="$(sed_escape "$OWNER")"
E_REPO="$(sed_escape "$REPO")"
E_BRANCH="$(sed_escape "$DEFAULT_BRANCH")"

apply_substitutions() {
  local file="$1"
  local tmpfile; tmpfile="$(mktemp)"
  sed \
    -e "s/__OWNER__/${E_OWNER}/g" \
    -e "s/__REPO__/${E_REPO}/g" \
    -e "s/__DEFAULT_BRANCH__/${E_BRANCH}/g" \
    "$file" > "$tmpfile" && mv "$tmpfile" "$file"
}

# ADR-0000-rulesets-declaration-in-repo D6: 置換後に __X__ 形式の
# placeholder が残っている場合を、ruleset 宣言ファイルについて検査する
# (apply-rulesets.sh 側の check_no_placeholders と二重に守る)。
verify_declaration() {
  local file="$1"
  [[ -f "$file" ]] || return 0
  if grep -qE '__[A-Z_]+__' "$file"; then
    echo "ERROR: $file still has an unreplaced placeholder (__X__) after substitution." >&2
    grep -oE '__[A-Z_]+__' "$file" | sort -u >&2
    exit 1
  fi
}

TS="$(date +%Y%m%dT%H%M%S)"

copy_file() {
  local rel="$1"
  local src="$TEMPLATES/$rel"
  local dst="$DEST/$rel"

  if [[ ! -f "$src" ]]; then
    echo "  WARN: source not found — $src"
    return
  fi

  if [[ "$DRY_RUN" == "true" ]]; then
    echo "  WOULD COPY  $rel"
    return
  fi

  mkdir -p "$(dirname "$dst")"
  if [[ -f "$dst" ]]; then
    cp "$dst" "${dst}.bak-${TS}"
    echo "  backed up   ${rel}  →  ${rel}.bak-${TS}"
  fi
  cp "$src" "$dst"
  apply_substitutions "$dst"
  echo "  ✓  $rel"
}

echo "Copying core-type templates to: $DEST"
echo ""

copy_file ".github/workflows/pr-title.yml"
copy_file ".github/rulesets/security.json"
copy_file ".github/rulesets/quality.json"
copy_file ".github/rulesets/workflow.json"
[[ "$WITH_REVIEW" == "true" ]] && copy_file ".github/rulesets/review.json"

if [[ "$DRY_RUN" == "false" ]]; then
  verify_declaration "$DEST/.github/rulesets/security.json"
  verify_declaration "$DEST/.github/rulesets/quality.json"
  verify_declaration "$DEST/.github/rulesets/workflow.json"
  [[ "$WITH_REVIEW" == "true" ]] && verify_declaration "$DEST/.github/rulesets/review.json"
fi

echo ""
echo "Next: commit + push $DEST, open a PR, then run"
echo "  apply-rulesets.sh $OWNER/$REPO --from-dir $DEST/.github/rulesets --unverified-contexts"
echo "and once CI has run once, re-run with --reconcile (no --unverified-contexts) to verify."
