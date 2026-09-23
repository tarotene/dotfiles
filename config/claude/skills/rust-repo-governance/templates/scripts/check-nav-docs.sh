#!/usr/bin/env bash
# check-nav-docs.sh — fail if a navigational document (the root README.md /
# CONTRIBUTING.md / AGENTS.md / CLAUDE.md, or any `<dir>/README.md`)
# materialises a hand-drawn directory tree or a hand-written file/content
# inventory (ADR-0033 in tarotene/dotfiles: navigational documents point at
# the source of truth, they don't copy it — the token count/name stay
# accurate only for as long as someone remembers to edit both places).
# Transcribed file contents (a README copy of another file's schema) is a
# third violation ADR-0033 names, but it needs semantic comparison against
# the source file to detect, which this deterministic check can't do — see
# the `github-audit` charters domain's nav-doc-* tokens (tarotene/dotfiles)
# for that pairing between a decision-time skill and a review-time LLM
# node; this script only re-implements the two mechanical checks.
#
# The mechanical logic (nav_doc_path_tokens/nav_doc_scan) is duplicated
# from scripts/github-audit in tarotene/dotfiles rather than sourced, same
# as check-language-mixing.sh's independent CJK-count reimplementation —
# a downstream repo shouldn't need github-audit installed to run its own
# CI. Keep the two in sync by hand; there is no single source for bash
# functions across repositories.
set -euo pipefail

NAV_DOC_PATH_THRESHOLD="${NAV_DOC_PATH_THRESHOLD:-4}"
NAV_DOC_PATH_EXTENSIONS='\.(md|yml|yaml|typ|toml|json|sh|nix|rs|py|ts|js|lock|tsv|bib)$'
NAV_DOC_TREE_CHARS_PATTERN='[\x{251C}\x{2514}\x{2502}]'

# nav_doc_path_tokens: $1=section body text -> one unique path-like token
# per line. Guarded with `|| true` because grep exits 1 on "zero matches",
# which is the common (not exceptional) case for a section with no path
# mentions at all — under `set -o pipefail`, an unguarded empty result
# would abort the caller instead of just meaning "no tokens".
nav_doc_path_tokens() {
  awk '
    /^```/ { infence = !infence; next }
    infence {
      n = split($0, parts, /[[:space:]]+/)
      if (n >= 1 && parts[1] != "") print parts[1]
      next
    }
    {
      s = $0
      while (match(s, /`[^`]+`/)) {
        tok = substr(s, RSTART + 1, RLENGTH - 2)
        if (tok !~ /[[:space:]]/) print tok
        s = substr(s, RSTART + RLENGTH)
      }
    }
  ' <<<"$1" \
    | grep -vE '^https?://' \
    | grep -E "(/|${NAV_DOC_PATH_EXTENSIONS})" \
    | sed -E 's#/$##' \
    | sort -u || true
}

# nav_doc_scan: $1=file label (relative path) $2=doc text -> newline-
# separated finding tokens: nav-doc-tree-fence:<file> / nav-doc-path-
# inventory:<file>:<heading>:<n> / nav-doc-exempt-malformed:<file> /
# nav-doc-exempt-unused:<file>:<check>. Empty input yields no findings.
#
# `<!-- nav-doc-exempt: <check> — <reason> -->` (check is path-inventory or
# tree-fence) exempts the block immediately following the marker — the
# whole fence if the next line opens one, otherwise contiguous lines up to
# the next blank line or heading — from that one check only. A marker
# missing the check name or the reason is nav-doc-exempt-malformed. A
# marker whose guarded block would not have drifted anyway (the fence has
# no tree characters; the section's raw, unexempted count is still below
# threshold) is nav-doc-exempt-unused — mirroring ESLint's
# reportUnusedDisableDirectives / Ruff's RUF100.
nav_doc_scan() {
  local file="$1" text="$2"
  [[ -n $text ]] || return 0

  local -a lines
  mapfile -t lines <<<"$text"
  local n=${#lines[@]}
  ((n > 0)) || return 0

  # Section index per line (0-based; section 0 is content before the first
  # heading of any level 1-6), and the heading text starting each section.
  local -a section_idx=() section_heading=("(intro)")
  local cur=0 k
  for ((k = 0; k < n; k++)); do
    if [[ "${lines[$k]}" =~ ^#{1,6}[[:space:]](.*)$ ]]; then
      cur=$((cur + 1))
      section_heading+=("${BASH_REMATCH[1]}")
    fi
    section_idx[k]=$cur
  done
  local n_sections=$((cur + 1))

  # Locate nav-doc-exempt markers and each one's guarded-block line range
  # (0-based, inclusive; end < start means "guards nothing").
  local -a m_check=() m_start=() m_end=() m_malformed=()
  for ((k = 0; k < n; k++)); do
    local trimmed="${lines[$k]}"
    trimmed="${trimmed#"${trimmed%%[![:space:]]*}"}"
    trimmed="${trimmed%"${trimmed##*[![:space:]]}"}"
    [[ $trimmed == '<!-- nav-doc-exempt:'*'-->' ]] || continue

    local content="${trimmed#<!-- nav-doc-exempt:}"
    content="${content%-->}"
    content="${content#"${content%%[![:space:]]*}"}"
    content="${content%"${content##*[![:space:]]}"}"

    local check='' malformed=1
    if [[ $content == *' — '* ]]; then
      local c="${content%% — *}" r="${content#* — }"
      c="${c%"${c##*[![:space:]]}"}"
      r="${r#"${r%%[![:space:]]*}"}"
      if { [[ $c == "path-inventory" ]] || [[ $c == "tree-fence" ]]; } && [[ -n $r ]]; then
        check="$c"
        malformed=0
      fi
    fi

    local start=$((k + 1)) end
    if ((malformed == 0)); then
      if ((start < n)) && [[ "${lines[$start]}" == '```'* ]]; then
        end=$start
        local j
        for ((j = start + 1; j < n; j++)); do
          if [[ "${lines[$j]}" == '```'* ]]; then
            end=$j
            break
          fi
        done
      else
        local j=$start
        while ((j < n)); do
          local bl="${lines[$j]}"
          [[ -z $bl ]] && break
          [[ $bl =~ ^#{1,6}[[:space:]] ]] && break
          j=$((j + 1))
        done
        end=$((j - 1))
      fi
    else
      end=$((start - 1))
    fi
    m_check+=("$check")
    m_start+=("$start")
    m_end+=("$end")
    m_malformed+=("$malformed")
  done

  # Blank out each valid marker's guarded range, per check type, so the
  # scans below skip exactly what was exempted and nothing else.
  local -a lines_path=("${lines[@]}") lines_tree=("${lines[@]}")
  local mi
  for ((mi = 0; mi < ${#m_check[@]}; mi++)); do
    ((m_malformed[mi] == 0)) || continue
    local s=${m_start[mi]} e=${m_end[mi]}
    ((e >= s)) || continue
    local idx
    for ((idx = s; idx <= e; idx++)); do
      if [[ ${m_check[mi]} == "path-inventory" ]]; then
        lines_path[idx]=""
      else
        lines_tree[idx]=""
      fi
    done
  done

  local -a findings=()

  # tree-fence: any fence (post path/tree exemption) containing box-drawing
  # characters is a hand-drawn directory tree. File-level, not per-section.
  local infence=0
  for ((k = 0; k < n; k++)); do
    local l="${lines_tree[$k]}"
    if [[ $l == '```'* ]]; then
      infence=$((1 - infence))
      continue
    fi
    if ((infence)) && grep -qP "$NAV_DOC_TREE_CHARS_PATTERN" <<<"$l"; then
      findings+=("nav-doc-tree-fence:$file")
      break
    fi
  done

  # path-inventory: per-section unique path-token count, on both the
  # exempted lines (what actually drifts) and the raw lines (the baseline
  # used below to tell a used exemption from an unused one).
  local -a section_raw_count=()
  local si
  for ((si = 0; si < n_sections; si++)); do
    local body_exempted='' body_raw=''
    for ((k = 0; k < n; k++)); do
      ((section_idx[k] == si)) || continue
      body_exempted+="${lines_path[$k]}"$'\n'
      body_raw+="${lines[$k]}"$'\n'
    done
    local count_exempted count_raw
    count_exempted="$(nav_doc_path_tokens "$body_exempted" | wc -l | tr -d '[:space:]')"
    count_raw="$(nav_doc_path_tokens "$body_raw" | wc -l | tr -d '[:space:]')"
    if ((count_exempted >= NAV_DOC_PATH_THRESHOLD)); then
      findings+=("nav-doc-path-inventory:$file:${section_heading[$si]}:$count_exempted")
    fi
    section_raw_count[si]=$count_raw
  done

  # Marker bookkeeping: malformed (missing check name or reason) and unused
  # (the guarded block would not have drifted even without the exemption).
  local any_malformed=0
  for ((mi = 0; mi < ${#m_check[@]}; mi++)); do
    if ((m_malformed[mi] == 1)); then
      any_malformed=1
      continue
    fi
    if [[ ${m_check[mi]} == "tree-fence" ]]; then
      local has_tree=0 idx2
      for ((idx2 = m_start[mi]; idx2 <= m_end[mi]; idx2++)); do
        ((idx2 >= 0 && idx2 < n)) || continue
        grep -qP "$NAV_DOC_TREE_CHARS_PATTERN" <<<"${lines[$idx2]}" && has_tree=1
      done
      ((has_tree == 1)) || findings+=("nav-doc-exempt-unused:$file:tree-fence")
    else
      local sec=0
      ((m_start[mi] < n)) && sec=${section_idx[${m_start[mi]}]}
      ((section_raw_count[sec] >= NAV_DOC_PATH_THRESHOLD)) || findings+=("nav-doc-exempt-unused:$file:path-inventory")
    fi
  done
  ((any_malformed == 0)) || findings+=("nav-doc-exempt-malformed:$file")

  ((${#findings[@]} > 0)) && printf '%s\n' "${findings[@]}"
  return 0
}

fail=0

# Root allowlist (ADR-0016) plus every subdirectory README — the one thing
# this per-repo check covers that github-audit's GraphQL fetch doesn't
# (it only reads the four root files).
targets=()
for f in README.md CONTRIBUTING.md AGENTS.md CLAUDE.md; do
  [[ -f $f ]] && targets+=("$f")
done
while IFS= read -r -d '' f; do
  targets+=("$f")
done < <(git ls-files -z -- '*/README.md')

for file in "${targets[@]}"; do
  text="$(cat "$file")"
  while IFS= read -r finding; do
    [[ -n $finding ]] || continue
    echo "::error file=$file::$finding"
    fail=1
  done < <(nav_doc_scan "$file" "$text")
done

exit "$fail"
