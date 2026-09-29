#!/usr/bin/env bash
# repo-create-guard.sh — `gh repo create` / `gh api -X POST user/repos`・
# `gh api -X POST orgs/*/repos` を作成時点で deny し、`repo-charter` スキル
# の手順(命名インタビュー → README/CONTRIBUTING → GitHub メタデータ反映 →
# governance 播種)に強制的に載せる PreToolUse(Bash) hook。
# `docs/adr/0013-repo-charter-schema.md` の Amendment(2026-09-29)参照。
#
# 動機: `gh repo create` 自体には作成直後に走るフック機構が無く、作成時の
# 強制点は `repo-charter` スキルという散文的手順のみだった(ADR-0013 D2)。
# ある private リポジトリの新規作成セッションで、`core` 型の charter 手順が
# `apply-repo-settings.sh` の呼び出しを欠いたまま実行され、実際に settings
# drift(squash-only 化・delete-branch-on-merge 等が未適用)が発生した
# (2026-09-28 実例、実名は書かない — ADR-0034)。この抜けは散文的手順を
# 読み飛ばせば常に起こりうるため、作成コマンドそのものを deny して手順に
# 強制的に載せる。
#
# コマンド解析エンジン(split_heredoc / tokenize / is_sep / TOK / CMD_SEPS /
# emit_deny)は attribution-guard.sh を source して再利用する(adr-number.sh
# / pr-title-guard.sh / stack-base-guard.sh / feedback-target-guard.sh /
# decision-colocation-guard.sh と同じ「1つの判定エンジンを source する」型
# — 既存手段: config/claude/hooks/attribution-guard.sh — 拡張: heredoc 分離・
# 引用符解釈込みのトークナイザ・コマンド位置判定の骨格を自前で再実装しない)。
# `is_target_at` / `decide_tokens` / `decide_api_tokens` / `main` /
# `selftest` をこのファイル専用に上書きする。
#
# 軸: 検出のみ — `gh` 経由の生成は deny できるが、`curl` で直接
# `api.github.com/user/repos` を叩く経路や Web UI からの作成はこの hook の
# 対象外(ADR-543 D1 と同型の限界、フルスクラッチ自体は表現不可能にできない)。
#
# 使い方:
#   hook として: settings.json の PreToolUse(matcher: "Bash|mcp__.*")から
#                stdin JSON で呼ばれる(mcp__github* は現在未接続のため
#                Bash のみ判定する — attribution-guard.sh の decide_mcp と
#                同じ理由、MCP 接続時は別途対応する)
#   自己検査:   repo-create-guard.sh --selftest(ネットワーク不使用)
#
# バイパス: REPO_CREATE_GUARD_BYPASS=1(rulesets-write-guard.rs と同型)。
set -uo pipefail

GUARD_SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./attribution-guard.sh
source "$GUARD_SELF_DIR/attribution-guard.sh"

# ---------------------------------------------------------------------------
# コマンド位置判定(attribution-guard.sh の同名関数を上書きする)。
# `gh repo create ...` と `gh api ...`(パス/メソッドの絞り込みは
# decide_api_tokens 側)だけを対象にする。
# ---------------------------------------------------------------------------
is_target_at() {
  local i=$1 n=${#TOK[@]} base
  ((i + 1 < n)) || return 1
  base="${TOK[i]##*/}"
  [[ $base == gh ]] || return 1
  if [[ ${TOK[i + 1]} == api ]]; then
    TARGET_KIND=api
    return 0
  fi
  ((i + 2 < n)) || return 1
  if [[ ${TOK[i + 1]} == repo && ${TOK[i + 2]} == create ]]; then
    TARGET_KIND=cli
    return 0
  fi
  return 1
}

DENY_HINT="repo-charter スキルの手順(config/claude/skills/repo-charter/SKILL.md)
を経由してください:

  1. §1 の命名インタビュー(閉語彙チェック — naming-codename/naming-descriptive
     等のクラス確定)を先に済ませる。
  2. §8 の手順どおり \`gh repo create\` → \`gh repo edit --add-topic <クラス>\`
     → 型別の governance 播種(rust/typst/astro は各 seed.sh、それ以外は
     \`repo-governance-common/scripts/apply-repo-settings.sh\` +
     \`copy-files.sh\` + \`apply-rulesets.sh\`)まで一式で行う。

判定不能な事情があれば REPO_CREATE_GUARD_BYPASS=1 で一時的に迂回できます
(ADR-0013 Amendment 2026-09-29 参照)。"

deny_reason_repo_create() {
  printf '%s' "素の \`gh repo create\` は使わないでください。

${DENY_HINT}"
}

deny_reason_repo_create_api() {
  printf '%s' "\`gh api\` での repo 作成(${1})は使わないでください。

${DENY_HINT}"
}

# ---------------------------------------------------------------------------
# decide_tokens を上書き(attribution-guard.sh の同名関数を差し替える)。
# is_target_at で既に `gh repo create` に絞られている範囲なので、body の
# 有無に関わらず常に deny する。
# ---------------------------------------------------------------------------
decide_tokens() {
  deny_reason_repo_create
  return 0
}

# gh api での repo 作成エンドポイント: 認証ユーザー自身の `user/repos` と
# 組織配下の `orgs/{org}/repos`。POST 以外(GET での一覧取得、PATCH での
# settings 変更 — apply-repo-settings.sh 自身がこれを使う)は対象外。
REPO_CREATE_API_RE='^/(user/repos|orgs/[^/]+/repos)$'

# ---------------------------------------------------------------------------
# decide_api_tokens を上書き(attribution-guard.sh の同名関数を差し替える)。
# -X/--method が明示的に POST で、かつパスが repo 作成エンドポイントに
# 一致するときだけ deny する(GET/PATCH/DELETE 等はここで通す)。
# ---------------------------------------------------------------------------
decide_api_tokens() {
  local -a tok=("$@")
  local n=${#tok[@]} i method='' path='' t

  for ((i = 0; i < n; i++)); do
    case "${tok[i]}" in
      -X | --method)
        ((i + 1 < n)) && method="${tok[i + 1]}"
        ;;
      --method=*)
        method="${tok[i]#--method=}"
        ;;
    esac
  done || true
  [[ ${method^^} == POST ]] || return 1 # 明示的な POST 指定が無ければ対象外

  for ((i = 0; i < n; i++)); do
    t="/${tok[i]#/}"
    if [[ $t =~ $REPO_CREATE_API_RE ]]; then
      path="$t"
      break
    fi
  done || true
  [[ -n $path ]] || return 1 # 対象エンドポイントでない → 通す

  deny_reason_repo_create_api "$path"
  return 0
}

# ---------------------------------------------------------------------------
# hook 入出力(feedback-target-guard.sh と同じ契約、Bash のみを対象にする
# — mcp__github* は現在未接続)。
# ---------------------------------------------------------------------------
main() {
  [[ -z ${REPO_CREATE_GUARD_BYPASS:-} ]] || exit 0
  command -v jq > /dev/null 2>&1 || exit 0

  local input tool cmd reason
  input="$(cat)" || exit 0

  tool="$(jq -r '.tool_name // empty' <<< "$input" 2> /dev/null)" || exit 0
  [[ $tool == Bash ]] || exit 0

  cmd="$(jq -r '.tool_input.command // empty' <<< "$input" 2> /dev/null)" || exit 0
  [[ -n $cmd ]] || exit 0

  reason="$(decide "$cmd")" && {
    emit_deny "$reason"
    exit 0
  }
  exit 0
}

# ---------------------------------------------------------------------------
# selftest(attribution-guard.sh の同名関数を上書きする)。
# ---------------------------------------------------------------------------
selftest() {
  local fails=0

  expect_pass() { # $1=名前 $2=cmd
    local out
    if out="$(decide "$2")"; then
      echo "FAIL $1 (expected pass, denied: $out)" >&2
      fails=$((fails + 1))
    else
      echo "ok   $1"
    fi
  }
  expect_deny() { # $1=名前 $2=cmd $3=期待する部分文字列(省略可)
    local out
    if out="$(decide "$2")"; then
      if [[ -n ${3-} ]] && ! grep -qF -- "$3" <<< "$out"; then
        echo "FAIL $1 (denied but missing substring [$3]: $out)" >&2
        fails=$((fails + 1))
      else
        echo "ok   $1"
      fi
    else
      echo "FAIL $1 (expected deny, but passed)" >&2
      fails=$((fails + 1))
    fi
  }

  expect_deny "素の gh repo create" \
    "gh repo create acme/foo --private" \
    "gh repo create"

  expect_pass "gh repo edit は対象外" \
    "gh repo edit acme/foo --description x"

  expect_pass "gh repo view は対象外" \
    "gh repo view acme/foo"

  expect_deny "gh api -X POST user/repos" \
    "gh api -X POST user/repos -f name=foo" \
    "repo 作成"

  expect_deny "gh api --method POST orgs/*/repos" \
    "gh api --method POST orgs/acme/repos -f name=foo" \
    "repo 作成"

  expect_pass "gh api GET user/repos は一覧取得なので対象外" \
    "gh api -X GET user/repos"

  expect_pass "gh api PATCH repos/o/r は settings 変更なので対象外(apply-repo-settings.sh 自身の呼び出し形)" \
    "gh api -X PATCH repos/acme/foo --input -"

  expect_pass "gh api rulesets への POST は repo 作成と無関係" \
    "gh api -X POST repos/acme/foo/rulesets --input -"

  expect_pass "コマンド位置外の綴りは発火しない" \
    "echo 'gh repo create acme/foo'"

  # REPO_CREATE_GUARD_BYPASS は main() の入口だけで見る(stack-base-guard.sh
  # の SKIP_STACK_BASE_GUARD と同じ配置 — decide() 自体は判定ロジックだけを
  # 持ち、バイパスは hook 入出力層の責務にする)。decide() 単体の selftest
  # では検査しない。

  if ((fails > 0)); then
    echo "selftest: ${fails} 件失敗" >&2
    exit 1
  fi
  echo "selftest: OK"
}

case "${1-}" in
  --selftest) selftest ;;
  *) main ;;
esac
