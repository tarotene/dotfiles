#!/usr/bin/env bash
# seed.sh — Main orchestrator for astro-site-governance Skill.
# Applies all governance templates to a target Astro site repository.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

OWNER=""
REPO=""
DEFAULT_BRANCH="main"
NODE_VERSION="22"
PACKAGE_NAME=""
PACKAGE_VERSION="0.1.0"
SITE_BASE=""
PAGES_URL=""
DEST=""
DRY_RUN=false
SKIP_RULESETS=false
SKIP_FILES=false
SKIP_SETTINGS=false
SKIP_HOOKS=false

usage() {
  cat <<EOF
Usage: $0 --owner OWNER --repo REPO --package-name NAME --dest PATH [options]

Required:
  --owner OWNER            GitHub owner login (e.g. tarotene)
  --repo REPO              Repository name (e.g. my-astro-site)
  --package-name NAME      npm package name (used in release-please-config.json)
  --dest PATH              Path to the target repository root on disk

Options:
  --default-branch BRANCH  Default branch name [default: main]
  --node-version VERSION   Node.js version [default: 22]
  --package-version VER    Current package.json version for manifest [default: 0.1.0]
  --site-base PATH         Astro base path, e.g. /my-site (docs only) [default: ""]
  --pages-url URL          Deployed Pages URL (docs only) [default: ""]
  --skip-rulesets          Skip Ruleset creation via gh api
  --skip-files             Skip template file copy
  --skip-settings          Skip repository settings update
  --skip-hooks             Skip git hooks configuration
  --dry-run                Show what would be done without making changes
  -h, --help               Show this help

Examples:
  # Dry-run to preview changes:
  $0 --owner tarotene --repo my-astro-site \\
     --package-name my-astro-site \\
     --site-base /my-astro-site \\
     --dest ~/src/my-astro-site --dry-run

  # Apply everything:
  $0 --owner tarotene --repo my-astro-site \\
     --package-name my-astro-site --dest ~/src/my-astro-site
EOF
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --owner)            OWNER="$2";             shift 2 ;;
    --repo)             REPO="$2";              shift 2 ;;
    --default-branch)   DEFAULT_BRANCH="$2";    shift 2 ;;
    --node-version)     NODE_VERSION="$2";      shift 2 ;;
    --package-name)     PACKAGE_NAME="$2";      shift 2 ;;
    --package-version)  PACKAGE_VERSION="$2";   shift 2 ;;
    --site-base)        SITE_BASE="$2";         shift 2 ;;
    --pages-url)        PAGES_URL="$2";         shift 2 ;;
    --dest)             DEST="$2";              shift 2 ;;
    --skip-rulesets)    SKIP_RULESETS=true;     shift ;;
    --skip-files)       SKIP_FILES=true;        shift ;;
    --skip-settings)    SKIP_SETTINGS=true;     shift ;;
    --skip-hooks)       SKIP_HOOKS=true;        shift ;;
    --dry-run)          DRY_RUN=true;           shift ;;
    -h|--help)          usage ;;
    *)                  echo "Unknown option: $1"; usage ;;
  esac
done

[[ -z "$OWNER" ]]        && echo "ERROR: --owner is required"        && usage
[[ -z "$REPO" ]]         && echo "ERROR: --repo is required"         && usage
[[ -z "$PACKAGE_NAME" ]] && echo "ERROR: --package-name is required" && usage
[[ -z "$DEST" ]]         && echo "ERROR: --dest is required"         && usage

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
echo "  node-version    : $NODE_VERSION"
echo "  package-name    : $PACKAGE_NAME"
echo "  package-version : $PACKAGE_VERSION"
echo "  site-base       : ${SITE_BASE:-(not set)}"
echo "  pages-url       : ${PAGES_URL:-(not set)}"
echo "  dest            : $DEST"
echo ""

COMMON_ARGS=(
  --owner "$OWNER"
  --repo "$REPO"
  --default-branch "$DEFAULT_BRANCH"
  --node-version "$NODE_VERSION"
  --package-name "$PACKAGE_NAME"
  --package-version "$PACKAGE_VERSION"
  --site-base "$SITE_BASE"
  --pages-url "$PAGES_URL"
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
echo "  See reference/manual-steps.md in the Skill directory for"
echo "  the full checklist. Short version:"
echo ""
echo "  1. Merge package.json + mise.toml snippets (shown above)."
echo "  2. Run: npm install && mise install"
echo "  3. Run: git -C $DEST config --local core.hooksPath .githooks"
echo "  4. Review all '# ADJUST:' comments in copied files."
echo "  5. Enable GitHub Pages: Settings → Pages → Source: GitHub Actions."
echo "  6. Commit all files and push to trigger CI."
echo "  7. (Optional) Create GitHub App for release-please to run full CI"
echo "     on release PRs — see reference/manual-steps.md."
echo "════════════════════════════════════════════════════════"
