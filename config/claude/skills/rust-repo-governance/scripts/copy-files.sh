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
WITH_REVIEW=false
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
    --with-review)      WITH_REVIEW=true;     shift ;;
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

# ADR-0000-rulesets-declaration-in-repo D6: 置換後に __X__ 形式の
# placeholder が残っている、または値が空のまま置換された(例: CLI_CRATE
# が空文字列で "Tools ( CLI clippy + tests)" のような不完全な context に
# なる)ケースを、ruleset 宣言ファイルについてだけ厳密に検査する
# (apply-rulesets.sh 側の check_no_placeholders は __X__ の残存だけを見て、
# 空文字列への置換は検出できないため、ここで二重に守る)。
verify_declaration() {
  local file="$1"
  [[ -f "$file" ]] || return 0
  if grep -qE '__[A-Z_]+__' "$file"; then
    echo "ERROR: $file still has an unreplaced placeholder (__X__) after substitution." >&2
    grep -oE '__[A-Z_]+__' "$file" | sort -u >&2
    exit 1
  fi
  if grep -qE '\( CLI clippy|MSRV \(\)|\(  *\)' "$file"; then
    echo "ERROR: $file looks like a placeholder was substituted with an empty value" >&2
    echo "  (e.g. --cli-crate/--msrv given as an empty string)." >&2
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
copy_file ".github/workflows/nav-docs.yml"
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
copy_file "scripts/check-nav-docs.sh"

# AI-facing document routing (ADR-0016 in tarotene/dotfiles): AGENTS.md is
# the canon, CLAUDE.md just imports it. Content stays generic here — the
# repo-charter skill (tarotene/dotfiles) fills in the charter itself.
copy_file "AGENTS.md"
copy_file "CLAUDE.md"

# ADR-0000-rulesets-declaration-in-repo: required context の正本を対象
# リポジトリ自身の .github/rulesets/*.json に置く。security/workflow は
# repo-governance-common と共有(1本化済み)、quality は rust 固有の
# job 名を持つためこの skill 自身のテンプレートから。
copy_file ".github/rulesets/security.json"
copy_file ".github/rulesets/quality.json"
copy_file ".github/rulesets/workflow.json"
[[ "$WITH_REVIEW" == "true" ]] && copy_file ".github/rulesets/review.json"

if [[ "$WITH_FIRMWARE" != "true" && "$DRY_RUN" == "false" && -f "$DEST/.github/rulesets/quality.json" ]]; then
  # Firmware(cross-compile nRF52840-DK)は組み込みプロジェクト固有の
  # workflow(--with-firmware で初めてコピーされる)。--with-firmware
  # 無しではその workflow 自体が存在せず、required context として残すと
  # 永久に報告されない BLOCKED 事故になる(ADR-0000-rulesets-declaration-
  # in-repo が修正した事故クラスそのもの)。
  tmpfile="$(mktemp)"
  jq '.rules |= map(
        if .type == "required_status_checks"
        then .parameters.required_status_checks |= map(select((.context | startswith("Firmware (")) | not))
        else . end)' \
    "$DEST/.github/rulesets/quality.json" >"$tmpfile" && mv "$tmpfile" "$DEST/.github/rulesets/quality.json"
  echo "  (no --with-firmware given: dropped the Firmware required context from quality.json)"
fi

if [[ "$DRY_RUN" == "false" ]]; then
  verify_declaration "$DEST/.github/rulesets/security.json"
  verify_declaration "$DEST/.github/rulesets/quality.json"
  verify_declaration "$DEST/.github/rulesets/workflow.json"
  [[ "$WITH_REVIEW" == "true" ]] && verify_declaration "$DEST/.github/rulesets/review.json"
fi

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
echo "Review all '# ADJUST:' comments in the copied files before committing."
