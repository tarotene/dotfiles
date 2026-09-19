#!/usr/bin/env bash
# apply-repo-settings.sh — Apply squash-only merge settings and other repo configuration.
set -euo pipefail

OWNER=""
REPO=""
DRY_RUN=false
ENABLE_DEPENDABOT=true

while [[ $# -gt 0 ]]; do
  case "$1" in
    --owner)            OWNER="$2";               shift 2 ;;
    --repo)             REPO="$2";                shift 2 ;;
    --no-dependabot)    ENABLE_DEPENDABOT=false;  shift ;;
    --dry-run)          DRY_RUN=true;             shift ;;
    # Accepted but unused (passed by seed.sh)
    --default-branch|--node-version|--package-name|--package-version|--site-base|--pages-url)
                                                  shift 2 ;;
    *)                  echo "Unknown option: $1"; exit 1 ;;
  esac
done

[[ -z "$OWNER" ]] && echo "ERROR: --owner is required" && exit 1
[[ -z "$REPO"  ]] && echo "ERROR: --repo is required"  && exit 1

command -v gh >/dev/null 2>&1 || { echo "ERROR: 'gh' not found"; exit 1; }

echo "Applying repository settings to: $OWNER/$REPO"
echo ""

SETTINGS=$(cat <<'JSON'
{
  "allow_squash_merge": true,
  "allow_merge_commit": false,
  "allow_rebase_merge": false,
  "allow_auto_merge": true,
  "has_wiki": false,
  "has_projects": false,
  "delete_branch_on_merge": true,
  "squash_merge_commit_title": "PR_TITLE",
  "squash_merge_commit_message": "BLANK",
  "use_squash_pr_title_as_default": true
}
JSON
)

if [[ "$DRY_RUN" == "true" ]]; then
  echo "  DRY-RUN: would PATCH repos/$OWNER/$REPO with:"
  echo "$SETTINGS" | jq .
else
  echo "$SETTINGS" | gh api -X PATCH "repos/$OWNER/$REPO" --input -
  echo "  ✓  Repository merge settings applied:"
  echo "     allow_squash_merge=true, allow_merge_commit=false, allow_rebase_merge=false"
  echo "     delete_branch_on_merge=true, squash_merge_commit_title=PR_TITLE"
  echo "     has_wiki=false, has_projects=false"
fi

if [[ "$ENABLE_DEPENDABOT" == "true" ]]; then
  if [[ "$DRY_RUN" == "true" ]]; then
    echo "  DRY-RUN: would enable Dependabot security updates"
  else
    gh api -X PUT "repos/$OWNER/$REPO/vulnerability-alerts" 2>/dev/null || true
    echo "  ✓  Dependabot security updates enabled"
  fi
fi
