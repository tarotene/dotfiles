#!/usr/bin/env bash
# apply-rulesets.sh — apply a GitHub repository's own declared rulesets
# (<repo>/.github/rulesets/{security,quality,workflow}[,review].json) to
# that same repository's live branch rulesets.
#
# 設計と根拠(ADR-0000-rulesets-declaration-in-repo — 段3で改番):
# required_status_checks の正本を「対象リポジトリの外(このリポジトリの
# *-repo-governance skill テンプレート)」に置いていたことが telepath#243
# を含む複数リポジトリの BLOCKED 事故の直接原因だった — テンプレートの
# required context が対象リポジトリの実際のジョブ名と同じ PR で編集される
# 保証が無いため。
#
# この script は「型」を引数に取らない(旧: rust/typst/astro/core/dotfiles
# の 5 型 + 3 skill それぞれの apply-rulesets.sh + 共通コア
# _rulesets-apply-core.sh の 4 層構成だった)。正本を対象リポジトリの
# `.github/rulesets/*.json` そのものに一本化したので、apply はどのリポ
# ジトリに対しても同じ処理で済む(D4「還元」)。
#
# PUT/POST の直前に、quality.json の required_status_checks[].context を
# 「検証対象コミットが実際に報告する job 名」と突合する(D5)。ここでの
# 「検証対象コミット」は既定で対象 ref に対する最新 PR の head SHA —
# default branch の squash commit 自体には pull_request 系の run が存在
# しないため(D14、on.pull_request トリガーは push を発火しない)。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./github-audit
source "$SCRIPT_DIR/github-audit"

# github-audit の全域変数 OWNER は main() の冒頭で owner_repo の owner
# 部分に上書きする — fetch_run_job_names() 等の github-audit 側ヘルパーが
# "repos/$OWNER/$1/..." を組み立てるため、ここが対象リポジトリの owner と
# ずれると常に 404 になる(自己テストで実際に踏んだ)。
usage() {
  cat <<'EOF'
usage: apply-rulesets.sh <owner/repo> [--ref REF] [--from-dir DIR]
                          [--verify-sha SHA] [--reconcile] [--dry-run]
                          [--unverified-contexts] [--delete-ruleset NAME]
       apply-rulesets.sh --selftest

対象リポジトリ自身が .github/rulesets/{security,quality,workflow}[,review].json
として持つ宣言を、そのリポジトリの live な branch ruleset に適用する。

宣言の取得元:
  既定          repos/<owner>/<repo>/contents/.github/rulesets を --ref
                (省略時は default branch)から読む。
  --from-dir    ローカルディレクトリから読む(seed 直後、まだ commit/push
                していない宣言を検証したいとき用)。

検証(D5): quality.json の required_status_checks[].context を、検証対象
コミットが実際に報告する job 名(Actions API 実測、YAML は静的パースしない
— D3)と突合する。報告されない context があれば拒否する(exit 4)。
検証対象コミットは既定で --ref に対する最新 PR の head SHA
(--verify-sha で明示指定可、D14: default branch の squash commit 自体には
pull_request 系の run が存在しないため)。--unverified-contexts で拒否を
スキップできる(seed 直後、まだ PR が無い時専用の明示的な脱出)。

--reconcile 無し: 名前一致で既存 ruleset を skip(create-only)。
--reconcile 有り: 名前一致で PUT(更新)。宣言に無い active な branch
  ruleset は報告のみ(削除しない)。

--delete-ruleset NAME: 宣言に同名が無いことを確認した上でその ruleset を
  DELETE する(旧 governance skill 群の --remove-review の後継。review 層
  を剥がすときに使う: review.json を宣言から消してから
  `--delete-ruleset Review` を実行する)。

書込み系の gh api 呼び出しは PreToolUse hook(crates/rulesets-write-guard)
の対象になるため、直前に RULESETS_WRITE_GUARD_BYPASS=1 を export する。
EOF
}

# $1=owner/repo -> default branch
resolve_ref() {
  local owner_repo="$1"
  "$GH_BIN" api "repos/$owner_repo" --jq '.default_branch' 2>/dev/null
}

# $1=owner/repo -> 最新 PR(state=all, updated 降順)の head SHA。無ければ空。
resolve_verify_sha() {
  local owner_repo="$1"
  "$GH_BIN" api "repos/$owner_repo/pulls" -X GET -F state=all -F sort=updated -F direction=desc -F per_page=1 \
    --jq '.[0].head.sha // empty' 2>/dev/null || printf ''
}

# $1=text -> 1 if any __UPPER_SNAKE__ placeholder remains
has_placeholder() {
  grep -qE '__[A-Z_]+__' <<<"$1"
}

# $1=owner/repo $2=ref $3=from_dir(空文字なら remote) $4=out_dir
# -> out_dir に security.json/quality.json/workflow.json[/review.json] を書く。
# security/quality/workflow が揃わなければ exit 3。
load_declarations() {
  local owner_repo="$1" ref="$2" from_dir="$3" out_dir="$4"
  mkdir -p "$out_dir"

  if [[ -n "$from_dir" ]]; then
    local f name
    for name in security quality workflow review; do
      f="$from_dir/$name.json"
      [[ -r "$f" ]] && cp "$f" "$out_dir/$name.json"
    done
  else
    local names name content
    names="$("$GH_BIN" api "repos/$owner_repo/contents/.github/rulesets" -X GET -F "ref=$ref" \
      --jq '[.[].name]' 2>/dev/null || printf '[]')"
    while IFS= read -r name; do
      [[ -n "$name" ]] || continue
      case "$name" in
        security.json | quality.json | workflow.json | review.json) ;;
        *) continue ;;
      esac
      content="$("$GH_BIN" api "repos/$owner_repo/contents/.github/rulesets/$name" -X GET -F "ref=$ref" \
        --jq '.content' 2>/dev/null || printf '')"
      [[ -n "$content" ]] || continue
      base64 -d <<<"$content" >"$out_dir/$name" 2>/dev/null || true
    done < <(jq -r '.[]?' <<<"$names")
  fi

  local missing=()
  local f
  for f in security quality workflow; do
    [[ -r "$out_dir/$f.json" ]] || missing+=("$f.json")
  done
  if ((${#missing[@]} > 0)); then
    echo "ERROR: $owner_repo@$ref has no .github/rulesets/ declaration (missing: ${missing[*]})." >&2
    echo "  seed it first (copy-files.sh of the matching *-repo-governance skill, or copy" >&2
    echo "  .github/rulesets/ by hand), then re-run." >&2
    return 3
  fi
}

# $1=declaration_dir -> exit 5 if any file has an unreplaced __X__ placeholder
check_no_placeholders() {
  local dir="$1" f name body
  for f in "$dir"/*.json; do
    [[ -e "$f" ]] || continue
    name="$(basename "$f")"
    body="$(cat "$f")"
    if has_placeholder "$body"; then
      echo "ERROR: $name still has an unreplaced placeholder (__X__) after substitution." >&2
      return 5
    fi
  done
}

# $1=quality.json -> JSON array of required_status_checks[].context
declared_contexts() {
  jq -c '[.rules[]? | select(.type=="required_status_checks") | .parameters.required_status_checks[]?.context] | unique' "$1" 2>/dev/null || printf '[]'
}

# $1=owner_repo $2=sha $3=contexts_json -> 0 if all contexts are reportable,
# else prints the unreportable ones and returns 1.
verify_contexts() {
  local owner_repo="$1" sha="$2" contexts="$3" jobs unreportable n
  if [[ -z "$sha" ]]; then
    echo "  (検証対象コミットが無いため context の実測を行いません)" >&2
    return 2
  fi
  jobs="$(fetch_head_sha_job_names "${owner_repo#*/}" "$sha")"
  unreportable="$(jq -c --argjson jobs "$jobs" '. - $jobs' <<<"$contexts")"
  n="$(jq 'length' <<<"$unreportable")"
  if ((n > 0)); then
    echo "  以下の required context は $sha の実測 job 名に含まれません:" >&2
    jq -r '.[] | "    - \(.)"' <<<"$unreportable" >&2
    return 1
  fi
  return 0
}

# $1=name $2=declaration_dir/security or quality or workflow or review .json path -> apply one ruleset
apply_one_ruleset() {
  local owner_repo="$1" existing="$2" f="$3"
  local name body existing_id
  name="$(jq -r '.name' "$f")"
  body="$(cat "$f")"
  existing_id="$(jq -r --arg n "$name" '.[] | select(.name == $n) | .id' <<<"$existing" | head -1)"

  if [[ -z "$existing_id" ]]; then
    if [[ "$DRY_RUN" == "true" ]]; then
      echo "  DRY-RUN: would POST ruleset '$name'"
    else
      local result id
      result="$(echo "$body" | RULESETS_WRITE_GUARD_BYPASS=1 "$GH_BIN" api -X POST "repos/$owner_repo/rulesets" --input -)"
      id="$(jq -r '.id' <<<"$result")"
      echo "  ✓  Created '$name' (id=$id)"
    fi
    return
  fi

  if [[ "$RECONCILE" != "true" ]]; then
    echo "  ⚠   '$name' already exists (id=$existing_id) — skipping (pass --reconcile to update)."
    return
  fi

  if [[ "$DRY_RUN" == "true" ]]; then
    echo "  DRY-RUN: would PUT '$name' (id=$existing_id)"
  else
    echo "$body" | RULESETS_WRITE_GUARD_BYPASS=1 "$GH_BIN" api -X PUT "repos/$owner_repo/rulesets/$existing_id" --input - >/dev/null
    echo "  ✓  Reconciled '$name' (id=$existing_id)"
  fi
}

# 新たに required になった context について、その context を報告しない
# open PR を列挙して警告する(bleep#32 型の時間差クラス — required 追加
# 前に開いた PR の head には、新しく required にした workflow が走って
# いないので、apply 後もそのまま BLOCKED になる)。
warn_open_prs_missing_contexts() {
  local owner_repo="$1" contexts="$2" prs sha number title jobs missing
  prs="$("$GH_BIN" api "repos/$owner_repo/pulls" -X GET -F state=open -F per_page=50 \
    --jq '[.[] | {number, title, sha: .head.sha}]' 2>/dev/null || printf '[]')"
  local n
  n="$(jq 'length' <<<"$prs")"
  ((n > 0)) || return 0
  while IFS=$'\t' read -r number sha title; do
    [[ -n "$number" ]] || continue
    jobs="$(fetch_head_sha_job_names "${owner_repo#*/}" "$sha")"
    missing="$(jq -c --argjson jobs "$jobs" '. - $jobs' <<<"$contexts")"
    if [[ "$(jq 'length' <<<"$missing")" -gt 0 ]]; then
      echo "  ⚠   PR #$number ($title) の head には次の required context が走っていません — rebase して再 push してください:" >&2
      jq -r '.[] | "      - \(.)"' <<<"$missing" >&2
    fi
  done < <(jq -r '.[] | [.number, .sha, .title] | @tsv' <<<"$prs")
  return 0
}

main() {
  [[ $# -ge 1 ]] || { usage >&2; return 2; }
  local owner_repo="$1"
  shift
  [[ "$owner_repo" == */* ]] || { echo "ERROR: '$owner_repo' is not owner/repo" >&2; return 2; }
  OWNER="${owner_repo%%/*}"

  # 各オプションはこの呼び出し限りの local — かつてこれらをモジュール
  # 直下のグローバルにしていたところ、ある main() 呼び出しで立てた
  # --dry-run/--reconcile 等が後続の呼び出しへ漏れ、selftest の後半ケース
  # が先の呼び出しのフラグを引き継いで誤判定する事故を自己テストで実測
  # した(#10 の DELETE が誤って DRY-RUN 扱いになった)。
  local REF="" FROM_DIR="" VERIFY_SHA=""
  local RECONCILE=false DRY_RUN=false UNVERIFIED_CONTEXTS=false DELETE_RULESET=""

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --ref) REF="$2"; shift 2 ;;
      --from-dir) FROM_DIR="$2"; shift 2 ;;
      --verify-sha) VERIFY_SHA="$2"; shift 2 ;;
      --reconcile) RECONCILE=true; shift ;;
      --dry-run) DRY_RUN=true; shift ;;
      --unverified-contexts) UNVERIFIED_CONTEXTS=true; shift ;;
      --delete-ruleset) DELETE_RULESET="$2"; shift 2 ;;
      --help | -h) usage; return 0 ;;
      *) echo "Unknown option: $1" >&2; return 2 ;;
    esac
  done

  for cmd in gh jq base64; do
    command -v "$cmd" >/dev/null 2>&1 || { echo "ERROR: '$cmd' not found" >&2; return 1; }
  done

  [[ -n "$REF" ]] || REF="$(resolve_ref "$owner_repo")"
  [[ -n "$REF" ]] || { echo "ERROR: could not resolve default branch for $owner_repo" >&2; return 1; }

  local decl_dir
  decl_dir="$(mktemp -d)"
  # `trap ... RETURN` set inside a function is a shell-global registration
  # that outlives this single call -- left alone it fires again (with a
  # stale/unbound $decl_dir under `set -u`) whenever ANY later function
  # returns, including a second invocation of main() itself. Clear it as
  # part of firing so it only ever runs once per call.
  trap 'rm -rf "$decl_dir"; trap - RETURN' RETURN
  load_declarations "$owner_repo" "$REF" "$FROM_DIR" "$decl_dir" || return $?
  check_no_placeholders "$decl_dir" || return $?

  if [[ -n "$DELETE_RULESET" ]]; then
    local existing id
    existing="$("$GH_BIN" api "repos/$owner_repo/rulesets" 2>/dev/null || echo '[]')"
    local f
    for f in "$decl_dir"/*.json; do
      [[ -e "$f" ]] || continue
      if [[ "$(jq -r '.name' "$f")" == "$DELETE_RULESET" ]]; then
        echo "ERROR: '$DELETE_RULESET' is still declared in .github/rulesets/ — remove it from the declaration first." >&2
        return 1
      fi
    done
    id="$(jq -r --arg n "$DELETE_RULESET" '.[] | select(.name == $n) | .id' <<<"$existing" | head -1)"
    if [[ -z "$id" ]]; then
      echo "  '$DELETE_RULESET' is not an active branch ruleset — nothing to delete."
      return 0
    fi
    if [[ "$DRY_RUN" == "true" ]]; then
      echo "  DRY-RUN: would DELETE '$DELETE_RULESET' (id=$id)"
    else
      RULESETS_WRITE_GUARD_BYPASS=1 "$GH_BIN" api -X DELETE "repos/$owner_repo/rulesets/$id" >/dev/null
      echo "  ✓  Deleted '$DELETE_RULESET' (id=$id)"
    fi
    return 0
  fi

  local sha
  if [[ -n "$VERIFY_SHA" ]]; then
    sha="$VERIFY_SHA"
  else
    sha="$(resolve_verify_sha "$owner_repo")"
  fi

  local contexts
  contexts="$(declared_contexts "$decl_dir/quality.json")"
  if [[ "$(jq 'length' <<<"$contexts")" -gt 0 ]]; then
    local verify_rc=0
    verify_contexts "$owner_repo" "$sha" "$contexts" || verify_rc=$?
    if [[ $verify_rc -ne 0 ]]; then
      if [[ "$UNVERIFIED_CONTEXTS" == "true" ]]; then
        echo "  WARNING: applying $(jq 'length' <<<"$contexts") unverified context(s) (--unverified-contexts)." >&2
      elif [[ $verify_rc -eq 1 ]]; then
        echo "ERROR: refusing to apply — required contexts are not reportable by the checked commit." >&2
        echo "  --unverified-contexts to override (only for a repository that has never had a PR yet)." >&2
        return 4
      fi
    fi
  fi

  echo "Applying declared rulesets to: $owner_repo (ref=$REF, reconcile=$RECONCILE)"
  echo ""

  local existing
  existing="$("$GH_BIN" api "repos/$owner_repo/rulesets" 2>/dev/null || echo '[]')"

  local declared_names=()
  local f
  for f in "$decl_dir"/*.json; do
    [[ -e "$f" ]] || continue
    apply_one_ruleset "$owner_repo" "$existing" "$f"
    declared_names+=("$(jq -r '.name' "$f")")
  done

  local names_json stray stray_count
  names_json="$(printf '%s\n' "${declared_names[@]}" | jq -R . | jq -s .)"
  stray="$(jq --argjson canon "$names_json" \
    '[.[] | select(.target == "branch" and (.name as $n | $canon | index($n) | not)) | {id, name}]' \
    <<<"$existing")"
  stray_count="$(jq 'length' <<<"$stray")"
  if [[ "$stray_count" -gt 0 ]]; then
    echo ""
    echo "NOTE: ${stray_count} active branch ruleset(s) not in the declaration:"
    jq -r '.[] | "  - \(.name) (id=\(.id))"' <<<"$stray"
    echo "  These are reported, not deleted."
  fi

  if [[ "$DRY_RUN" != "true" ]]; then
    warn_open_prs_missing_contexts "$owner_repo" "$contexts"
  fi
  return 0
}

# ---------------------------------------------------------------------------
# selftest — stubs gh (no network), path→fixture convention shared with
# scripts/rulesets-context-check.
# ---------------------------------------------------------------------------

selftest() {
  local fails=0 tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"; trap - RETURN' RETURN

  mkdir -p "$tmp/bin" "$tmp/fixtures" "$tmp/decl"

  cat >"$tmp/decl/security.json" <<'EOF'
{"name": "Security", "target": "branch", "enforcement": "active", "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}}, "rules": [{"type": "deletion"}]}
EOF
  cat >"$tmp/decl/workflow.json" <<'EOF'
{"name": "Workflow", "target": "branch", "enforcement": "active", "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}}, "rules": [{"type": "non_fast_forward"}]}
EOF
  cat >"$tmp/decl/quality.json" <<'EOF'
{"name": "Quality", "target": "branch", "enforcement": "active", "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}}, "rules": [{"type": "required_status_checks", "parameters": {"required_status_checks": [{"context": "test"}, {"context": "PR Title / PR title"}]}}]}
EOF

  # gh スタブ: path→fixture(pr-title-context-check の規約を移植)。
  # 引数の並びを構文木として解析し、$1..=api の後の非フラグ最初の値を
  # path とみなす。-X/-F/-H は値も含めて skip、--jq はクライアント適用。
  cat >"$tmp/bin/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
dir="$(dirname "$0")"
fixtures="$dir/../fixtures"
[[ "$1" == api ]] || { echo "unexpected gh invocation: $*" >&2; exit 1; }
shift
method="GET"
path=""
jqfilter=""
positional_seen=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    -X | --method) method="$2"; shift 2 ;;
    -F | -H) shift 2 ;;
    --jq) jqfilter="$2"; shift 2 ;;
    --input)
      shift
      body="$(cat)"
      ;;
    *)
      if [[ $positional_seen -eq 0 ]]; then
        path="$1"
        positional_seen=1
      fi
      shift
      ;;
  esac
done
if [[ "$method" != "GET" ]]; then
  name="$(printf '%s' "${body:-{}}" | jq -r '.name // empty' 2>/dev/null || true)"
  printf '%s %s %s\n' "$method" "$path" "$name" >>"$STUB_LOG"
  if [[ -f "$dir/write-fail" ]]; then
    exit 1
  fi
  if [[ "$method" == "POST" ]]; then
    printf '{"id": 999}'
  fi
  exit 0
fi
key="$(printf '%s' "$path" | tr '/' '_')"
if [[ -f "$dir/fail-$key" ]]; then
  exit 1
fi
[[ -f "$fixtures/$key.json" ]] || { echo "no fixture for: $key" >&2; exit 1; }
if [[ -n "$jqfilter" ]]; then
  # 実 `gh api --jq` は top-level scalar(文字列等)を jq -r 相当の raw
  # output で返す(go-gh の EvaluateFormatted / jsonScalarToString、
  # 2026-09-26 取得)。array/object はそのまま JSON エンコードされる —
  # `jq -rc` が同じ挙動(scalar は raw、それ以外は compact JSON)。
  jq -rc "$jqfilter" "$fixtures/$key.json"
else
  cat "$fixtures/$key.json"
fi
STUB
  chmod +x "$tmp/bin/gh"

  export PATH="$tmp/bin:$PATH"
  export STUB_LOG="$tmp/calls.log"
  export GITHUB_AUDIT_GH_BIN="$tmp/bin/gh"
  GH_BIN="$tmp/bin/gh"

  # 実測 job 名の実装は fetch_head_sha_job_names(actions/runs?head_sha= →
  # 各 run の jobs)。fixture: runs 一覧 + 各 run の jobs。
  write_run_fixtures() { # $1=owner/repo $2=sha $3=jobs_json_array
    local owner_repo="$1" sha="$2" jobs="$3"
    printf '{"workflow_runs":[{"id":1}]}' >"$tmp/fixtures/repos_${owner_repo//\//_}_actions_runs.json"
    printf '{"jobs":%s}' "$jobs" >"$tmp/fixtures/repos_${owner_repo//\//_}_actions_runs_1_jobs.json"
  }
  write_pulls_fixture() { # $1=owner/repo $2=sha(空なら PR 無し)
    local owner_repo="$1" sha="$2"
    if [[ -n "$sha" ]]; then
      printf '[{"head":{"sha":"%s"}}]' "$sha" >"$tmp/fixtures/repos_${owner_repo//\//_}_pulls.json"
    else
      printf '[]' >"$tmp/fixtures/repos_${owner_repo//\//_}_pulls.json"
    fi
  }

  check() { # $1=label $2=want_rc $3=got_rc
    if [[ "$2" == "$3" ]]; then
      echo "ok   $1"
    else
      echo "FAIL $1 (expected exit $2, got $3)" >&2
      fails=$((fails + 1))
    fi
  }
  expect_contains() {
    grep -qF -- "$2" "$STUB_LOG" 2>/dev/null || {
      echo "FAIL($1): ログに含まれない: $2" >&2
      cat "$STUB_LOG" 2>/dev/null >&2 || true
      fails=$((fails + 1))
    }
  }
  expect_not_contains() {
    grep -qF -- "$2" "$STUB_LOG" 2>/dev/null && {
      echo "FAIL($1): ログに含まれてはいけない: $2" >&2
      fails=$((fails + 1))
    }
    return 0
  }

  # 1) --from-dir・全 context 報告済(sha あり)→ 3 POST
  : >"$STUB_LOG"
  printf '[]' >"$tmp/fixtures/repos_tarotene_x_rulesets.json"
  write_run_fixtures "tarotene/x" "abc" '[{"name":"test"},{"name":"PR Title / PR title"}]'
  rc=0; main tarotene/x --ref main --from-dir "$tmp/decl" --verify-sha abc >/dev/null 2>"$tmp/err.log" || rc=$?
  check "1 rc=0" 0 "$rc"
  expect_contains "1 Security POST" "POST repos/tarotene/x/rulesets Security"
  expect_contains "1 Quality POST" "POST repos/tarotene/x/rulesets Quality"
  expect_contains "1 Workflow POST" "POST repos/tarotene/x/rulesets Workflow"

  # 1c) --from-dir 無し(remote fetch 経路): contents API 一覧 + 各ファイル
  #     取得で --from-dir と同じ結果になることを確認する(load_declarations
  #     の gh api --jq '.content' は top-level scalar を raw output で返す
  #     ため、選択的に -r 相当の解釈をスタブ側でも正しく再現できている
  #     ことを検証する回帰テスト)。
  printf '[{"name":"security.json"},{"name":"quality.json"},{"name":"workflow.json"}]' \
    >"$tmp/fixtures/repos_tarotene_x_contents_.github_rulesets.json"
  local rname
  for rname in security quality workflow; do
    printf '{"content":"%s"}' "$(base64 -w0 <"$tmp/decl/$rname.json")" \
      >"$tmp/fixtures/repos_tarotene_x_contents_.github_rulesets_${rname}.json.json"
  done
  : >"$STUB_LOG"
  rc=0; main tarotene/x --ref main --verify-sha abc >/dev/null 2>"$tmp/err1c.log" || rc=$?
  check "1c remote fetch rc=0" 0 "$rc"
  expect_contains "1c remote Quality POST" "POST repos/tarotene/x/rulesets Quality"

  # 2) 既存 Quality あり・--reconcile 無し → skip / 有り → PUT
  printf '[{"id": 1, "name": "Quality", "target": "branch"}]' >"$tmp/fixtures/repos_tarotene_x_rulesets.json"
  : >"$STUB_LOG"
  main tarotene/x --ref main --from-dir "$tmp/decl" --verify-sha abc >/dev/null
  expect_not_contains "2 reconcile 無しは PUT しない" "PUT repos/tarotene/x/rulesets/1"
  : >"$STUB_LOG"
  main tarotene/x --ref main --from-dir "$tmp/decl" --verify-sha abc --reconcile >/dev/null
  expect_contains "2 reconcile ありは PUT する" "PUT repos/tarotene/x/rulesets/1 Quality"
  printf '[]' >"$tmp/fixtures/repos_tarotene_x_rulesets.json"

  # 3) 報告不能 context → exit 4・書込みゼロ / --unverified-contexts → 続行
  write_run_fixtures "tarotene/x" "def" '[{"name":"unrelated"}]'
  : >"$STUB_LOG"
  rc=0; main tarotene/x --ref main --from-dir "$tmp/decl" --verify-sha def >/dev/null 2>"$tmp/err.log" || rc=$?
  check "3 報告不能で exit 4" 4 "$rc"
  [[ ! -s "$STUB_LOG" ]] && echo "ok   3 書込みゼロ" || { echo "FAIL 3 書込みが発生した" >&2; fails=$((fails + 1)); }
  : >"$STUB_LOG"
  rc=0; main tarotene/x --ref main --from-dir "$tmp/decl" --verify-sha def --unverified-contexts >/dev/null 2>"$tmp/err2.log" || rc=$?
  check "3b --unverified-contexts で続行" 0 "$rc"
  grep -qF "WARNING" "$tmp/err2.log" && echo "ok   3b WARNING を出す" || { echo "FAIL 3b WARNING が無い" >&2; fails=$((fails + 1)); }

  # 4) 宣言なし → exit 3・出力に seed
  : >"$STUB_LOG"
  rc=0; main tarotene/x --ref main --from-dir "$tmp/empty-decl" --verify-sha abc >"$tmp/out4.log" 2>&1 || rc=$?
  check "4 宣言なしで exit 3" 3 "$rc"
  grep -qi "seed" "$tmp/out4.log" && echo "ok   4 seed を案内する" || { echo "FAIL 4 seed 案内が無い: $(cat "$tmp/out4.log")" >&2; fails=$((fails + 1)); }

  # 5) --dry-run → -X 呼出ゼロ(test 3 が abc の job fixture を上書きして
  #    いるので、ここで再び「全 context 報告済」に戻す)
  write_run_fixtures "tarotene/x" "abc" '[{"name":"test"},{"name":"PR Title / PR title"}]'
  : >"$STUB_LOG"
  main tarotene/x --ref main --from-dir "$tmp/decl" --verify-sha abc --dry-run >/dev/null
  [[ ! -s "$STUB_LOG" ]] && echo "ok   5 --dry-run は書込みゼロ" || { echo "FAIL 5: $(cat "$STUB_LOG")" >&2; fails=$((fails + 1)); }

  # 6) placeholder 残存 → exit 5
  mkdir -p "$tmp/decl-bad"
  cp "$tmp/decl"/*.json "$tmp/decl-bad/"
  sed -i 's/"test"/"__CLI_CRATE__ CLI"/' "$tmp/decl-bad/quality.json"
  : >"$STUB_LOG"
  rc=0; main tarotene/x --ref main --from-dir "$tmp/decl-bad" --verify-sha abc >/dev/null 2>&1 || rc=$?
  check "6 placeholder 残存で exit 5" 5 "$rc"

  # 7) --verify-sha 指定 → pulls fixture を呼ばない(fixture を消しておき、
  #    呼ばれたら「no fixture」で exit 1 になることで検出する)
  rm -f "$tmp/fixtures/repos_tarotene_x_pulls.json"
  : >"$STUB_LOG"
  rc=0; main tarotene/x --ref main --from-dir "$tmp/decl" --verify-sha abc >/dev/null 2>&1 || rc=$?
  check "7 --verify-sha は pulls を呼ばない" 0 "$rc"

  # 8) --verify-sha 省略 → resolve_verify_sha が pulls から取得
  write_pulls_fixture "tarotene/x" "abc"
  : >"$STUB_LOG"
  rc=0; main tarotene/x --ref main --from-dir "$tmp/decl" >/dev/null 2>&1 || rc=$?
  check "8 pulls から verify sha を解決" 0 "$rc"

  # 9) 未宣言の "Ephemeral Initial" は報告のみ、DELETE は呼ばれない
  printf '[{"id": 1, "name": "Ephemeral Initial", "target": "branch"}]' >"$tmp/fixtures/repos_tarotene_x_rulesets.json"
  : >"$STUB_LOG"
  out="$(main tarotene/x --ref main --from-dir "$tmp/decl" --verify-sha abc --reconcile 2>&1)"
  expect_not_contains "9 DELETE は呼ばれない" "DELETE"
  echo "$out" | grep -qF "Ephemeral Initial" && echo "ok   9 未宣言 ruleset は報告される" || { echo "FAIL 9: $out" >&2; fails=$((fails + 1)); }
  printf '[]' >"$tmp/fixtures/repos_tarotene_x_rulesets.json"

  # 10) --delete-ruleset: 宣言に無ければ DELETE、あれば拒否
  printf '[{"id": 5, "name": "Review", "target": "branch"}]' >"$tmp/fixtures/repos_tarotene_x_rulesets.json"
  : >"$STUB_LOG"
  main tarotene/x --ref main --from-dir "$tmp/decl" --delete-ruleset Review >/dev/null
  expect_contains "10 Review DELETE" "DELETE repos/tarotene/x/rulesets/5"
  mkdir -p "$tmp/decl-review"
  cp "$tmp/decl"/*.json "$tmp/decl-review/"
  cp "$tmp/decl/workflow.json" "$tmp/decl-review/review.json"
  jq '.name = "Review"' "$tmp/decl-review/review.json" >"$tmp/decl-review/review.json.tmp" && mv "$tmp/decl-review/review.json.tmp" "$tmp/decl-review/review.json"
  rc=0; main tarotene/x --ref main --from-dir "$tmp/decl-review" --delete-ruleset Review >/dev/null 2>&1 || rc=$?
  check "10b 宣言に有るときは拒否" 1 "$rc"
  printf '[]' >"$tmp/fixtures/repos_tarotene_x_rulesets.json"

  # 11) open PR 走査の配線確認: 全 PR の head が required context を満たす
  #     ときは警告が出ない(stub は head_sha を path 鍵にできないため——実
  #     API では head_sha でフィルタされる——「declared − 実測」の差分計算
  #     自体は test 3 で検証済みなので、ここでは配線のみ確認する)。
  write_run_fixtures "tarotene/x" "abc" '[{"name":"test"},{"name":"PR Title / PR title"}]'
  printf '[{"number":7,"title":"open pr","head":{"sha":"abc"}}]' >"$tmp/fixtures/repos_tarotene_x_pulls.json"
  rc=0; out="$(main tarotene/x --ref main --from-dir "$tmp/decl" --verify-sha abc 2>&1)" || rc=$?
  check "11 rc=0" 0 "$rc"
  echo "$out" | grep -qF "PR #7" && { echo "FAIL 11: 満たしている PR に警告が出た: $out" >&2; fails=$((fails + 1)); } || echo "ok   11 満たしている PR には警告なし"

  if ((fails > 0)); then
    echo "selftest: ${fails} 件失敗" >&2
    return 1
  fi
  echo "selftest: OK"
}

case "${1-}" in
  --selftest) selftest ;;
  --help | -h) usage ;;
  *) main "$@" ;;
esac
