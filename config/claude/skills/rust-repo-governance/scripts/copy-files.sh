#!/usr/bin/env bash
# copy-files.sh — Copy template files to target repo with placeholder substitution.
# Backs up existing files before overwriting.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILL_DIR="$(dirname "$SCRIPT_DIR")"
TEMPLATES="$SKILL_DIR/templates"

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
# #222: constraints.rust (renovate.json) は "実在する MSRV pin" を表明する
# 明示フラグが無い限り埋めない — 適用先が dtolnay/rust-toolchain@stable 等
# チャンネル名運用(pin なし)のとき、機械的な既定値がRenovateの依存更新を
# 黙って阻害する事故を防ぐ。
HAS_MSRV_PIN=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --owner)            OWNER="$2";           shift 2 ;;
    --repo)             REPO="$2";            shift 2 ;;
    --default-branch)   DEFAULT_BRANCH="$2";  shift 2 ;;
    --msrv)             MSRV="$2";            shift 2 ;;
    --msrv-full)        MSRV_FULL="$2"; HAS_MSRV_PIN=true; shift 2 ;;
    --canonical-crate)  CANONICAL_CRATE="$2"; shift 2 ;;
    --cli-crate)        CLI_CRATE="$2";       shift 2 ;;
    --dest)             DEST="$2";            shift 2 ;;
    --with-firmware)    WITH_FIRMWARE=true;   shift ;;
    --dry-run)          DRY_RUN=true;         shift ;;
    *)                  echo "Unknown option: $1"; exit 1 ;;
  esac
done

for v in OWNER REPO CANONICAL_CRATE CLI_CRATE DEST; do
  [[ -z "${!v}" ]] && echo "ERROR: --${v,,} is required (got empty)" && exit 1
done

# Escape a string for use in a sed replacement (escapes &, /, \)
sed_escape() { printf '%s' "$1" | sed 's/[&/\]/\\&/g'; }

E_OWNER="$(sed_escape "$OWNER")"
E_REPO="$(sed_escape "$REPO")"
E_BRANCH="$(sed_escape "$DEFAULT_BRANCH")"
E_MSRV="$(sed_escape "$MSRV")"
E_MSRV_FULL="$(sed_escape "$MSRV_FULL")"
E_CANONICAL="$(sed_escape "$CANONICAL_CRATE")"
E_CLI="$(sed_escape "$CLI_CRATE")"

apply_substitutions() {
  local file="$1"
  local tmpfile; tmpfile="$(mktemp)"
  # Note: __MSRV_FULL__ must be replaced before __MSRV__ to avoid partial matches.
  sed \
    -e "s/__OWNER__/${E_OWNER}/g" \
    -e "s/__REPO__/${E_REPO}/g" \
    -e "s/__DEFAULT_BRANCH__/${E_BRANCH}/g" \
    -e "s/__MSRV_FULL__/${E_MSRV_FULL}/g" \
    -e "s/__MSRV__/${E_MSRV}/g" \
    -e "s/__CANONICAL_CRATE__/${E_CANONICAL}/g" \
    -e "s/__CLI_CRATE__/${E_CLI}/g" \
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
copy_file ".github/actions/rust-setup/action.yml"
copy_file ".github/workflows/fmt.yml"
copy_file ".github/workflows/host.yml"
copy_file ".github/workflows/tools.yml"
copy_file ".github/workflows/msrv.yml"
[[ "$WITH_FIRMWARE" == "true" ]] && copy_file ".github/workflows/firmware.yml"
copy_file ".github/workflows/release-plz.yml"
copy_file ".github/workflows/release-binaries.yml"
copy_file ".github/workflows/release-nudge.yml"
copy_file ".github/workflows/lang-mix.yml"
copy_file ".github/workflows/pr-title.yml"

# .githooks
copy_file ".githooks/commit-msg"
copy_file ".githooks/pre-commit"
copy_file ".githooks/pre-push"

# Root config files
copy_file "renovate.json"
if [[ "$HAS_MSRV_PIN" != "true" && "$DRY_RUN" == "false" && -f "$DEST/renovate.json" ]]; then
  # #222: MSRV pin が実在しない適用先では、constraints.rust ブロックと
  # それを保護する packageRule(matchDepNames: ["rust"])を丸ごと省略する。
  tmpfile="$(mktemp)"
  jq 'del(.constraints) | .packageRules |= map(select((.matchDepNames // []) != ["rust"]))' \
    "$DEST/renovate.json" > "$tmpfile" && mv "$tmpfile" "$DEST/renovate.json"
  echo "  (no --msrv-full given: dropped renovate.json constraints.rust + its protective packageRule)"
fi
copy_file "release-plz.toml"
copy_file "cog.toml"
copy_file "rust-toolchain.toml"
copy_file "Justfile"

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

echo ""
echo "Review all '# ADJUST:' comments in the copied files before committing."
