#!/usr/bin/env bash
# apply-repo-settings.sh — Apply squash-only merge settings and other repo
# configuration. Single source shared by rust/typst/astro-site-repo-
# governance (#388): the three per-skill copies had zero real logic
# difference — only the names of ecosystem-specific flags they silently
# discard and a one-word wording difference in the Dependabot confirmation
# echo. This file drops both (see setup-hooks.sh, deployed alongside this
# file, for the identical rationale on the flag-discarding pattern).
#
# docs/adr/568-renovate-automerge-shared-preset.md D5b: Dependabot alerts
# stay on (they feed Renovate's vulnerabilityAlerts), but Dependabot's own
# fix-PR generation (security updates) is disabled — Renovate is the single
# fix-PR channel, so the two mechanisms stop racing each other on the same
# advisory.
#
# Deployed into every *-repo-governance skill directory by
# home/modules/claude.nix (ADR-0032-style single-source, multi-mount).
set -euo pipefail

SELF="$(realpath "${BASH_SOURCE[0]}")"

usage() {
  cat <<'EOF'
usage: apply-repo-settings.sh --owner <owner> --repo <repo> [--no-dependabot] [--dry-run]
       apply-repo-settings.sh --selftest
EOF
}

# ---- --selftest ------------------------------------------------------------

selftest() {
  local fails=0 tmp log stub

  tmp="$(mktemp -d)"
  log="$tmp/gh-log.txt"
  stub="$tmp/gh"

  cat > "$stub" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$GH_STUB_LOG"
# SETTINGS json is piped via --input - ; drain stdin so the caller doesn't block.
case "$*" in
  *"--input -"*) cat > /dev/null ;;
esac
exit 0
STUB
  chmod +x "$stub"

  export GH_STUB_LOG="$log"
  export PATH="$tmp:$PATH"

  check_contains() { # $1=name $2=needle $3=haystack
    if [[ "$3" == *"$2"* ]]; then
      echo "ok   $1"
    else
      echo "FAIL $1 (expected to find: $2)" >&2
      fails=$((fails + 1))
    fi
  }
  check_not_contains() { # $1=name $2=needle $3=haystack
    if [[ "$3" != *"$2"* ]]; then
      echo "ok   $1"
    else
      echo "FAIL $1 (did not expect to find: $2)" >&2
      fails=$((fails + 1))
    fi
  }
  check_empty() { # $1=name $2=haystack
    if [[ -z "$2" ]]; then
      echo "ok   $1"
    else
      echo "FAIL $1 (expected no gh api calls, got: $2)" >&2
      fails=$((fails + 1))
    fi
  }

  echo "既定(--owner/--repo のみ):"
  : > "$log"
  bash "$SELF" --owner o --repo r > /dev/null 2>&1 || true
  check_contains "1 PATCH repos/o/r" "-X PATCH repos/o/r" "$(cat "$log")"
  check_contains "2 PUT vulnerability-alerts" "-X PUT repos/o/r/vulnerability-alerts" "$(cat "$log")"
  check_contains "3 DELETE automated-security-fixes" "-X DELETE repos/o/r/automated-security-fixes" "$(cat "$log")"

  echo "--no-dependabot:"
  : > "$log"
  bash "$SELF" --owner o --repo r --no-dependabot > /dev/null 2>&1 || true
  check_contains "4 PATCH still runs" "-X PATCH repos/o/r" "$(cat "$log")"
  check_not_contains "5 vulnerability-alerts skipped" "vulnerability-alerts" "$(cat "$log")"
  check_not_contains "6 automated-security-fixes skipped" "automated-security-fixes" "$(cat "$log")"

  echo "--dry-run:"
  : > "$log"
  bash "$SELF" --owner o --repo r --dry-run > /dev/null 2>&1 || true
  check_empty "7 no gh api calls at all" "$(cat "$log")"

  rm -rf "$tmp"

  if [[ $fails -gt 0 ]]; then
    echo "selftest: ${fails} 件失敗" >&2
    return 1
  fi
  echo "selftest: OK"
  return 0
}

case "${1-}" in
  --selftest) selftest; exit $? ;;
  --help | -h) usage; exit 0 ;;
esac

# ---- main -------------------------------------------------------------

OWNER=""
REPO=""
DRY_RUN=false
ENABLE_DEPENDABOT=true

while [[ $# -gt 0 ]]; do
  case "$1" in
    --owner) OWNER="$2"; shift 2 ;;
    --repo) REPO="$2"; shift 2 ;;
    --no-dependabot) ENABLE_DEPENDABOT=false; shift ;;
    --dry-run) DRY_RUN=true; shift ;;
    --with-firmware) shift ;; # see setup-hooks.sh: rust's one 0-arg ecosystem flag
    --with-review) shift ;; # ADR-503: shared 0-arg flag, all 3 skills
    --*) shift 2 ;; # every other ecosystem-specific flag is `--flag value`
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

[[ -z "$OWNER" ]] && echo "ERROR: --owner is required" && exit 1
[[ -z "$REPO" ]] && echo "ERROR: --repo is required" && exit 1

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
    echo "  DRY-RUN: would enable Dependabot vulnerability alerts"
    echo "  DRY-RUN: would disable Dependabot security updates (DELETE repos/$OWNER/$REPO/automated-security-fixes)"
  else
    gh api -X PUT "repos/$OWNER/$REPO/vulnerability-alerts" 2>/dev/null || true
    echo "  ✓  Dependabot vulnerability alerts enabled"
    gh api -X DELETE "repos/$OWNER/$REPO/automated-security-fixes" 2>/dev/null || true
    echo "  ✓  Dependabot security updates disabled (Renovate vulnerabilityAlerts is the single fix-PR channel, ADR-568)"
  fi
fi
