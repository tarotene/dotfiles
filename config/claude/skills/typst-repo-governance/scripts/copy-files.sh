#!/usr/bin/env bash
# copy-files.sh — Copy template files to target repo with placeholder substitution.
# Backs up any pre-existing files before overwriting.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILL_DIR="$(dirname "$SCRIPT_DIR")"
TEMPLATES="$SKILL_DIR/templates"

OWNER=""
REPO=""
DEFAULT_BRANCH="main"
TYPST_VERSION="0.14.2"
MIN_TYPST="0.14.0"
ATS_EMAIL=""
DEST=""
DRY_RUN=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --owner)           OWNER="$2";           shift 2 ;;
    --repo)            REPO="$2";            shift 2 ;;
    --default-branch)  DEFAULT_BRANCH="$2";  shift 2 ;;
    --typst-version)   TYPST_VERSION="$2";   shift 2 ;;
    --min-typst)       MIN_TYPST="$2";       shift 2 ;;
    --ats-email)       ATS_EMAIL="$2";       shift 2 ;;
    --dest)            DEST="$2";            shift 2 ;;
    --dry-run)         DRY_RUN=true;         shift ;;
    *)                 echo "Unknown option: $1"; exit 1 ;;
  esac
done

for v in OWNER REPO ATS_EMAIL DEST; do
  [[ -z "${!v}" ]] && echo "ERROR: --${v,,} is required (got empty)" && exit 1
done

# Escape a string for use in a sed replacement (escapes &, /, \)
sed_escape() { printf '%s' "$1" | sed 's/[&/\]/\\&/g'; }

E_OWNER="$(sed_escape "$OWNER")"
E_REPO="$(sed_escape "$REPO")"
E_BRANCH="$(sed_escape "$DEFAULT_BRANCH")"
E_TYPST="$(sed_escape "$TYPST_VERSION")"
E_MIN="$(sed_escape "$MIN_TYPST")"
E_EMAIL="$(sed_escape "$ATS_EMAIL")"

apply_substitutions() {
  local file="$1"
  local tmpfile; tmpfile="$(mktemp)"
  # Note: __TYPST_VERSION__ and __MIN_TYPST__ do not share a token prefix,
  # so ordering is not critical — both are substituted independently.
  sed \
    -e "s/__OWNER__/${E_OWNER}/g" \
    -e "s/__REPO__/${E_REPO}/g" \
    -e "s/__DEFAULT_BRANCH__/${E_BRANCH}/g" \
    -e "s/__TYPST_VERSION__/${E_TYPST}/g" \
    -e "s/__MIN_TYPST__/${E_MIN}/g" \
    -e "s/__ATS_EMAIL__/${E_EMAIL}/g" \
    "$file" > "$tmpfile" && mv "$tmpfile" "$file"
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

echo "Copying templates to: $DEST"
echo ""

# .github
copy_file ".github/CODEOWNERS"
copy_file ".github/actions/typst-setup/action.yml"
copy_file ".github/workflows/build.yml"
copy_file ".github/workflows/fmt.yml"
copy_file ".github/workflows/lint.yml"
copy_file ".github/workflows/min-typst.yml"
copy_file ".github/workflows/pr-title.yml"
copy_file ".github/workflows/release.yml"
copy_file ".github/workflows/metrics-reminder.yml"
copy_file ".github/workflows/lang-mix.yml"
copy_file ".github/workflows/nav-docs.yml"

# .githooks
copy_file ".githooks/commit-msg"
copy_file ".githooks/pre-commit"
copy_file ".githooks/pre-push"

# Root config files
copy_file "renovate.json"
copy_file "cliff.toml"
copy_file ".yamllint"
copy_file "Justfile"

copy_file "scripts/check-language-mixing.sh"
copy_file "scripts/check-nav-docs.sh"

# AI-facing document routing (ADR-0016 in tarotene/dotfiles): AGENTS.md is
# the canon, CLAUDE.md just imports it. Content stays generic here — the
# repo-charter skill (tarotene/dotfiles) fills in the charter itself.
copy_file "AGENTS.md"
copy_file "CLAUDE.md"

if [[ "$DRY_RUN" == "false" ]]; then
  chmod +x \
    "$DEST/.githooks/commit-msg" \
    "$DEST/.githooks/pre-commit" \
    "$DEST/.githooks/pre-push" \
    "$DEST/scripts/check-language-mixing.sh" \
    "$DEST/scripts/check-nav-docs.sh" 2>/dev/null || true
  echo ""
  echo "Made .githooks scripts executable."
fi

echo ""
echo "Note: .gitignore-snippet is NOT auto-copied — merge it manually into $DEST/.gitignore"
echo "Review all '# ADJUST:' comments in the copied files before committing."
