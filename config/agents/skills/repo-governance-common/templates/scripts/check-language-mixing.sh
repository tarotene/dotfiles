#!/usr/bin/env bash
# check-language-mixing.sh — fail if any git-tracked markdown file mixes
# CJK and Latin script above a small threshold (ADR-0016 in
# tarotene/dotfiles: README/CONTRIBUTING are single-language; other
# markdown files must each be one language too). Provisional heuristic —
# swap for a textlint-class linter if one is found suitable.
#
# 検査対象は環境変数 LANG_MIX_PATHS(空白区切りの git pathspec)で絞れる。
# 未設定は従来どおり全 tracked markdown(`*.md`)。日本語が正本の文書群を
# 持つリポジトリは、単一言語を義務づける文書(README/CONTRIBUTING 等)だけ
# を渡す。指定した pathspec が 1 件も markdown に当たらなければ、typo で
# 検査が黙って空振りするのを避けるため失敗させる。
#
# このファイルは repo-governance-common の templates が単一ソースで、
# typst/rust/astro の各テンプレートはここへの symlink。
set -euo pipefail

THRESHOLD="${LANG_MIX_THRESHOLD:-20}"

strip_for_lang() {
  awk '
    /^```/ { infence = !infence; next }
    infence { next }
    { print }
  ' "$1" | sed -E 's/`[^`]*`//g; s#https?://[^ )]+##g'
}

# $1 の CJK 文字数と Latin 文字数を "cjk latin" の形で出す。範囲は
# ひらがな・カタカナ(3040-30FF)、CJK 統合漢字(4E00-9FFF)、拡張 A(3400-4DBF)。
count_scripts() {
  local stripped cjk latin
  stripped="$(strip_for_lang "$1")"
  cjk="$(grep -oP '[\x{3040}-\x{30FF}\x{4E00}-\x{9FFF}\x{3400}-\x{4DBF}]' <<<"$stripped" 2>/dev/null | wc -l || true)"
  latin="$(grep -oP '[A-Za-z]' <<<"$stripped" 2>/dev/null | wc -l || true)"
  echo "$cjk $latin"
}

run_check() {
  local fail=0 matched=0 file cjk latin
  local -a pathspecs
  read -r -a pathspecs <<<"${LANG_MIX_PATHS:-*.md}"
  while IFS= read -r -d '' file; do
    [[ $file == *.md ]] || continue
    matched=$((matched + 1))
    read -r cjk latin <<<"$(count_scripts "$file")"
    if [[ $cjk -ge $THRESHOLD && $latin -ge $THRESHOLD ]]; then
      echo "::error file=$file::language-mixed (cjk=$cjk latin=$latin, threshold=$THRESHOLD)"
      fail=1
    fi
  done < <(git ls-files -z -- "${pathspecs[@]}")
  if [[ -n ${LANG_MIX_PATHS:-} && $matched -eq 0 ]]; then
    echo "::error::LANG_MIX_PATHS='${LANG_MIX_PATHS}' matched no tracked markdown file" >&2
    return 2
  fi
  return "$fail"
}

selftest() {
  local tmp fails=0 rc
  tmp="$(mktemp -d)"
  git -C "$tmp" init -q -b main
  git -C "$tmp" config core.hooksPath /dev/null
  local jp_kanji="漢字漢字漢字漢字漢字漢字漢字漢字漢字漢字漢字漢字漢字漢字漢字漢字漢字漢字漢字漢字"
  printf 'This file is written only in English. %.0s' {1..3} > "$tmp/en.md"
  printf '%s\n' "$jp_kanji" > "$tmp/kanji-only.md"
  printf '%s\nThis paragraph mixes English words into a Japanese note.\n' "$jp_kanji" > "$tmp/mixed.md"
  git -C "$tmp" add -A

  check() { # $1=名前 $2=want-rc -- $3..=環境変数(KEY=VAL)
    local name="$1" want="$2"
    shift 2
    rc=0
    (cd "$tmp" && env "$@" bash "$SELF" > /dev/null 2>&1) || rc=$?
    if [[ $rc == "$want" ]]; then
      echo "ok   $name"
    else
      echo "FAIL $name (expected rc=$want got rc=$rc)" >&2
      fails=$((fails + 1))
    fi
  }

  # 漢字だけの文書は、漢字が数えられるようになった後も英字が 0 なので通る。
  # 漢字+英語の混在は、かな・カタカナが無くても検出される(旧正規表現は
  # 4E00 と 9FFF の 2 文字しか数えず、この混在を見逃していた)。
  check "1 全 md を対象にすると混在ファイルで失敗" 1
  check "2 漢字+英語の混在を検出(旧: 漢字が数えられず見逃し)" 1 LANG_MIX_PATHS=mixed.md
  check "3 単一言語(英語)のファイルは通過" 0 LANG_MIX_PATHS=en.md
  check "4 単一言語(漢字のみ)のファイルは通過" 0 LANG_MIX_PATHS=kanji-only.md
  check "5 pathspec は空白区切りで複数指定できる" 0 "LANG_MIX_PATHS=en.md kanji-only.md"
  check "6 混在ファイルを含む pathspec は失敗" 1 "LANG_MIX_PATHS=en.md mixed.md"
  check "7 どの markdown にも当たらない pathspec は失敗(空振り防止)" 2 LANG_MIX_PATHS=missing.md

  rm -rf "$tmp"
  if [[ $fails -gt 0 ]]; then
    echo "selftest: ${fails} 件失敗" >&2
    return 1
  fi
  echo "selftest: OK"
}

SELF="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

if [[ ${1-} == --selftest ]]; then
  selftest
else
  run_check
fi
