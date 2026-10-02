#!/usr/bin/env bash
# bleep-canary.sh — 配備された bleep hook の canary(#604、CI から移設)。
#
# home/modules/claude.nix の home.checks が、generation の build 時にこれを
# 配備後のレイアウト(`<home-files>/.claude/hooks`)に対して走らせる。bleep 入力の
# pin が決める hook の実体(.claude/hooks/bleep/ の bash 本体と、crane でビルドした
# .claude/hooks/bleep-hook)に、登録コマンド(claude.nix の bleepEnvPrefix)と同じ環境を
# 与え、隔離した denylist と stub の gh(全リポジトリが private の cwd、pub/repo だけ
# PUBLIC)で判定させる。bleep 側の破壊的変更に dotfiles の配線が追従していない版は、
# ここで落ちて generation が build できない(hms が bleep main を適用前に確かめる
# 根拠、crates/hms)。`bleep doctor` は導入の健全性だけで、実コマンドに canary を
# 通す検査は持たない(bleep の cmd_doctor のコメント)ので、これは置き換えられない。
#
# 使い方: bleep-canary.sh <hooks-dir>
set -euo pipefail

H="${1:?usage: bleep-canary.sh <hooks-dir>}"
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/config" "$T/state" "$T/cwd"
printf 'acme\n' >"$T/config/orgs.txt"
printf 'acme/secret-project\n' >"$T/config/repos.txt"
cat >"$T/bin-gh" <<'STUB'
#!/usr/bin/env bash
case "$1 $2" in
  "repo list") printf 'my-private-tool\n' ;;
  "repo view") if [[ "$*" == *pub/repo* ]]; then printf 'PUBLIC'; else printf 'PRIVATE'; fi ;;
  "api user") printf 'test-owner' ;;
  *) exit 1 ;;
esac
STUB
chmod +x "$T/bin-gh"

# 生の判定 JSON(失敗時に理由文を出すため、permissionDecision の抽出は expect 側)。
decide_raw() {
  (cd "$T/cwd" &&
    BLEEP_HOOK_BIN="$H/bleep-hook" BLEEP_LEX_BIN="$H/bleep-hook" \
      BLEEP_CONFIG_DIR="$T/config" BLEEP_STATE_DIR="$T/state" \
      BLEEP_ORGS_FILE="$T/config/orgs.txt" BLEEP_REPOS_FILE="$T/config/repos.txt" \
      BLEEP_OWNER=test-owner BLEEP_GH_BIN="$T/bin-gh" BLEEP_LEDGER_DIR="$T/ledger" \
      bash "$H/bleep/hooks/bleep.sh" --host=claude <<<"$1")
}

fail=0
expect() { # $1=label $2=expected decision ("" = pass) $3=hook input JSON
  local raw got
  raw="$(decide_raw "$3" 2>&1 || true)"
  got="$(sed -n 's/.*"permissionDecision":"\([a-z]*\)".*/\1/p' <<<"$raw")"
  if [ "$got" = "$2" ]; then
    echo "ok   $1 -> ${got:-pass}"
  else
    echo "bleep canary '$1': expected '${2:-pass}', got '${got:-pass}'" >&2
    echo "  raw: $raw" >&2
    fail=1
  fi
}

# bleep ADR-0003 (#675): gh posts are accepted only in the canonical form
# (literal -R + an absolute --body-file), so the denylist match must be
# exercised through a body file; an inline --body is denied by the grammar
# before any content is looked at.
printf 'acme/secret-project\n' >"$T/body-secret.md"
printf 'nothing sensitive\n' >"$T/body-clean.md"
bash_cmd() { jq -nc --arg c "$1" '{tool_name:"Bash",tool_input:{command:$c}}'; }

expect "gh -R before subcommand" deny "$(bash_cmd "gh -R pub/repo issue create --title t --body-file $T/body-secret.md")"
expect "gh -R after subcommand" deny "$(bash_cmd "gh issue create -R pub/repo --title t --body-file $T/body-secret.md")"
expect "gh --repo after subcommand" deny "$(bash_cmd "gh pr create --repo pub/repo --title t --body-file $T/body-secret.md")"
expect "MCP GitHub post" deny '{"tool_name":"mcp__github__create_issue","tool_input":{"owner":"pub","repo":"repo","title":"t","body":"acme/secret-project"}}'
expect "inline --body is outside the canonical form" deny "$(bash_cmd 'gh issue create -R pub/repo --title t --body "x"')"
expect "unresolved shell variable is outside the canonical form" deny "$(bash_cmd "gh issue create -R \$TARGET --title t --body-file $T/body-clean.md")"
expect "canonical post with a clean body" "" "$(bash_cmd "gh issue create -R pub/repo --title t --body-file $T/body-clean.md")"
expect "push that disables pre-push" deny "$(bash_cmd 'git push --no-verify origin x')"
expect "unrelated command" "" "$(bash_cmd 'ls -la')"
exit $fail
