#!/usr/bin/env bash
# seed.sh — Main orchestrator for rust-repo-governance Skill.
# Applies all governance templates to a target Rust workspace repository.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

OWNER=""
REPO=""
DEFAULT_BRANCH="main"
MSRV="1.88"
MSRV_FULL="1.88.0"
CANONICAL_CRATE=""
CLI_CRATE=""
DEST=""
WITH_FIRMWARE=false
DRY_RUN=false
SKIP_RULESETS=false
SKIP_FILES=false
SKIP_SETTINGS=false
SKIP_HOOKS=false

usage() {
  cat <<EOF
Usage: $0 --owner OWNER --repo REPO --canonical-crate CRATE --cli-crate CRATE --dest PATH [options]

Required:
  --owner OWNER             GitHub owner login (e.g. acme)
  --repo REPO               Repository name (e.g. my-lib)
  --canonical-crate NAME    Crate that owns the release tag (e.g. my-lib-core)
  --cli-crate NAME          Excluded CLI crate under tools/ (e.g. my-cli)
  --dest PATH               Path to the target repository root on disk

Options:
  --default-branch BRANCH   Default branch name [default: main]
  --msrv VERSION            MSRV short form, e.g. 1.88 [default: 1.88]
  --msrv-full VERSION       MSRV full form, e.g. 1.88.0 [default: 1.88.0]
  --with-firmware           Include firmware.yml workflow (embedded projects only)
  --skip-rulesets           Skip Ruleset creation via gh api
  --skip-files              Skip template file copy
  --skip-settings           Skip repository settings update
  --skip-hooks              Skip git hooks configuration
  --dry-run                 Show what would be done without making changes
  -h, --help                Show this help

Examples:
  # Dry-run to preview changes:
  $0 --owner acme --repo my-lib --canonical-crate my-lib-core \\
     --cli-crate my-cli --dest ~/src/my-lib --dry-run

  # Apply everything:
  $0 --owner acme --repo my-lib --canonical-crate my-lib-core \\
     --cli-crate my-cli --dest ~/src/my-lib

  # Apply with embedded firmware workflow:
  $0 --owner acme --repo my-lib --canonical-crate my-lib-core \\
     --cli-crate my-cli --dest ~/src/my-lib --with-firmware

  # Only create Rulesets (files already copied manually):
  $0 --owner acme --repo my-lib --canonical-crate x --cli-crate x \\
     --dest /tmp --skip-files --skip-settings --skip-hooks
EOF
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --owner)            OWNER="$2";           shift 2 ;;
    --repo)             REPO="$2";            shift 2 ;;
    --default-branch)   DEFAULT_BRANCH="$2";  shift 2 ;;
    --msrv)             MSRV="$2";            shift 2 ;;
    --msrv-full)        MSRV_FULL="$2";       shift 2 ;;
    --canonical-crate)  CANONICAL_CRATE="$2"; shift 2 ;;
    --cli-crate)        CLI_CRATE="$2";       shift 2 ;;
    --dest)             DEST="$2";            shift 2 ;;
    --with-firmware)    WITH_FIRMWARE=true;   shift ;;
    --skip-rulesets)    SKIP_RULESETS=true;   shift ;;
    --skip-files)       SKIP_FILES=true;      shift ;;
    --skip-settings)    SKIP_SETTINGS=true;   shift ;;
    --skip-hooks)       SKIP_HOOKS=true;      shift ;;
    --dry-run)          DRY_RUN=true;         shift ;;
    -h|--help)          usage ;;
    *)                  echo "Unknown option: $1"; usage ;;
  esac
done

[[ -z "$OWNER" ]]           && echo "ERROR: --owner is required"           && usage
[[ -z "$REPO" ]]            && echo "ERROR: --repo is required"            && usage
[[ -z "$CANONICAL_CRATE" ]] && echo "ERROR: --canonical-crate is required" && usage
[[ -z "$CLI_CRATE" ]]       && echo "ERROR: --cli-crate is required"       && usage
[[ -z "$DEST" ]]            && echo "ERROR: --dest is required"            && usage

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
echo "  default-branch : $DEFAULT_BRANCH"
echo "  MSRV           : $MSRV ($MSRV_FULL)"
echo "  canonical-crate: $CANONICAL_CRATE"
echo "  cli-crate      : $CLI_CRATE"
echo "  dest           : $DEST"
echo "  with-firmware  : $WITH_FIRMWARE"
echo ""

COMMON_ARGS=(
  --owner "$OWNER"
  --repo "$REPO"
  --default-branch "$DEFAULT_BRANCH"
  --msrv "$MSRV"
  --msrv-full "$MSRV_FULL"
  --canonical-crate "$CANONICAL_CRATE"
  --cli-crate "$CLI_CRATE"
)
[[ "$WITH_FIRMWARE" == "true" ]] && COMMON_ARGS+=(--with-firmware)
[[ "$DRY_RUN" == "true" ]]       && COMMON_ARGS+=(--dry-run)

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
echo "  See reference/manual-steps.md in the Skill directory for"
echo "  the full checklist. Short version:"
echo ""
echo "  1. Review all '# ADJUST:' comments in copied files."
echo "  2. Install the shared releaser App (do not create a new one) →"
echo "     set secrets RELEASER_APP_ID and RELEASER_APP_PRIVATE_KEY."
echo "  3. Register crates.io Trusted Publishing entries."
echo "  4. git -C $DEST config --local core.hooksPath .githooks"
echo "  5. Commit files and push to trigger CI."
echo "════════════════════════════════════════════════════════"
