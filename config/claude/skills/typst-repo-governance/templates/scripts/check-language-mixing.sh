#!/usr/bin/env bash
# check-language-mixing.sh — fail if any git-tracked markdown file mixes
# CJK and Latin script above a small threshold (ADR-0016 in
# tarotene/dotfiles: README/CONTRIBUTING are single-language; other
# markdown files must each be one language too). Provisional heuristic —
# swap for a textlint-class linter if one is found suitable.
set -euo pipefail

THRESHOLD="${LANG_MIX_THRESHOLD:-20}"
fail=0

strip_for_lang() {
  awk '
    /^```/ { infence = !infence; next }
    infence { next }
    { print }
  ' "$1" | sed -E 's/`[^`]*`//g; s#https?://[^ )]+##g'
}

while IFS= read -r -d '' file; do
  stripped="$(strip_for_lang "$file")"
  cjk="$(grep -oP '[\x{3040}-\x{30FF}\x{4E00}\x{9FFF}\x{3400}-\x{4DBF}]' <<<"$stripped" 2>/dev/null | wc -l || true)"
  latin="$(grep -oP '[A-Za-z]' <<<"$stripped" 2>/dev/null | wc -l || true)"
  if [[ $cjk -ge $THRESHOLD && $latin -ge $THRESHOLD ]]; then
    echo "::error file=$file::language-mixed (cjk=$cjk latin=$latin, threshold=$THRESHOLD)"
    fail=1
  fi
done < <(git ls-files -z -- '*.md')

exit "$fail"
