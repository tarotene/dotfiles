#!/usr/bin/env bash
# copy-files.sh — Copy template files to target repo with placeholder substitution.
# Backs up existing files before overwriting.
#
# IMPORTANT: package.json and mise.toml are NOT overwritten automatically
# (they contain existing dependencies). Instead, this script prints snippets
# and instructions for merging them manually (or via jq if available).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILL_DIR="$(dirname "$SCRIPT_DIR")"
TEMPLATES="$SKILL_DIR/templates"

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

while [[ $# -gt 0 ]]; do
  case "$1" in
    --owner)             OWNER="$2";            shift 2 ;;
    --repo)              REPO="$2";             shift 2 ;;
    --default-branch)    DEFAULT_BRANCH="$2";   shift 2 ;;
    --node-version)      NODE_VERSION="$2";     shift 2 ;;
    --package-name)      PACKAGE_NAME="$2";     shift 2 ;;
    --package-version)   PACKAGE_VERSION="$2";  shift 2 ;;
    --site-base)         SITE_BASE="$2";        shift 2 ;;
    --pages-url)         PAGES_URL="$2";        shift 2 ;;
    --dest)              DEST="$2";             shift 2 ;;
    --dry-run)           DRY_RUN=true;          shift ;;
    *)                   echo "Unknown option: $1"; exit 1 ;;
  esac
done

for v in OWNER REPO PACKAGE_NAME DEST; do
  [[ -z "${!v}" ]] && echo "ERROR: --${v//_/-} is required (got empty)" && exit 1
done

# Escape a string for use in a sed replacement (escapes &, /, \)
sed_escape() { printf '%s' "$1" | sed 's/[&/\]/\\&/g'; }

E_OWNER="$(sed_escape "$OWNER")"
E_REPO="$(sed_escape "$REPO")"
E_BRANCH="$(sed_escape "$DEFAULT_BRANCH")"
E_NODE="$(sed_escape "$NODE_VERSION")"
E_PKG="$(sed_escape "$PACKAGE_NAME")"
E_PKGVER="$(sed_escape "$PACKAGE_VERSION")"
E_BASE="$(sed_escape "$SITE_BASE")"
E_URL="$(sed_escape "$PAGES_URL")"

apply_substitutions() {
  local file="$1"
  local tmpfile; tmpfile="$(mktemp)"
  sed \
    -e "s/__OWNER__/${E_OWNER}/g" \
    -e "s/__REPO__/${E_REPO}/g" \
    -e "s/__DEFAULT_BRANCH__/${E_BRANCH}/g" \
    -e "s/__NODE_VERSION__/${E_NODE}/g" \
    -e "s/__PACKAGE_NAME__/${E_PKG}/g" \
    -e "s/__PACKAGE_VERSION__/${E_PKGVER}/g" \
    -e "s/__SITE_BASE__/${E_BASE}/g" \
    -e "s/__PAGES_URL__/${E_URL}/g" \
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
copy_file ".github/workflows/ci.yml"
copy_file ".github/workflows/release-please.yml"
copy_file ".github/workflows/lang-mix.yml"

# .githooks
copy_file ".githooks/commit-msg"
copy_file ".githooks/pre-commit"
copy_file ".githooks/pre-push"

# Root config files (safe to overwrite — these are added fresh)
copy_file "biome.json"
copy_file "cog.toml"
copy_file "renovate.json"
copy_file "release-please-config.json"
copy_file ".release-please-manifest.json"
copy_file "vitest.config.ts"

copy_file "scripts/check-language-mixing.sh"

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
    "$DEST/scripts/check-language-mixing.sh" 2>/dev/null || true
  echo ""
  echo "Made .githooks scripts executable."
fi

# ----------------------------------------------------------------
# package.json and mise.toml — MERGE, do not overwrite
# ----------------------------------------------------------------
echo ""
echo "══════════════════════════════════════════════════════"
echo "  MANUAL MERGE REQUIRED: package.json and mise.toml"
echo "══════════════════════════════════════════════════════"
echo ""
echo "  package.json — add the following (preserving existing dependencies):"
echo ""
cat "$TEMPLATES/package.scripts.jsonc-snippet" | sed 's/^/    /'
echo ""
echo "  mise.toml — add cocogitto (if not already present):"
echo ""
cat "$TEMPLATES/mise.toml-snippet" | sed 's/^/    /'
echo ""

# Offer jq-based merge if jq is available and not in dry-run
if [[ "$DRY_RUN" == "false" ]] && command -v jq >/dev/null 2>&1 && [[ -f "$DEST/package.json" ]]; then
  echo "  jq is available. Attempting automatic merge of package.json scripts..."
  PKG="$DEST/package.json"
  tmpfile="$(mktemp)"
  jq '
    .private = true |
    .engines = { "node": (">=__NODE_VERSION__" | gsub("__NODE_VERSION__"; "'"$NODE_VERSION"'")) } |
    .scripts["lint"]         = "biome lint ." |
    .scripts["lint:fix"]     = "biome lint --write ." |
    .scripts["format"]       = "biome format --write ." |
    .scripts["format:check"] = "biome format ." |
    .scripts["ci:biome"]     = "biome ci ." |
    .scripts["test"]         = "vitest run" |
    .scripts["test:watch"]   = "vitest" |
    .scripts["commit-check"] = "cog verify" |
    if .devDependencies == null then .devDependencies = {} else . end |
    .devDependencies["@biomejs/biome"]      = "^2.4.0" |
    .devDependencies["vitest"]              = "^4.1.0" |
    .devDependencies["@vitest/coverage-v8"] = "^4.1.0"
  ' "$PKG" > "$tmpfile" && mv "$tmpfile" "$PKG"
  echo "  ✓  package.json scripts/devDependencies merged via jq."
  echo "     Run 'npm install' to update package-lock.json."
fi

echo ""
echo "Review all '# ADJUST:' comments in the copied files before committing."
