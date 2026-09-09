#!/usr/bin/env bash
# plan-scope-gate.sh — ExitPlanMode 直前に、要求インベントリ(config/claude/CLAUDE.md
# の「複数項目の依頼は要求インベントリで受ける」節、config/claude/skills/
# scope-inventory/SKILL.md)の脱落を機械的に検査する hook。
#
# 設計と根拠: docs/claude/scope-inventory.md(このリポジトリ内)
#
# 指示文だけでは足りない(BAITBENCH: 明示的に禁止してもショートカット使用率は
# 平均50%超)ため、決定論的な検査を追加する。critic/judge の分離はしない —
# この gate は LLM を呼ばず、jq/grep/gh だけで判定する純粋な judge。
#
#   経路A(Issue起点の実カバレッジ): ユーザー自身が書いたメッセージから参照
#     Issue を抽出し、子(sub-issues、無ければ本文の未チェック task-list)が
#     2件以上ある Issue ごとに、プランが「実装対象として全子項目を処分」または
#     `Reference-Only: #N — <理由>` のどちらかを宣言しているかを検査する。
#   経路B(インベントリ内整合性): `## 要求インベントリ` 節が存在するときだけ、
#     各 `Rn` 行に処分(段の指定 or 閉じたタグ)があるか・タグが妥当か・
#     重複処分がないかを検査する。
#
#   - いずれかで問題が見つかれば deny(欠落項目を列挙してプラン修正を促す)。
#   - 問題なし / gh 不在 / 認証エラー / ネットワーク不通 / 対象 Issue 無し
#       → 何も決定しない(exit 0)= 通常の Approve ダイアログに進む。
#   - allow は決して返さない。
#
# 既知の限界(意図的な選択、docs/claude/scope-inventory.md 参照):
#   - checkbox フォールバックの「子項目」はテキストの部分一致でしか検査できない
#     (sub-issues の子番号ほど厳密ではない)。
#   - `Reference-Only: #N` は Issue の**番号だけ**で照合するため、異なる repo に
#     同じ番号の Issue が両方参照されている稀なケースでは両方が免除される。
#
# 使い方:
#   hook として: settings.json の PreToolUse(matcher: ExitPlanMode)から
#                stdin JSON で呼ばれる
#   手動 e2e:   plan-scope-gate.sh --check <plan.md> <issue-ref>
#               (issue-ref は `#N` または `owner/repo#N`。bare の場合は cwd の
#               git remote から owner/repo を解決する)
#   自己検査:   plan-scope-gate.sh --selftest(gh をスタブしネットワーク不使用)
#
# スキップ手段:
#   touch ~/.claude/plan-scope-gate/skip   または   SKIP_PLAN_SCOPE_GATE=1
#
# 既知バグ対策:
#   - ExitPlanMode hook は cwd=~ で走る → stdin JSON の .cwd へ明示 cd
#     (anthropics/claude-code#22343、copilot-plan-review.sh と同じ対策)。
#   - transcript には SessionStart hook の additionalContext 注入(issue-index 等)
#     が type=user の行に紛れ込むが、それらは message.content が array(tool_result
#     等)であり文字列ではない。ユーザーが実際に打った行だけを拾うため、
#     `type=="user" and (.message.content|type)=="string"` で厳密に絞る
#     (実測で確認済み。docs/claude/scope-inventory.md の罠1)。
set -u

GATE_DIR="${CLAUDE_PLAN_SCOPE_GATE_DIR:-$HOME/.claude/plan-scope-gate}"
CLOSED_TAGS_RE='(Blocked-Upstream|Obsolete|User-Excluded)'

# ---------------------------------------------------------------------------
# hook 出力(copilot-plan-review.sh と同じ契約)
# ---------------------------------------------------------------------------

pass_through() { # $1=警告メッセージ(省略可)
  local msg="${1:-}"
  [[ -n "$msg" ]] && jq -n --arg m "$msg" '{systemMessage: $m}'
  exit 0
}

deny_with() { # $1=理由(Claude に届く)
  local reason="$1"
  if [[ "${EVENT:-PreToolUse}" == "PermissionRequest" ]]; then
    jq -n --arg m "$reason" \
      '{hookSpecificOutput: {hookEventName: "PermissionRequest", decision: {behavior: "deny", message: $m}}}'
  else
    jq -n --arg m "$reason" \
      '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $m}}'
  fi
  exit 0
}

# ---------------------------------------------------------------------------
# 抽出・判定ロジック(gh を直接呼ばない部分は selftest でネットワーク無しに検査できる)
# ---------------------------------------------------------------------------

# $1=cwd ; origin の owner/repo を出力(git remote だけで判定、gh は呼ばない)
resolve_owner_repo() {
  local cwd="$1" url
  url="$(git -C "$cwd" remote get-url origin 2> /dev/null)" || return 0
  case "$url" in
    git@github.com:*) url="${url#git@github.com:}" ;;
    ssh://git@github.com/*) url="${url#ssh://git@github.com/}" ;;
    https://github.com/*) url="${url#https://github.com/}" ;;
    *) return 0 ;;
  esac
  printf '%s' "${url%.git}"
}

# $1=transcript_path ; ユーザー自身が打った行だけを改行区切りで出力
extract_user_text() {
  jq -r 'select(.type == "user" and (.message.content | type) == "string") | .message.content' \
    "$1" 2> /dev/null
}

# $1=text $2=default_owner_repo ; "owner/repo#N" を重複無しで出力
extract_issue_refs() {
  local text="$1" default="$2" ref
  grep -oE '[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+#[0-9]+|#[0-9]+' <<< "$text" 2> /dev/null \
    | while IFS= read -r ref; do
      if [[ $ref == */* ]]; then
        printf '%s\n' "$ref"
      elif [[ -n $default ]]; then
        printf '%s%s\n' "$default" "$ref"
      fi
    done \
    | sort -u
}

# $1="owner/repo#N" ; {"totalCount":N,"kind":"sub"|"checkbox"|"none","items":[...]} を出力
# 失敗(gh 不在・認証切れ・ネットワーク不通・不正な応答)は非0を返す — 呼び出し側は
# その参照だけを黙って諦める(fail-open、ADR-0005)。
fetch_children() {
  local ref="$1" ownerrepo number owner repo resp total
  ownerrepo="${ref%%#*}"
  number="${ref##*#}"
  owner="${ownerrepo%%/*}"
  repo="${ownerrepo#*/}"

  resp="$(gh api graphql -f query='
    query($owner:String!,$repo:String!,$number:Int!){
      repository(owner:$owner,name:$repo){
        issue(number:$number){
          subIssues(first:100){ totalCount nodes{ number title } }
        }
      }
    }' -f owner="$owner" -f repo="$repo" -F number="$number" 2> /dev/null)" || return 1
  jq -e '.data.repository.issue' > /dev/null 2>&1 <<< "$resp" || return 1

  total="$(jq -r '.data.repository.issue.subIssues.totalCount' <<< "$resp")"
  if [[ $total != "0" ]]; then
    jq '{totalCount: .data.repository.issue.subIssues.totalCount, kind: "sub",
         items: .data.repository.issue.subIssues.nodes}' <<< "$resp"
    return 0
  fi

  # フォールバック: 本文の未チェック task-list
  local body items_json n
  body="$(gh issue view "$number" --repo "$owner/$repo" --json body -q .body 2> /dev/null)" || {
    echo '{"totalCount":0,"kind":"none","items":[]}'
    return 0
  }
  items_json="$(grep -E '^[[:space:]]*[-*][[:space:]]*\[[[:space:]]\][[:space:]]*.+' <<< "$body" \
    | sed -E 's/^[[:space:]]*[-*][[:space:]]*\[[[:space:]]\][[:space:]]*//' \
    | jq -R -s 'split("\n") | map(select(length > 0)) | map({text: .})')"
  n="$(jq 'length' <<< "$items_json")"
  jq -n --argjson items "$items_json" --argjson n "$n" \
    '{totalCount: $n, kind: "checkbox", items: $items}'
}

# `## 要求インベントリ` 節の本文だけを出力(次の見出しまで)。無ければ何も出さない。
extract_inventory_section() {
  awk '
    BEGIN { insec = 0 }
    /^#+[[:space:]]*要求インベントリ/ { insec = 1; print; next }
    insec && /^#+[[:space:]]/ { exit }
    insec { print }
  ' <<< "$1"
}

# $1=ref $2=children_json $3=plan_body
# 出力(複数行): 1行目が SKIP|REFERENCE_ONLY|COVERED|MISSING、MISSING のときは
# 続く行に欠落項目を1行1件で列挙する。
judge_issue() {
  local ref="$1" children="$2" plan="$3" total num kind
  total="$(jq -r '.totalCount' <<< "$children")"
  case "$total" in
    '' | *[!0-9]*) echo SKIP; return 0 ;;
  esac
  if ((total < 2)); then
    echo SKIP
    return 0
  fi

  num="${ref##*#}"
  if grep -Eq "Reference-Only:[[:space:]]*([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#${num}([^0-9]|\$)" <<< "$plan"; then
    echo REFERENCE_ONLY
    return 0
  fi

  local section
  section="$(extract_inventory_section "$plan")"

  kind="$(jq -r '.kind' <<< "$children")"
  local -a missing=()
  if [[ $kind == sub ]]; then
    local cnum ctitle line
    while IFS=$'\t' read -r cnum ctitle; do
      line="$(grep -E "#${cnum}([^0-9]|\$)" <<< "$section" | head -1)"
      if [[ -z $line ]] || ! grep -Eq "(段[0-9]+|Stage[[:space:]]*[0-9]+|${CLOSED_TAGS_RE}:)" <<< "$line"; then
        missing+=("#${cnum} ${ctitle}")
      fi
    done < <(jq -r '.items[] | [.number, .title] | @tsv' <<< "$children")
  elif [[ $kind == checkbox ]]; then
    local ctext key
    while IFS= read -r ctext; do
      [[ ${#ctext} -ge 10 ]] || continue
      key="${ctext:0:40}"
      grep -qiF "$key" <<< "$plan" || missing+=("$ctext")
    done < <(jq -r '.items[].text' <<< "$children")
  fi

  if ((${#missing[@]} == 0)); then
    echo COVERED
  else
    echo MISSING
    printf '%s\n' "${missing[@]}"
  fi
}

# $1=plan_body ; 見つかった問題を1行1件で出力(無ければ何も出さない)。
# `## 要求インベントリ` 節が無いプランでは何もしない(存在しないことは deny しない)。
judge_inventory() {
  local plan="$1" section line id rest
  section="$(extract_inventory_section "$plan")"
  [[ -n $section ]] || return 0

  local rn_re='^[[:space:]]*[-*]?[[:space:]]*\*{0,2}R([0-9]+)\*{0,2}[:.)]'
  local -A seen=()
  while IFS= read -r line; do
    [[ $line =~ $rn_re ]] || continue
    id="${BASH_REMATCH[1]}"
    rest="${line#"${BASH_REMATCH[0]}"}"

    if [[ -n "${seen[$id]:-}" ]]; then
      printf 'R%s が複数回処分されています\n' "$id"
    fi
    seen[$id]=1

    if [[ $rest =~ (段[0-9]+|Stage[[:space:]]*[0-9]+) ]]; then
      : # 段の指定あり
    elif [[ $rest =~ ${CLOSED_TAGS_RE}: ]]; then
      : # 閉じたタグでの棄却
    elif [[ $rest =~ ^[[:space:]]*([A-Za-z][A-Za-z-]*): ]]; then
      printf 'R%s: 閉じたタグ集合に無い棄却タグ(%s:)が使われています\n' "$id" "${BASH_REMATCH[1]}"
    else
      printf 'R%s: 処分(実装する段、または棄却タグ)が未記載です\n' "$id"
    fi
  done <<< "$section"
}

# ---------------------------------------------------------------------------
# hook モード
# ---------------------------------------------------------------------------

main() {
  command -v gh > /dev/null 2>&1 || pass_through
  command -v jq > /dev/null 2>&1 || pass_through

  if [[ -e "$GATE_DIR/skip" || "${SKIP_PLAN_SCOPE_GATE:-0}" == "1" ]]; then
    pass_through
  fi

  local INPUT EVENT CWD
  INPUT="$(cat)"
  EVENT="$(jq -r '.hook_event_name // "PreToolUse"' <<< "$INPUT")"
  CWD="$(jq -r '.cwd // empty' <<< "$INPUT")"
  [[ -d $CWD ]] || CWD="$HOME"
  cd -- "$CWD" 2> /dev/null || true

  # プラン本文の取得: tool_input.plan → planFilePath → 最新の ~/.claude/plans/*.md
  local plan_text plan_path plan_body=""
  plan_text="$(jq -r '.tool_input.plan // empty' <<< "$INPUT")"
  plan_path="$(jq -r '.tool_input.planFilePath // empty' <<< "$INPUT")"
  if [[ -n $plan_text ]]; then
    plan_body="$plan_text"
  elif [[ -n $plan_path && -f $plan_path ]]; then
    plan_body="$(cat "$plan_path")"
  else
    local latest_plan
    latest_plan="$(ls -t "$HOME/.claude/plans/"*.md 2> /dev/null | head -1)"
    [[ -n $latest_plan ]] || pass_through
    plan_body="$(cat "$latest_plan")"
  fi

  local transcript owner_repo
  transcript="$(jq -r '.transcript_path // empty' <<< "$INPUT")"
  owner_repo="$(resolve_owner_repo "$CWD")"

  local -a deny_lines=()

  # --- 経路A: Issue 起点の実カバレッジ ---
  if [[ -n $transcript && -f $transcript ]]; then
    local user_text ref children status
    user_text="$(extract_user_text "$transcript")"
    while IFS= read -r ref; do
      [[ -n $ref ]] || continue
      children="$(fetch_children "$ref" 2> /dev/null)" || continue
      jq -e . > /dev/null 2>&1 <<< "$children" || continue

      local -a result
      mapfile -t result < <(judge_issue "$ref" "$children" "$plan_body")
      status="${result[0]:-SKIP}"
      if [[ $status == MISSING ]]; then
        deny_lines+=("参照 Issue ${ref} の子項目が要求インベントリに欠落しています(全子項目を処分するか、Reference-Only: ${ref} — <理由> を宣言してください):")
        local item
        for item in "${result[@]:1}"; do
          deny_lines+=("  - ${item}")
        done
      fi
    done < <(extract_issue_refs "$user_text" "$owner_repo")
  fi

  # --- 経路B: インベントリ内整合性 ---
  local inv_problem
  while IFS= read -r inv_problem; do
    [[ -n $inv_problem ]] && deny_lines+=("$inv_problem")
  done < <(judge_inventory "$plan_body")

  if ((${#deny_lines[@]} == 0)); then
    pass_through
  fi

  local msg
  msg="要求インベントリの検査で問題が見つかりました。scope-inventory スキルの手順に従って計画を修正してください。

$(printf '%s\n' "${deny_lines[@]}")"
  deny_with "$msg"
}

# ---------------------------------------------------------------------------
# --check(手動 e2e)
# ---------------------------------------------------------------------------

cmd_check() { # $1=plan_file $2=issue_ref
  local plan_file="$1" ref="$2" plan_body owner_repo children
  [[ -f $plan_file ]] || {
    echo "plan file not found: $plan_file" >&2
    exit 1
  }
  plan_body="$(cat "$plan_file")"

  if [[ $ref != */* ]]; then
    owner_repo="$(resolve_owner_repo "$PWD")"
    [[ -n $owner_repo ]] || {
      echo "cwd is not inside a GitHub-remote repo; pass owner/repo#N explicitly" >&2
      exit 1
    }
    ref="${owner_repo}${ref}"
  fi

  children="$(fetch_children "$ref")" || {
    echo "failed to fetch children for $ref (gh 不在・未認証・ネットワーク不通のいずれか)" >&2
    exit 1
  }
  echo "children: $(jq -c . <<< "$children")"
  judge_issue "$ref" "$children" "$plan_body"
}

# ---------------------------------------------------------------------------
# --selftest(gh をスタブし、ネットワークを使わない)
# ---------------------------------------------------------------------------

selftest() {
  local fails=0
  expect_eq() { # $1=actual $2=expected $3=label
    [[ "$1" == "$2" ]] || {
      echo "FAIL(${3}): expected [${2}] got [${1}]" >&2
      fails=$((fails + 1))
    }
  }

  # --- extract_issue_refs: bare と owner/repo 混在、重複除去、辞書順 ---
  local refs
  refs="$(extract_issue_refs '#136 の話と owner2/repo2#5 も参照。#136 は再掲。' 'tarotene/dotfiles')"
  expect_eq "$refs" "$(printf 'owner2/repo2#5\ntarotene/dotfiles#136')" "extract_issue_refs"

  # --- resolve_owner_repo: ssh / https 形式 ---
  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "${tmp:-}"' EXIT
  git -C "$tmp" init -q
  git -C "$tmp" remote add origin git@github.com:foo/bar.git
  expect_eq "$(resolve_owner_repo "$tmp")" "foo/bar" "resolve_owner_repo(ssh)"
  git -C "$tmp" remote set-url origin https://github.com/foo/bar.git
  expect_eq "$(resolve_owner_repo "$tmp")" "foo/bar" "resolve_owner_repo(https)"

  # --- judge_issue: 子0/1件 → SKIP ---
  expect_eq "$(judge_issue 'o/r#1' '{"totalCount":0,"kind":"none","items":[]}' '')" "SKIP" "judge_issue(子0件)"
  expect_eq "$(judge_issue 'o/r#1' '{"totalCount":1,"kind":"sub","items":[{"number":9,"title":"x"}]}' '')" "SKIP" "judge_issue(子1件)"

  # --- judge_issue: sub、全カバー ---
  local children plan
  children='{"totalCount":2,"kind":"sub","items":[{"number":137,"title":"t1"},{"number":138,"title":"t2"}]}'
  plan=$'## 要求インベントリ\n- R1: #137 は段1で実装\n- R2: #138 は段2で実装\n'
  local -a result
  mapfile -t result < <(judge_issue 'tarotene/dotfiles#136' "$children" "$plan")
  expect_eq "${result[0]}" "COVERED" "judge_issue(sub, covered)"

  # --- judge_issue: sub、1件欠落 ---
  plan=$'## 要求インベントリ\n- R1: #137 は段1で実装\n'
  mapfile -t result < <(judge_issue 'tarotene/dotfiles#136' "$children" "$plan")
  expect_eq "${result[0]}" "MISSING" "judge_issue(sub, missing 件数=1 の状態)"
  expect_eq "${#result[@]}" "2" "judge_issue(sub, missing 行数)"

  # --- judge_issue: Reference-Only 宣言で免除 ---
  plan=$'依頼文\nReference-Only: #136 — 参考のみで実装対象ではない\n'
  mapfile -t result < <(judge_issue 'tarotene/dotfiles#136' "$children" "$plan")
  expect_eq "${result[0]}" "REFERENCE_ONLY" "judge_issue(Reference-Only 宣言あり)"

  # --- judge_issue: 宣言なしで実装対象扱い(欠落) ---
  plan=$'依頼文だけで要求インベントリを書いていない\n'
  mapfile -t result < <(judge_issue 'tarotene/dotfiles#136' "$children" "$plan")
  expect_eq "${result[0]}" "MISSING" "judge_issue(Reference-Only 宣言なし・インベントリ節も無し)"

  # --- judge_issue: checkbox フォールバック(部分一致) ---
  children='{"totalCount":2,"kind":"checkbox","items":[{"text":"child item one two three four five six"},{"text":"another distinct item text entirely"}]}'
  plan="計画本文には child item one two three four five six は載っているが、もう一方は無い"
  mapfile -t result < <(judge_issue 'o/r#1' "$children" "$plan")
  expect_eq "${result[0]}" "MISSING" "judge_issue(checkbox, 片方欠落)"
  expect_eq "${#result[@]}" "2" "judge_issue(checkbox, missing 行数)"

  # --- judge_inventory: 正常系(段指定・閉じたタグとも OK) ---
  plan=$'## 要求インベントリ\n- R1: 段1で実装\n- R2: Obsolete: もう不要\n'
  local -a problems
  mapfile -t problems < <(judge_inventory "$plan")
  expect_eq "${#problems[@]}" "0" "judge_inventory(正常系)"

  # --- judge_inventory: 節が無い → 何もしない ---
  mapfile -t problems < <(judge_inventory $'依頼文だけ\n')
  expect_eq "${#problems[@]}" "0" "judge_inventory(節なし)"

  # --- judge_inventory: 重複処分・不正タグ・処分未記載 ---
  plan=$'## 要求インベントリ\n- R1: 段1で実装\n- R1: 段2でも実装\n- R2: Conflicts: 何か\n- R3: よくわからない項目\n'
  mapfile -t problems < <(judge_inventory "$plan")
  expect_eq "${#problems[@]}" "3" "judge_inventory(異常系 件数)"

  # --- fetch_children: gh をスタブしてネットワークを使わない ---
  gh() {
    case "$*" in
      *"api graphql"*)
        echo '{"data":{"repository":{"issue":{"subIssues":{"totalCount":0,"nodes":[]}}}}}'
        ;;
      *"issue view"*)
        printf '%s\n' '- [ ] child item one two three four five' '- [x] done item'
        ;;
    esac
  }
  local children_out
  children_out="$(fetch_children 'o/r#1')"
  expect_eq "$(jq -r '.kind' <<< "$children_out")" "checkbox" "fetch_children(checkbox フォールバック)"
  expect_eq "$(jq -r '.totalCount' <<< "$children_out")" "1" "fetch_children(未チェックのみ数える)"
  unset -f gh

  # --- transcript 汚染耐性: issue-index 注入(tool_result)を無視する ---
  local jsonl_tmp
  jsonl_tmp="$(mktemp)"
  {
    printf '%s\n' '{"type":"user","message":{"content":[{"type":"tool_result","content":"issue-index: #999 何か"}]}}'
    printf '%s\n' '{"type":"user","message":{"content":"依頼: #136 をお願い"}}'
  } > "$jsonl_tmp"
  local extracted
  extracted="$(extract_user_text "$jsonl_tmp")"
  if grep -q '#999' <<< "$extracted"; then
    echo "FAIL: issue-index 注入(tool_result)を誤って拾った" >&2
    fails=$((fails + 1))
  fi
  if ! grep -q '#136' <<< "$extracted"; then
    echo "FAIL: 本来のユーザー入力(#136)を取り逃した" >&2
    fails=$((fails + 1))
  fi
  rm -f "$jsonl_tmp"

  if ((fails > 0)); then
    echo "selftest: ${fails} 件失敗" >&2
    exit 1
  fi
  echo "selftest: OK"
}

case "${1-}" in
  --selftest) selftest ;;
  --check)
    shift
    cmd_check "${1-}" "${2-}"
    ;;
  *) main ;;
esac
