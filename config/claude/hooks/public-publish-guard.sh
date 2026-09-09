#!/usr/bin/env bash
# public-publish-guard.sh — 会社/private リポジトリの実名が PUBLIC な面
# (git push・gh pr/issue の create/edit/comment)に漏れるのを防ぐ PreToolUse hook。
#
# 設計と根拠: docs/claude/public-publish-guard.md(このリポジトリ内)
#
# 経緯: PUBLIC な tarotene/dotfiles に、会社(org)の private リポ名と
# tarotene 所有の private リポ名が PR 本文・Issue 本文・コミット済み
# ファイルの複数箇所に混入した(2026-09-10)。denylist との突き合わせを
# git-stash-guard.sh と同じ deny-hook パターンで実装する。
#
# 判定は 2 段階:
#   hard-deny(permissionDecision: deny)  — "org/repo" 形式(URL も含む)、
#     または denylist に載った具体的なリポ名(裸の単語一致)
#   warn(permissionDecision: ask)        — denylist の org 名が裸の単語で
#     単体出現(メールドメイン等の正当な用途と衝突しうるため deny にしない)
#
# denylist は一切コミットしない(scripts/github-audit-rulesets の
# overrides.tsv と同じ「ツールは公開・データはローカル」原則):
#   $XDG_CONFIG_HOME/public-publish-guard/orgs.txt   — 1行1 org 名
#   $XDG_CONFIG_HOME/public-publish-guard/repos.txt  — 1行1 "org/repo"
#   自分の private リポ名は `gh repo list --visibility private` から live 取得し
#   $XDG_STATE_HOME/public-publish-guard/private-repos-cache.json に TTL 付きで
#   キャッシュする(--refresh-cache で強制更新)。
#
# git-worktree-allow.sh との非対称性(git-stash-guard.sh と同じ理由):
#   deny/ask 側では if 不一致 = 無検査で素通り、が事故そのものになるため、
#   if は "Bash(*)" まで広げ、絞り込みは hook 内部の早期 exit に置く。
#
# 縮退(ADR-0005 の binary-existence gating に倣う): jq 不在・stdin 不正は黙って exit 0。
#
# 既知の限界(docs/claude/public-publish-guard.md に詳細):
#   - `gh api` の生呼び出し(-f body=...)は対象外。
#   - private リポの一覧はキャッシュ TTL の間は更新されない — 直前に作った
#     private リポは穴になりうる(--refresh-cache で手動更新可能)。
#   - 新規ブランチの初回 push で origin/<default> をローカルに fetch して
#     いない場合、merge-base が取れずスキャン自体が黙って空振りする。
#
# 使い方:
#   hook として: settings.json の PreToolUse(matcher: Bash, if: "Bash(*)")
#                から stdin JSON で呼ばれる
#   監査:        public-publish-guard.sh --audit [path...]
#                public-publish-guard.sh --audit --remote (open Issue/PR も検査)
#   キャッシュ更新: public-publish-guard.sh --refresh-cache
#   自己検査:    public-publish-guard.sh --selftest
set -euo pipefail

SELF="$(realpath "$0")"
TEST_TMP=''

# init_vars: env から派生変数を(再)計算する。selftest が env を export した
# 「後」に呼び直せるよう関数化してある — トップレベルで一度だけ計算すると、
# selftest 内で export した値が既に評価済みの変数に反映されない。
init_vars() {
  CONFIG_DIR="${PUBLIC_PUBLISH_GUARD_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/public-publish-guard}"
  STATE_DIR="${PUBLIC_PUBLISH_GUARD_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/public-publish-guard}"
  ORGS_FILE="${PUBLIC_PUBLISH_GUARD_ORGS_FILE:-$CONFIG_DIR/orgs.txt}"
  REPOS_FILE="${PUBLIC_PUBLISH_GUARD_REPOS_FILE:-$CONFIG_DIR/repos.txt}"
  OWNER="${PUBLIC_PUBLISH_GUARD_OWNER:-tarotene}"
  GH_BIN="${PUBLIC_PUBLISH_GUARD_GH_BIN:-gh}"
  CACHE_TTL="${PUBLIC_PUBLISH_GUARD_CACHE_TTL:-86400}"
  PRIVATE_CACHE="$STATE_DIR/private-repos-cache.json"
  VIS_CACHE="$STATE_DIR/visibility-cache.tsv"
}
init_vars

usage() {
  cat <<'EOF'
usage: public-publish-guard.sh [--audit [path...] [--remote]|--refresh-cache|--selftest]

PreToolUse hook: git push / gh pr|issue create|edit|comment を、会社 org や
自分の private リポの実名が漏れていないか検査する。判定は2段階 —
"org/repo" 形式・denylist 上の具体リポ名(裸)は deny、denylist 上の org 名の
裸単体出現は ask。PUBLIC_PUBLISH_GUARD_ALLOW=1 で意識的に bypass できる。

--audit [path...]   指定ファイル(省略時は git ls-files 全体)を同じ denylist
                    で検査する。--remote を付けると open な Issue/PR の
                    title/body も対象に含める。
--refresh-cache     自分の private リポ一覧キャッシュを強制更新する。
--selftest          回帰テスト(スタブ gh/git)。
EOF
}

read_lines() { # $1=file; コメント行・空行を除いて出力。ファイル無しは無出力。
  [[ -f "$1" ]] || return 0
  grep -Ev '^[[:space:]]*(#|$)' "$1"
}

# refresh_private_repo_cache: 自分の private リポ名一覧を live 取得して
# キャッシュに書く。取得に失敗したら何もしない(呼び出し側が古いキャッシュに
# フォールバックする)。
#
# `.github` は GitHub の特殊リポジトリ(org-wide デフォルト用)で、その裸の
# 名前は `.github/workflows/`・`docs.github.com` 等どこにでも出現する
# パス片・URL 断片と衝突し、denylist に混ぜると際限なく誤検知する
# (実測: CODEOWNERS・nix.yml・docs 内の GitHub 公式ドキュメント URL 等)。
# 「その名前が漏れても実害がない」特殊リポジトリなので、常に除外する。
EXCLUDE_REPO_NAMES=(.github)
refresh_private_repo_cache() {
  mkdir -p "$STATE_DIR"
  local tmp names exclude_jq
  tmp="$(mktemp "$STATE_DIR/private-repos-cache.json.XXXXXX")"
  exclude_jq="$(printf '%s\n' "${EXCLUDE_REPO_NAMES[@]}" | jq -R . | jq -s .)"
  if names="$("$GH_BIN" repo list "$OWNER" --visibility private --limit 500 --json name \
    --jq "[.[].name] - ${exclude_jq}" 2>/dev/null)" \
    && [[ -n $names ]]; then
    printf '%s' "$names" > "$tmp"
    mv "$tmp" "$PRIVATE_CACHE"
  else
    rm -f "$tmp"
    return 1
  fi
}

# private_repo_names: キャッシュが TTL 内ならそれを、無ければ live 取得して
# 出力する。live 取得に失敗し、古いキャッシュも無ければ何も出力しない
# (denylist が空になるだけで、検査自体は他の denylist で継続する)。
private_repo_names() {
  local now mtime
  now="$(date +%s)"
  if [[ -f $PRIVATE_CACHE ]]; then
    mtime="$(stat -c %Y "$PRIVATE_CACHE" 2>/dev/null || stat -f %m "$PRIVATE_CACHE" 2>/dev/null || printf 0)"
    if (( now - mtime < CACHE_TTL )); then
      jq -r '.[]?' "$PRIVATE_CACHE" 2>/dev/null
      return 0
    fi
  fi
  refresh_private_repo_cache || true
  [[ -f $PRIVATE_CACHE ]] && jq -r '.[]?' "$PRIVATE_CACHE" 2>/dev/null
  return 0
}

# build_patterns: denylist 3種を集めて3つのパターン配列をグローバル変数に積む。
#   PLAIN_PATTERNS  — スラッシュを含む固定文字列。-w は使わない(末尾が "/" の
#                     パターンは -w だと理論上一致し得ない — \> は直前の文字が
#                     word 文字であることを要求するため、"/" 終端では常に不成立)。
#   WORD_HARD       — 裸のリポ名(単語境界一致、hard-deny)。
#   WORD_WARN       — 裸の org 名(単語境界一致、ask)。
PLAIN_PATTERNS=()
WORD_HARD=()
WORD_WARN=()
build_patterns() {
  PLAIN_PATTERNS=() WORD_HARD=() WORD_WARN=()
  local org nwo repo
  while IFS= read -r org; do
    [[ -n $org ]] || continue
    PLAIN_PATTERNS+=("${org}/")
    WORD_WARN+=("$org")
  done < <(read_lines "$ORGS_FILE")

  while IFS= read -r nwo; do
    [[ -n $nwo ]] || continue
    PLAIN_PATTERNS+=("$nwo")
    repo="${nwo#*/}"
    [[ -n $repo && $repo != "$nwo" ]] && WORD_HARD+=("$repo")
  done < <(read_lines "$REPOS_FILE")

  while IFS= read -r repo; do
    [[ -n $repo ]] || continue
    WORD_HARD+=("$repo")
  done < <(private_repo_names)
}

# match_verdict: $1 の中身を denylist と突き合わせ、"deny"/"ask" を印字して
# 0 を返す。どちらにも一致しなければ何も印字せず非 0 を返す。
match_verdict() {
  local text="$1"
  if ((${#PLAIN_PATTERNS[@]} > 0)) && grep -Fqf <(printf '%s\n' "${PLAIN_PATTERNS[@]}") <<< "$text"; then
    printf 'deny'
    return 0
  fi
  if ((${#WORD_HARD[@]} > 0)) && grep -Fwqf <(printf '%s\n' "${WORD_HARD[@]}") <<< "$text"; then
    printf 'deny'
    return 0
  fi
  if ((${#WORD_WARN[@]} > 0)) && grep -Fwqf <(printf '%s\n' "${WORD_WARN[@]}") <<< "$text"; then
    printf 'ask'
    return 0
  fi
  return 1
}

# resolve_default_branch: scripts/git-audit-worktrees の同名関数と同じ idiom。
# cwd のリポジトリに対して行う(こちらは複数リポを横断しないので --git-dir 不要)。
resolve_default_branch() {
  local ref b
  ref="$(git symbolic-ref -q --short refs/remotes/origin/HEAD 2>/dev/null)" || ref=""
  if [[ -n $ref ]]; then
    printf '%s' "${ref#origin/}"
    return 0
  fi
  for b in main master; do
    git show-ref --verify --quiet "refs/heads/$b" && { printf '%s' "$b"; return 0; }
  done
  printf ''
}

# scan_push_text: これから push する範囲のファイル差分 + コミットメッセージを
# 連結して出力する。merge-base が取れない(origin/<default> 未 fetch 等)場合は
# 何も出さない — 既知の限界としてドキュメントに明記(検査は黙って空振りする)。
scan_push_text() {
  local default_branch base
  default_branch="$(resolve_default_branch)"
  [[ -n $default_branch ]] || return 0
  base="$(git merge-base HEAD "origin/$default_branch" 2>/dev/null)" || return 0
  [[ -n $base ]] || return 0
  git diff "$base"..HEAD 2>/dev/null
  git log --format=%B "$base"..HEAD 2>/dev/null
}

# resolve_repo_nwo: cmd 中の --repo/-R 明示指定を優先し、無ければ cwd の
# origin リモートから owner/repo を導出する。どちらも取れなければ非 0。
resolve_repo_nwo() {
  local cmd="$1"
  if [[ $cmd =~ (--repo|-R)[=\ ]+([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+) ]]; then
    printf '%s' "${BASH_REMATCH[2]}"
    return 0
  fi
  local url
  url="$(git remote get-url origin 2>/dev/null)" || return 1
  url="${url%.git}"
  url="${url#git@github.com:}"
  url="${url#https://github.com/}"
  url="${url#ssh://git@github.com/}"
  [[ -n $url && $url == */* ]] || return 1
  printf '%s' "$url"
}

# repo_visibility: $1=owner/repo -> PUBLIC/PRIVATE/INTERNAL/UNKNOWN。
# TTL 付きキャッシュ(TSV: nwo, visibility, checked-at epoch)。取得に失敗したら
# UNKNOWN を返す — 呼び出し側は UNKNOWN を PUBLIC と同じ扱い(検査続行)にする。
repo_visibility() {
  local nwo="$1" now line r v t
  now="$(date +%s)"
  if [[ -f $VIS_CACHE ]]; then
    while IFS=$'\t' read -r r v t; do
      [[ $r == "$nwo" ]] || continue
      if (( now - t < CACHE_TTL )); then
        printf '%s' "$v"
        return 0
      fi
      break
    done < "$VIS_CACHE"
  fi
  local vis
  vis="$("$GH_BIN" repo view "$nwo" --json visibility --jq .visibility 2>/dev/null)" || vis=""
  [[ -n $vis ]] || { printf 'UNKNOWN'; return 0; }
  mkdir -p "$STATE_DIR"
  local tmp
  tmp="$(mktemp "$STATE_DIR/visibility-cache.tsv.XXXXXX")"
  { [[ -f $VIS_CACHE ]] && awk -F'\t' -v r="$nwo" '$1 != r' "$VIS_CACHE"
    printf '%s\t%s\t%s\n' "$nwo" "$vis" "$now"; } > "$tmp"
  mv "$tmp" "$VIS_CACHE"
  printf '%s' "$vis"
}

# gather_scan_text: cmd から検査対象の文字列を切り出す。該当パターンが無ければ
# 非 0(呼び出し元は即フォールスルー、jq/gh/git を一切叩かない高速経路)。
gather_scan_text() {
  local cmd="$1"
  if [[ $cmd =~ (^|[^[:alnum:]_])gh[[:space:]]+(pr|issue)[[:space:]]+(create|edit|comment)([^[:alnum:]_]|$) ]]; then
    printf '%s' "$cmd"
    return 0
  fi
  if [[ $cmd =~ (^|[^[:alnum:]_])git[[:space:]]+push([^[:alnum:]_]|$) ]]; then
    scan_push_text
    return 0
  fi
  return 1
}

# decide: $1=cmd 全体。"deny"/"ask" のどちらかと理由を1行で印字して 0 を返す。
# 非該当は非 0(フォールスルー)。
decide() {
  local cmd="$1" text nwo vis verdict
  text="$(gather_scan_text "$cmd")" || return 1
  [[ -n $text ]] || return 1

  nwo="$(resolve_repo_nwo "$cmd")" || nwo=""
  if [[ -n $nwo ]]; then
    vis="$(repo_visibility "$nwo")"
    [[ $vis == PRIVATE || $vis == INTERNAL ]] && return 1
  fi
  # nwo が取れない、または UNKNOWN/PUBLIC の場合は安全側(PUBLIC とみなして続行)。

  build_patterns
  verdict="$(match_verdict "$text")" || return 1
  printf '%s' "$verdict"
}

main() {
  command -v jq > /dev/null 2>&1 || exit 0
  [[ "${PUBLIC_PUBLISH_GUARD_ALLOW:-}" == "1" ]] && exit 0

  local input cmd
  input="$(cat)" || exit 0
  # 最速フィルタ: push/gh の気配が無ければ jq すら呼ばない。
  grep -qE '(^|[^[:alnum:]_])(push|gh)([^[:alnum:]_]|$)' <<< "$input" || exit 0

  cmd="$(jq -r 'select(.tool_name == "Bash") | .tool_input.command // empty' \
    <<< "$input" 2> /dev/null)" || exit 0
  [[ -n $cmd ]] || exit 0

  local verdict decision reason
  verdict="$(decide "$cmd")" || exit 0
  case "$verdict" in
    deny) decision=deny; reason="denylist に登録された company/private リポジトリの参照と一致しました(PUBLIC_PUBLISH_GUARD_ALLOW=1 で意識的に bypass できます)。" ;;
    ask) decision=ask; reason="denylist に登録された org 名の裸の単体出現と一致しました。正当な用途(例: メールドメイン)か確認してください。" ;;
    *) exit 0 ;;
  esac

  jq -n --arg decision "$decision" --arg reason "$reason" '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: $decision,
      permissionDecisionReason: $reason
    }
  }'
  exit 0
}

audit() {
  local remote=false
  local -a paths=()
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --remote) remote=true ;;
      *) paths+=("$1") ;;
    esac
    shift
  done

  build_patterns
  local hit=0 verdict content

  if ((${#paths[@]} == 0)); then
    mapfile -t paths < <(git ls-files 2>/dev/null)
  fi
  local f
  for f in "${paths[@]}"; do
    [[ -f $f ]] || continue
    content="$(cat "$f" 2>/dev/null)"
    if verdict="$(match_verdict "$content")"; then
      printf '%s: file=%s\n' "$verdict" "$f"
      hit=1
    fi
  done

  if [[ $remote == true ]] && command -v gh > /dev/null 2>&1; then
    local n title body
    while IFS=$'\t' read -r n title body; do
      [[ -n $n ]] || continue
      if verdict="$(match_verdict "$title"$'\n'"$body")"; then
        printf '%s: issue=#%s\n' "$verdict" "$n"
        hit=1
      fi
    done < <(gh issue list --state open --limit 200 --json number,title,body \
      --jq '.[] | [.number, .title, (.body // "" | gsub("\n";" "))] | @tsv' 2> /dev/null)
    while IFS=$'\t' read -r n title body; do
      [[ -n $n ]] || continue
      if verdict="$(match_verdict "$title"$'\n'"$body")"; then
        printf '%s: pr=#%s\n' "$verdict" "$n"
        hit=1
      fi
    done < <(gh pr list --state open --limit 200 --json number,title,body \
      --jq '.[] | [.number, .title, (.body // "" | gsub("\n";" "))] | @tsv' 2> /dev/null)
  fi

  if [[ $hit -eq 0 ]]; then
    printf '残存ヒットはありません。\n'
  fi
  return $hit
}

selftest() {
  local tmp
  tmp="$(mktemp -d)"
  TEST_TMP="$tmp"
  trap 'rm -rf "$TEST_TMP"' EXIT

  export PUBLIC_PUBLISH_GUARD_CONFIG_DIR="$tmp/config"
  export PUBLIC_PUBLISH_GUARD_STATE_DIR="$tmp/state"
  export PUBLIC_PUBLISH_GUARD_ORGS_FILE="$tmp/config/orgs.txt"
  export PUBLIC_PUBLISH_GUARD_REPOS_FILE="$tmp/config/repos.txt"
  init_vars # export しただけでは既に評価済みの変数に反映されないため明示的に再計算する
  mkdir -p "$tmp/config" "$tmp/state"
  printf 'acme\n' > "$PUBLIC_PUBLISH_GUARD_ORGS_FILE"
  printf 'acme/secret-project\n' > "$PUBLIC_PUBLISH_GUARD_REPOS_FILE"

  # gh をスタブし、private リポ一覧の live 取得を固定内容にする。
  # 実際の呼び出しは常に --jq を付ける(--json name --jq '[.[].name]' /
  # --json visibility --jq .visibility)ので、スタブも jq 適用後の値を返す。
  cat > "$tmp/bin-gh" <<'STUB'
#!/usr/bin/env bash
case "$1 $2" in
  "repo list") printf '["my-private-tool"]' ;;
  "repo view") printf 'PUBLIC' ;;
  *) exit 1 ;;
esac
STUB
  chmod +x "$tmp/bin-gh"
  export PUBLIC_PUBLISH_GUARD_GH_BIN="$tmp/bin-gh"
  init_vars

  local fails=0
  expect_verdict() { # $1=expected(deny/ask/pass) $2=text
    local got
    build_patterns
    if got="$(match_verdict "$2")"; then :; else got="pass"; fi
    if [[ $got != "$1" ]]; then
      echo "FAIL(期待=$1, 実際=$got): $2" >&2
      fails=$((fails + 1))
    fi
  }

  # hard-deny: org/repo 形式(URL に埋め込まれていても)
  expect_verdict deny "参照: acme/secret-project の話"
  expect_verdict deny "https://github.com/acme/secret-project/issues/1"
  # hard-deny: repos.txt の裸のリポ名単体
  expect_verdict deny "secret-project の CI が壊れている"
  # hard-deny: 自分の private リポ名(live 取得したキャッシュ)
  expect_verdict deny "my-private-tool のドリフトを見つけた"
  # ask: org 名の裸の単体出現(スラッシュ無し)
  expect_verdict ask "会社は acme という名前です"
  expect_verdict ask "user@acmecorp.example のような紛らわしいドメインではなく単語一致 acme のみ"
  # pass: 無関係
  expect_verdict pass "これは無関係な文章です"
  expect_verdict pass "academia という単語だけの文章"

  # gather_scan_text: gh/git push の検出とフォールスルー
  local out
  out="$(gather_scan_text 'gh pr create --title x --body y')" || { echo "FAIL: gh pr create を検出しなかった" >&2; fails=$((fails + 1)); }
  gather_scan_text 'gh pr view 1' && { echo "FAIL: gh pr view まで拾ってしまった" >&2; fails=$((fails + 1)); }
  gather_scan_text 'echo hello' && { echo "FAIL: 無関係なコマンドを拾ってしまった" >&2; fails=$((fails + 1)); }

  # git push の diff スキャン(実リポで検証)
  local repo="$tmp/repo"
  mkdir -p "$repo"
  git -C "$repo" init -qb main
  git -C "$repo" config core.hooksPath /dev/null
  git -C "$repo" config user.email test@example.invalid
  git -C "$repo" config user.name test
  git -C "$repo" config commit.gpgsign false
  echo "clean content" > "$repo/a.txt"
  git -C "$repo" add a.txt
  git -C "$repo" commit -qm initial
  git init -q --bare "$tmp/remote.git"
  git -C "$repo" remote add origin "$tmp/remote.git"
  git -C "$repo" push -q origin main
  git -C "$repo" symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main
  echo "acme/secret-project が話題" >> "$repo/a.txt"
  git -C "$repo" add a.txt
  git -C "$repo" commit -qm "mentions acme/secret-project"

  local pushdir
  pushdir="$(cd "$repo" && scan_push_text)"
  [[ $pushdir == *"acme/secret-project"* ]] || {
    echo "FAIL: scan_push_text が diff 内の denylist 文字列を拾わなかった" >&2
    fails=$((fails + 1))
  }

  # decide(): 実際の PreToolUse 相当の入力で deny/ask/フォールスルーを確認
  (
    cd "$repo"
    build_patterns
    local d
    d="$(decide 'git push origin main')" || d="pass"
    [[ $d == deny ]] || { echo "FAIL(decide git push): 期待=deny 実際=$d" >&2; exit 1; }
    d="$(decide 'gh pr create --title t --body "acme is our employer"')" || d="pass"
    [[ $d == ask ]] || { echo "FAIL(decide gh pr create ask): 期待=ask 実際=$d" >&2; exit 1; }
    d="$(decide 'git status')" || d="pass"
    [[ $d == pass ]] || { echo "FAIL(decide git status): 期待=pass 実際=$d" >&2; exit 1; }
  ) || fails=$((fails + 1))

  # PUBLIC_PUBLISH_GUARD_ALLOW=1 は main() 冒頭で即 exit 0(hook 全体の bypass)。
  local hook_out
  hook_out="$(cd "$repo" && PUBLIC_PUBLISH_GUARD_ALLOW=1 \
    "$SELF" <<< '{"tool_name":"Bash","tool_input":{"command":"git push origin main"}}')"
  [[ -z $hook_out ]] || { echo "FAIL: ALLOW=1 でも出力があった" >&2; fails=$((fails + 1)); }

  # PRIVATE な対象リポは検査自体をスキップする(--repo 明示指定)。
  cat > "$tmp/bin-gh" <<'STUB'
#!/usr/bin/env bash
case "$1 $2" in
  "repo list") printf '["my-private-tool"]' ;;
  "repo view") printf 'PRIVATE' ;;
  *) exit 1 ;;
esac
STUB
  hook_out="$(cd "$repo" && rm -f "$PUBLIC_PUBLISH_GUARD_STATE_DIR/visibility-cache.tsv" && \
    "$SELF" <<< '{"tool_name":"Bash","tool_input":{"command":"gh pr create --repo acme/other --title t --body \"acme/secret-project\""}}')"
  [[ -z $hook_out ]] || { echo "FAIL: PRIVATE な対象なのに検査された: $hook_out" >&2; fails=$((fails + 1)); }

  if [[ $fails -gt 0 ]]; then
    echo "selftest: ${fails} 件失敗" >&2
    exit 1
  fi
  echo "selftest: OK"
}

case "${1-}" in
  --selftest) selftest ;;
  --refresh-cache)
    refresh_private_repo_cache && printf 'private-repos-cache.json を更新しました。\n' || printf 'キャッシュ更新に失敗しました(gh の認証状態を確認してください)。\n' >&2
    ;;
  --audit)
    shift
    audit "$@"
    ;;
  -h | --help) usage ;;
  "") main ;;
  *)
    usage >&2
    exit 2
    ;;
esac
