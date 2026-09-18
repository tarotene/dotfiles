#!/usr/bin/env bash
# seed.sh — Main orchestrator for typst-repo-governance Skill.
# Applies all governance templates to a target Typst document repository.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

OWNER=""
REPO=""
DEFAULT_BRANCH="main"
TYPST_VERSION="0.14.2"
MIN_TYPST="0.14.0"
ATS_EMAIL=""
DEST=""
DRY_RUN=false
SKIP_RULESETS=false
SKIP_FILES=false
SKIP_SETTINGS=false
SKIP_HOOKS=false

usage() {
  cat <<EOF
Usage: $0 --owner OWNER --repo REPO --ats-email EMAIL --dest PATH [options]

Required:
  --owner OWNER             GitHub owner login (e.g. tarotene)
  --repo REPO               Repository name (e.g. cv)
  --ats-email EMAIL         Contact email that must appear verbatim in the ATS PDF
  --dest PATH               Path to the target repository root on disk

Options:
  --default-branch BRANCH   Default branch name [default: main]
  --typst-version VERSION   Typst version to pin in CI (e.g. 0.14.2) [default: 0.14.2]
  --min-typst VERSION       Minimum supported Typst version (e.g. 0.14.0) [default: 0.14.0]
  --skip-rulesets           Skip Ruleset creation via gh api
  --skip-files              Skip template file copy
  --skip-settings           Skip repository settings update
  --skip-hooks              Skip git hooks configuration
  --dry-run                 Show what would be done without making changes
  -h, --help                Show this help

Examples:
  # Dry-run to preview changes:
  $0 --owner tarotene --repo cv --ats-email you@example.com \\
     --dest ~/src/cv --dry-run

  # Apply everything:
  $0 --owner tarotene --repo cv --ats-email you@example.com \\
     --dest ~/src/cv

  # Only create Rulesets (files already copied):
  $0 --owner tarotene --repo cv --ats-email x --dest /tmp \\
     --skip-files --skip-settings --skip-hooks
EOF
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --owner)           OWNER="$2";           shift 2 ;;
    --repo)            REPO="$2";            shift 2 ;;
    --default-branch)  DEFAULT_BRANCH="$2";  shift 2 ;;
    --typst-version)   TYPST_VERSION="$2";   shift 2 ;;
    --min-typst)       MIN_TYPST="$2";       shift 2 ;;
    --ats-email)       ATS_EMAIL="$2";       shift 2 ;;
    --dest)            DEST="$2";            shift 2 ;;
    --skip-rulesets)   SKIP_RULESETS=true;   shift ;;
    --skip-files)      SKIP_FILES=true;      shift ;;
    --skip-settings)   SKIP_SETTINGS=true;   shift ;;
    --skip-hooks)      SKIP_HOOKS=true;      shift ;;
    --dry-run)         DRY_RUN=true;         shift ;;
    -h|--help)         usage ;;
    *)                 echo "Unknown option: $1"; usage ;;
  esac
done

[[ -z "$OWNER"    ]] && echo "ERROR: --owner is required"     && usage
[[ -z "$REPO"     ]] && echo "ERROR: --repo is required"      && usage
[[ -z "$ATS_EMAIL" ]] && echo "ERROR: --ats-email is required" && usage
[[ -z "$DEST"     ]] && echo "ERROR: --dest is required"      && usage

# Validate prerequisites
for cmd in gh jq git; do
  command -v "$cmd" >/dev/null 2>&1 || {
    echo "ERROR: '$cmd' is required but not found on PATH."
    exit 1
  }
done

if [[ "$DRY_RUN" == "true" ]]; then
  echo "═══════════════════════════════════════════════════"
  echo "  DRY-RUN MODE — no changes will be made"
  echo "═══════════════════════════════════════════════════"
fi

echo ""
echo "Target: $OWNER/$REPO"
echo "  default-branch  : $DEFAULT_BRANCH"
echo "  typst-version   : $TYPST_VERSION"
echo "  min-typst       : $MIN_TYPST"
echo "  ats-email       : $ATS_EMAIL"
echo "  dest            : $DEST"
echo ""

COMMON_ARGS=(
  --owner "$OWNER"
  --repo "$REPO"
  --default-branch "$DEFAULT_BRANCH"
  --typst-version "$TYPST_VERSION"
  --min-typst "$MIN_TYPST"
  --ats-email "$ATS_EMAIL"
)
[[ "$DRY_RUN" == "true" ]] && COMMON_ARGS+=(--dry-run)

# Step 1: Copy template files
if [[ "$SKIP_FILES" == "false" ]]; then
  echo "── Step 1/4: Copy template files ──────────────────────"
  bash "$SCRIPT_DIR/copy-files.sh" "${COMMON_ARGS[@]}" --dest "$DEST"
else
  echo "── Step 1/4: (skipped) copy template files"
fi

# Step 2: Configure git hooks
if [[ "$SKIP_HOOKS" == "false" ]]; then
  echo ""
  echo "── Step 2/4: Configure git hooks ───────────────────────"
  bash "$SCRIPT_DIR/setup-hooks.sh" "${COMMON_ARGS[@]}" --dest "$DEST"
else
  echo "── Step 2/4: (skipped) git hooks"
fi

# Step 3: Apply repository settings
if [[ "$SKIP_SETTINGS" == "false" ]]; then
  echo ""
  echo "── Step 3/4: Apply repository settings ─────────────────"
  bash "$SCRIPT_DIR/apply-repo-settings.sh" "${COMMON_ARGS[@]}"
else
  echo "── Step 3/4: (skipped) repository settings"
fi

# Step 4: Create GitHub Rulesets
if [[ "$SKIP_RULESETS" == "false" ]]; then
  echo ""
  echo "── Step 4/4: Create GitHub Rulesets ────────────────────"
  bash "$SCRIPT_DIR/apply-rulesets.sh" "${COMMON_ARGS[@]}"
else
  echo "── Step 4/4: (skipped) GitHub Rulesets"
fi

echo ""
echo "════════════════════════════════════════════════════════"
echo "  Done."
echo ""
echo "  Next steps (manual):"
echo "  See reference/manual-steps.md for the full checklist."
echo "  Short version:"
echo ""
echo "  1. Review all '# ADJUST:' comments in copied files."
echo "  2. Install Mend Renovate GitHub App on $OWNER/$REPO."
echo "  3. Set up commit signing (GPG or SSH) for 'required_signatures'."
echo "  4. Run: git -C $DEST config --local core.hooksPath .githooks"
echo "  5. Commit files, push branch, open PR — all 5 checks should go green."
echo "  6. After merge: apply-rulesets.sh and apply-repo-settings.sh."
echo "════════════════════════════════════════════════════════"
