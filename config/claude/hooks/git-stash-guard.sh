#!/usr/bin/env bash
# git-stash-guard.sh — 素の `git stash` を弾く PreToolUse hook。
#
# 設計と根拠: docs/claude/git-stash-guard.md(このリポジトリ内)
#
# stash スタックは herdr が worktree ごとに切るセッション間で共有されている
# (git のスタックはリポジトリ単位で、worktree 単位ではない)。素の
# `git stash` / `git stash pop` / `git stash apply`(SHA 無し)は、他の
# worktree が積んだ WIP を取り違えて pop/apply したり、自分の WIP を
# 見分けの付かない stash に積んで埋もれさせたりする事故につながる。
#
# 許可(通す)条件:
#   git stash list / git stash show                     — 参照のみ
#   git stash push -u -m <tag>                           — タグ付きで積む
#   git stash apply <SHA>  / git stash drop <SHA>         — SHA 指定(取り違え防止)
# それ以外の stash 呼び出し(裸の stash・push で -u/-m 抜け・pop・SHA 無しの
# apply/drop・clear)は deny する。
#
# git-worktree-allow.sh(allow 側)との非対称性:
#   git-worktree-allow は「if 不一致 = hook が spawn しない = 通常のプロンプト
#   フローに委ねる」ことを安全側とみなせる(許可を出さないだけ)。deny 側の
#   この hook では逆で、「if 不一致 = 検査されず素通り」が事故そのものになる。
#   そして `registerHooks` は command 文字列の完全一致でしか存在判定しない
#   ため、同一スクリプトを 2 つの if で二重登録することもできない
#   (2 回目の register が 1 回目を見つけてスキップする)。よって if は
#   `Bash(git *)` まで広げ、絞り込みは hook 内部の早期 exit に移した。
#   代償は git コマンドごとに短命なプロセス 1 個。
#
# 既知の限界: if は `Bash(git *)` という「コマンド文字列の先頭一致」なので、
# `cd wt && git stash pop` のように git が先頭語でない複合コマンドはそもそも
# hook を spawn させない。脅威モデルは敵対的入力ではなく Claude 自身が生成
# するコマンドなので許容している(git-worktree-allow と同じ前提)。
#
# 縮退(ADR-0005 の binary-existence gating に倣う): jq 不在・stdin 不正は黙って exit 0。
#
# 使い方:
#   hook として: settings.json の PreToolUse(matcher: Bash, if: "Bash(git *)")
#                から stdin JSON で呼ばれる
#   自己検査:   git-stash-guard.sh --selftest
set -euo pipefail

has_flag() { # has_flag <長形> <短形> "${args[@]}"
  local long="$1" short="$2"
  shift 2
  local a
  for a in "$@"; do
    [[ "$a" == "$long" || "$a" == "$long"=* || "$a" == "$short" ]] && return 0
  done
  return 1
}

has_sha() { # 40 桁 hex の引数が 1 つでもあるか
  local a
  for a in "$@"; do
    [[ "$a" =~ ^[0-9a-f]{40}$ ]] && return 0
  done
  return 1
}

# $1 = 複合コマンドを含まない「単一の呼び出し」1 つ。deny なら理由文を stdout
# に出して 0、それ以外(フォールスルー)は非 0 を返す。
#
# トークン分割は素朴な空白分割(git-worktree-allow と同じ割り切り)。
# "git stash …" と "git -C <dir> stash …" の 2 形だけを扱う — `-c` などの
# グローバルオプションを挟む形は対象外(フォールスルー)。
decide_single() {
  local seg="$1"
  local -a tok
  read -r -a tok <<< "$seg"
  [[ ${#tok[@]} -ge 1 && ${tok[0]} == "git" ]] || return 1

  local idx=1
  if [[ ${idx} -lt ${#tok[@]} && ${tok[1]:-} == "-C" ]]; then
    idx=3
  fi
  [[ ${idx} -lt ${#tok[@]} ]] || return 1
  [[ ${tok[$idx]} == "stash" ]] || return 1

  local -a args=("${tok[@]:$((idx + 1))}")
  local sub="${args[0]:-push}" # 裸の `git stash` は push と同じ

  case "$sub" in
    list | show)
      return 1 # 参照のみ、通す
      ;;
    push)
      if has_flag --include-untracked -u "${args[@]:1}" \
        && has_flag --message -m "${args[@]:1}"; then
        return 1 # -u かつ -m: タグ付きで積む正規の手順、通す
      fi
      printf 'git stash push は -u と -m <tag> を両方付けてください(deny)'
      ;;
    apply | drop)
      if has_sha "${args[@]:1}"; then
        return 1 # SHA 指定: 取り違えない、通す
      fi
      printf 'git stash %s は SHA を明示してください(deny)。%s' \
        "$sub" "git stash list --format='%H %gs' で確認してから apply/drop してください"
      ;;
    pop | clear)
      printf 'git stash %s は他の worktree の WIP を巻き込みます(deny)' "$sub"
      ;;
    *)
      # 未知の形(--keep-index 等の push 相当フラグ, stash@{N} を裸で渡す等)
      # は安全側 = deny に倒す。
      printf '未知の git stash 呼び出しです(deny): %s' "$seg"
      ;;
  esac
}

# $1 = Bash ツールのコマンド文字列全体。deny なら理由文を stdout に出して 0、
# それ以外(フォールスルー)は非 0 を返す。
#
# 複合コマンド(改行 `;` `&&` `||` `|` を含むもの)は個々の文に分解してから
# decide_single に渡す。最初は「改行や `;` を含む = 複合コマンド = stash を
# 含んでいれば丸ごと deny」という単純な実装だったが、これは
# 「echo "..."; ls .../git-stash-guard.sh」のような、"stash" という単語が
# パスに含まれるだけの無関係な複数行コマンドまで deny してしまう実害のある
# 誤検知だった(このファイル自身のパスで実際に踏んだ)。分解してから見る。
decide() {
  local cmd="$1"

  # 大多数の git コマンドはここで抜ける(bash 起動 + grep 1 回のコスト)。
  # 単語境界で見る — commit メッセージに "stash" という文字列が含まれる
  # だけの呼び出し(git commit -m "add stash guard")を後段で確実に落とす
  # ための最初のフィルタなので、ここは広めに構ってよい。
  grep -qw stash <<< "$cmd" || return 1

  # 複数文字の演算子を先に改行へ正規化してから、単純な文字クラスで分割する
  # (クォート等を解釈しない素朴な文字列置換 — git-worktree-allow と同じ
  # 割り切り)。
  local normalized="$cmd"
  normalized="${normalized//&&/$'\n'}"
  normalized="${normalized//||/$'\n'}"
  normalized="${normalized//;/$'\n'}"
  normalized="${normalized//|/$'\n'}"

  local seg reason
  while IFS= read -r seg; do
    [[ -n "$seg" ]] || continue

    # コマンド置換・リダイレクトを含む文は個別サブコマンドへの厳密な分解を
    # 諦め、"stash" という語を含んでいればその文だけ deny 側に倒す
    # (git-worktree-allow の allow 側とは向きが逆 — 素通しは事故そのもの)。
    if [[ "$seg" == *'$('* || "$seg" == *'`'* || "$seg" == *'>'* || "$seg" == *'<'* ]]; then
      if grep -qw stash <<< "$seg"; then
        printf '複合コマンドの中に stash が含まれています(deny): %s' "$seg"
        return 0
      fi
      continue
    fi

    if reason="$(decide_single "$seg")"; then
      printf '%s' "$reason"
      return 0
    fi
  done <<< "$normalized"

  return 1
}

main() {
  command -v jq > /dev/null 2>&1 || exit 0

  local input cmd reason
  input="$(cat)" || exit 0
  grep -qw stash <<< "$input" || exit 0 # jq を呼ぶ前の最速フィルタ

  cmd="$(jq -r 'select(.tool_name == "Bash") | .tool_input.command // empty' \
    <<< "$input" 2> /dev/null)" || exit 0
  [[ -n $cmd ]] || exit 0

  if reason="$(decide "$cmd")"; then
    jq -n --arg reason "$reason" '{
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "deny",
        permissionDecisionReason: $reason
      }
    }'
  fi
  exit 0
}

selftest() {
  local fails=0

  expect_deny() {
    local out
    if ! out="$(decide "$1")"; then
      echo "FAIL(deny 期待, フォールスルーした): $1" >&2
      fails=$((fails + 1))
    elif [[ -z "$out" ]]; then
      echo "FAIL(deny 期待, 理由が空): $1" >&2
      fails=$((fails + 1))
    fi
  }
  expect_pass() {
    if decide "$1" > /dev/null; then
      echo "FAIL(pass 期待, deny された): $1" >&2
      fails=$((fails + 1))
    fi
  }

  local sha="0123456789abcdef0123456789abcdef01234567"

  # deny されるべき形
  expect_deny "git stash"
  expect_deny "git stash push"
  expect_deny "git stash push -u"
  expect_deny "git stash push -m wip"
  expect_deny "git stash pop"
  expect_deny "git stash apply"
  expect_deny "git stash drop"
  expect_deny "git stash clear"
  expect_deny "git -C /some/worktree stash pop"
  expect_deny "git -C /some/worktree stash"
  expect_deny "git stash --keep-index"

  # 複合コマンド(stash を含んでいれば deny 側に倒す)
  expect_deny "git status; git stash pop"
  expect_deny "git stash pop && echo done"
  expect_deny "git stash pop | cat"
  expect_deny "git stash apply \$(evil)"

  # 通すべき形
  expect_pass "git stash list"
  expect_pass "git stash list --format='%H %gs'"
  expect_pass "git stash show -p"
  expect_pass "git stash push -u -m rescue-tag"
  expect_pass "git stash push --include-untracked --message=rescue-tag"
  expect_pass "git -C /some/worktree stash push -u -m rescue-tag"
  expect_pass "git stash apply $sha"
  expect_pass "git stash drop $sha"
  expect_pass "git -C /some/worktree stash apply $sha"

  # stash と無関係な git 呼び出し(文字列に "stash" を含むだけのものも含む)
  expect_pass "git status"
  expect_pass "git commit -m 'add stash guard'"
  expect_pass "git -C /some/worktree commit -m wip"
  expect_pass "git log --oneline -- stash-notes.md"

  # 回帰: 改行を含む複数文のコマンドで、どの文も実際には git stash を
  # 呼んでいない(パスやコメントに "stash" という単語が出るだけ)なら通す。
  # 実際にこのファイル自身のパスを含む複数行コマンドで誤 deny された。
  expect_pass "$(printf 'echo hi\nls -la ~/.claude/hooks/git-stash-guard.sh')"
  expect_pass "$(printf 'git status\ngit commit -m wip')"
  expect_pass "$(printf 'echo about-stash-guard.md\ngit log --oneline')"

  # 分解後、片方の文だけが実際の stash 呼び出しなら、その文だけで deny する。
  expect_deny "$(printf 'echo hi\ngit stash pop')"

  if [[ $fails -gt 0 ]]; then
    echo "selftest: ${fails} 件失敗" >&2
    exit 1
  fi
  echo "selftest: OK"
}

if [[ ${1-} == "--selftest" ]]; then
  selftest
else
  main
fi
