#!/usr/bin/env bash
# git-worktree-allow.sh — herdr worktree を外から駆動する `git -C` を許可する PreToolUse hook。
#
# 設計と根拠: docs/claude/git-worktree-allow.md(このリポジトリ内)
#
# `Bash(git -C * add *)` のような中間ワイルドカードの permission rule は、`-C` の
# 位置に任意のオプション(`--exec-path` 等 = 任意コード実行)を挿し込めるため
# Claude Code が起動時に毎回警告する — しかも中間 `*` は実際にはマッチしない。
# 代わりにこの hook が、`-C` の引数が「実在する herdr worktree 配下のディレクトリ」
# であることを検証したうえで、限られたサブコマンドだけをプログラム的に許可する。
#
# 許可条件(すべて満たすときだけ allow を返す):
#   1. コマンド全体が単一の git 呼び出しであること。`;` `&` `|` `$(` バッククォート
#      リダイレクト・改行を含む複合コマンドは即フォールスルー — hook は生の文字列を
#      見るため、複合コマンドを許可すると別コマンドの同乗を許してしまう
#   2. 形が `git -C <dir> <subcommand> …` に厳密一致(`git` の直後は `-C` のみ。
#      `-c` などのグローバルオプションは不可)
#   3. <dir> は `-` 始まりでない実在ディレクトリで、realpath(symlink 解決後)が
#      ~/.herdr/worktrees/ 配下(テスト用に HERDR_WORKTREES_DIR で上書き可)
#   4. <subcommand> が許可リスト内(status diff log show add commit push —
#      permissionRules の素の git ルール群と同じ守備範囲)
#   5. 残りの引数に git を別実行体へ向けられるものがない(--receive-pack /
#      --upload-pack / --exec-path / ext:: transport)。これは防壁ではなく
#      belt-and-suspenders — 素の `Bash(git push *)` ルールと同等以上の水準を保つ措置
#
# 非該当時は何も出力せず exit 0 する — Claude Code の公式仕様どおり、通常の
# permission フロー(プロンプト)にフォールスルーする。allow を返しても deny
# ルールは上書きされない(最も制限的な決定が勝つ)。
# https://code.claude.com/docs/en/hooks-guide.md
#
# 縮退(ADR-0005 の binary-existence gating に倣う): jq 不在・stdin 不正は黙って exit 0。
#
# 使い方:
#   hook として: settings.json の PreToolUse(matcher: Bash, if: "Bash(git -C *)")
#                から stdin JSON で呼ばれる
#   自己検査:   git-worktree-allow.sh --selftest
set -euo pipefail

ALLOWED_SUBCOMMANDS=(status diff log show add commit push)

# $1 = Bash ツールのコマンド文字列。許可なら理由文を stdout に出して 0、
# それ以外(フォールスルー)は非 0 を返す。
decide() {
  local cmd="$1"
  local root="${HERDR_WORKTREES_DIR:-$HOME/.herdr/worktrees}"

  # 1. 複合コマンド・リダイレクト・展開の拒否
  case "$cmd" in
    *';'* | *'&'* | *'|'* | *'$('* | *'`'* | *'>'* | *'<'* | *$'\n'*) return 1 ;;
  esac

  # 2. 形の厳密一致。トークン分割は素朴な空白分割 — クォートを含むトークンは
  #    以下の実在チェック・許可リスト照合に自然に落ちるので、誤許可側には倒れない。
  local -a tok
  read -r -a tok <<< "$cmd"
  [[ ${#tok[@]} -ge 4 ]] || return 1
  [[ ${tok[0]} == "git" && ${tok[1]} == "-C" ]] || return 1

  # 3. 実在する herdr worktree 配下のディレクトリか(symlink は解決してから判定)
  local dir="${tok[2]}" real
  [[ $dir != -* ]] || return 1
  real="$(realpath -e -- "$dir" 2> /dev/null)" || return 1
  [[ -d $real ]] || return 1
  case "$real/" in
    "$root"/?*) : ;;
    *) return 1 ;;
  esac

  # 4. サブコマンドの許可リスト照合
  local sub="${tok[3]}" s ok=0
  for s in "${ALLOWED_SUBCOMMANDS[@]}"; do
    [[ $sub == "$s" ]] && ok=1
  done
  [[ $ok -eq 1 ]] || return 1

  # 5. git を別実行体へ向けられる引数の拒否
  local t
  for t in "${tok[@]:4}"; do
    case "$t" in
      --receive-pack* | --upload-pack* | --exec-path* | *ext::*) return 1 ;;
    esac
  done

  printf 'git -C %s %s (herdr worktree)' "$real" "$sub"
}

main() {
  command -v jq > /dev/null 2>&1 || exit 0

  local input cmd reason
  input="$(cat)" || exit 0
  cmd="$(jq -r 'select(.tool_name == "Bash") | .tool_input.command // empty' \
    <<< "$input" 2> /dev/null)" || exit 0
  [[ -n $cmd ]] || exit 0

  if reason="$(decide "$cmd")"; then
    jq -n --arg reason "$reason" '{
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "allow",
        permissionDecisionReason: $reason
      }
    }'
  fi
  exit 0
}

selftest() {
  local fails=0
  # trap は関数スコープ外(スクリプト exit 時)で走るので、local にせず今展開する。
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  export HERDR_WORKTREES_DIR="$tmp/worktrees"
  mkdir -p "$tmp/worktrees/wt1/nested" "$tmp/outside"
  ln -s "$tmp/outside" "$tmp/worktrees/escape"

  expect_allow() {
    if ! decide "$1" > /dev/null; then
      echo "FAIL(allow 期待): $1" >&2
      fails=$((fails + 1))
    fi
  }
  expect_deny() {
    if decide "$1" > /dev/null; then
      echo "FAIL(deny 期待): $1" >&2
      fails=$((fails + 1))
    fi
  }

  local wt="$tmp/worktrees/wt1"

  # 許可されるべき形
  expect_allow "git -C $wt status"
  expect_allow "git -C $wt diff --stat"
  expect_allow "git -C $wt add -A"
  expect_allow "git -C $wt commit -m wip"
  expect_allow "git -C $wt push origin HEAD"
  expect_allow "git -C $wt/nested log --oneline -5"

  # 範囲外パス・実在しないパス・symlink 越境・root そのもの
  expect_deny "git -C /tmp status"
  expect_deny "git -C $tmp/worktrees/missing status"
  expect_deny "git -C $tmp/worktrees/escape status"
  expect_deny "git -C $tmp/worktrees status"

  # オプション注入・形の崩れ
  expect_deny "git -C --exec-path=/evil add ."
  expect_deny "git -c core.pager=evil -C $wt status"
  expect_deny "git --exec-path=/evil -C $wt status"
  expect_deny "git -C $wt"
  expect_deny "env git -C $wt status"

  # 範囲外サブコマンド
  expect_deny "git -C $wt rebase main"
  expect_deny "git -C $wt config user.name evil"

  # 複合コマンド・リダイレクト・展開
  expect_deny "git -C $wt status; rm -rf /"
  expect_deny "git -C $wt status && evil"
  expect_deny "git -C $wt status | tee /tmp/x"
  expect_deny "git -C $wt add \$(evil)"
  expect_deny "git -C $wt status > /tmp/x"

  # 別実行体へ向ける引数
  expect_deny "git -C $wt push --receive-pack=/evil origin"
  expect_deny "git -C $wt push --upload-pack=/evil origin"
  expect_deny "git -C $wt push 'ext::sh -c evil'"

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
