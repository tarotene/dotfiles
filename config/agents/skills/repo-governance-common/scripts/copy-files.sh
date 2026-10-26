#!/usr/bin/env bash
# copy-files.sh — seed the "core" type (#337): a repository that matches
# none of the rust/typst/astro ecosystems above but still needs governance
# (ADR-0021 core layer). Copies templates/.github/workflows/pr-title.yml
# and templates/.github/rulesets/*.json (the branch-protection declarations)
# with placeholder substitution. Quality's required_status_checks is
# `CI passed` + `PR title` (ADR-591 D1), and `CI passed` is reported by the
# aggregate job in ci.yml, so the minimal templates/.github/workflows/ci.yml
# (only the `ci-passed` job, no gate jobs: this repository's CI shape is
# unknown to the skill) is seeded in the SAME step -- without it the first PR
# waits ~20 min on a `CI passed` that is never reported and then fails
# workflow-naming-check with ci-yml-missing (#794, #815). An existing ci.yml is
# never overwritten: a repository that already has gate jobs keeps them.
# The ADR-number check (ADR-380) is seeded too: adr-number.yml plus its
# scripts/adr-number-check engine. kiriban.yml (milestone-number celebration,
# docs/kiriban.md) is seeded as well; `--only <rel>` seeds just one template
# file into an existing repository.
#
# Unlike rust/typst/astro, this type has no seed.sh: the caller runs this
# script, commits + pushes the result, then applies the ruleset with
# `apply-rulesets.sh <owner>/<repo> --from-dir <dest>/.github/rulesets
# --unverified-contexts` (ADR-503).
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
ONLY=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --owner)            OWNER="$2";           shift 2 ;;
    --repo)             REPO="$2";            shift 2 ;;
    --default-branch)   DEFAULT_BRANCH="$2";  shift 2 ;;
    --dest)             DEST="$2";            shift 2 ;;
    --with-review)      WITH_REVIEW=true;     shift ;;
    --only)             ONLY="$2";            shift 2 ;;
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

# ADR-503 D6: 置換後に __X__ 形式の
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

# Copy only when the destination does not exist yet (ci.yml: the repository's own
# gate jobs must survive a re-run of the seeding).
copy_file_if_absent() {
  local rel="$1"
  if [[ -e "$DEST/$rel" ]]; then
    echo "  kept        ${rel}  (already exists)"
    return
  fi
  copy_file "$rel"
}

# --only <relative path>: seed exactly one template file into an EXISTING
# repository and stop -- no ci.yml / rulesets / adr-number set. For a file that
# has no placeholders and is the same for every repository type (kiriban.yml,
# docs/kiriban.md "Adding it to an existing repository"), so this one script
# serves them all.
if [[ -n "$ONLY" ]]; then
  if [[ "$ONLY" == /* || "$ONLY" == *..* || ! -f "$TEMPLATES/$ONLY" ]]; then
    echo "ERROR: --only needs a template path relative to $TEMPLATES (got: $ONLY)" >&2
    exit 1
  fi
  echo "Copying one template to: $DEST"
  echo ""
  copy_file "$ONLY"
  exit 0
fi

echo "Copying core-type templates to: $DEST"
echo ""

copy_file_if_absent ".github/workflows/ci.yml"
copy_file ".github/workflows/pr-title.yml"
copy_file ".github/workflows/adr-number.yml"
copy_file "scripts/adr-number-check"
copy_file ".github/workflows/close-linked-issues.yml"
copy_file ".github/workflows/kiriban.yml"
copy_file ".github/zizmor.yml"
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

if [[ "$DRY_RUN" == "false" ]]; then
  chmod +x "$DEST/scripts/adr-number-check" 2>/dev/null || true
fi

echo ""
echo "Next: commit + push $DEST, open a PR, then run"
echo "  apply-rulesets.sh $OWNER/$REPO --from-dir $DEST/.github/rulesets --unverified-contexts"
echo "and once CI has run once, re-run with --reconcile (no --unverified-contexts) to verify."
