#!/bin/sh
# herdr-copilot-metadata — Copilot CLI の model(+ git branch)を Herdr
# サイドバーに流す。仕組みは config/claude/hooks/herdr-claude-metadata.sh
# (Claude 版)と同じ: herdr 統合 hook(~/.copilot/hooks/herdr-agent-state.sh、
# herdr 管理・編集禁止)と同じパターンで unix socket に JSON 1 行、失敗は無音。
# Herdr 外(HERDR_ENV なし)や依存欠如では黙って exit 0 する(ADR-0005 の
# binary-existence gating に倣う)。
#
# Copilot の hook input には event 名も model も乗らない(公式リファレンス:
# 全イベント共通で sessionId/timestamp/cwd のみ)。そのため:
#   - イベントは argv で渡す(herdr 自身の `herdr-agent-state.sh session` と
#     同じパターン)。$1 が "report" なら報告、"clear" なら全 token null クリア。
#   - model は payload からではなく `~/.copilot/settings.json` の `.model`
#     (Copilot CLI 自身が永続化する現在のモデル設定)から読む。/model 直後は
#     Copilot の書き込みタイミング次第で遅延しうるが、表示専用なので許容する。
#
# 詳細は docs/claude/herdr-sidebar-metadata.md。
#
# 自己検査: sh config/copilot/hooks/herdr-copilot-metadata.sh --selftest

set -eu

# argv の action が既知(report / clear)かを判定する。
valid_action() {
  case "$1" in
    report | clear) return 0 ;;
    *) return 1 ;;
  esac
}

# payload ファイルから cwd を取り出す。欠落・不正 JSON は空文字。
payload_cwd() {
  jq -r '.cwd // ""' "$1" 2>/dev/null || true
}

# Copilot CLI の settings.json から現在の model を取り出す。ファイル欠落・
# 不正 JSON は空文字。
settings_model() {
  [ -f "$1" ] || return 0
  jq -r '.model // ""' "$1" 2>/dev/null || true
}

# worktree トップレベルのパス(worktree-<name>-<hex4>)から推しマークを引く。
# 該当しなければ空文字。
oshi_for_toplevel() { # toplevel marks_file
  case "${1##*/}" in
    worktree-*-*)
      _talent="${1##*/worktree-}"
      _talent="${_talent%-*}"
      [ -f "$2" ] && awk -F'\t' -v n="$_talent" \
        '!/^#/ && $1==n {print $2; exit}' "$2"
      ;;
  esac
  return 0
}

# --selftest: 切り出した純粋ロジックを合成入力で検査する。stdin を読む前に
# dispatch するので、パイプ無しで実行できる。
selftest() {
  command -v jq >/dev/null 2>&1 || { echo "selftest: jq required" >&2; return 1; }
  _tmp="$(mktemp -d "${TMPDIR:-/tmp}/herdr-copilot-metadata-selftest.XXXXXX")" || return 1
  _fail=0

  _eq() { # label got want
    if [ "$2" != "$3" ]; then
      echo "selftest: FAIL $1: got '$2', want '$3'" >&2
      _fail=1
    fi
  }

  _action() { # name action expect(0|1)
    _rc=0
    valid_action "$2" || _rc=1
    _eq "[action $1]" "$_rc" "$3"
  }
  _action "report" "report" 0
  _action "clear" "clear" 0
  _action "empty" "" 1
  _action "unknown" "session" 1

  _cwd() { # name json expect
    printf '%s' "$2" >"$_tmp/in.json"
    _eq "[cwd $1]" "$(payload_cwd "$_tmp/in.json")" "$3"
  }
  _cwd "full" '{"sessionId":"s","timestamp":1,"cwd":"/tmp/a"}' "/tmp/a"
  _cwd "no-cwd" '{"sessionId":"s","timestamp":1}' ""
  _cwd "empty-payload" '' ""
  _cwd "invalid-json" '{' ""

  _model() { # name settings_json expect
    printf '%s' "$2" >"$_tmp/settings.json"
    _eq "[model $1]" "$(settings_model "$_tmp/settings.json")" "$3"
  }
  _model "set" '{"model":"gpt-5"}' "gpt-5"
  _model "unset" '{"theme":"dark"}' ""
  _model "invalid-json" '{' ""
  _eq "[model missing-file]" "$(settings_model "$_tmp/nonexistent.json")" ""

  printf '# comment\nsuisei\t☄️\nmarine\t🏴‍☠️\n' >"$_tmp/marks.tsv"
  _oshi() { # name toplevel expect
    _eq "[oshi $1]" "$(oshi_for_toplevel "$2" "$_tmp/marks.tsv")" "$3"
  }
  _oshi "hit" "/w/worktree-suisei-ab12" "☄️"
  _oshi "miss" "/w/worktree-pekora-ab12" ""
  _oshi "not-worktree" "/w/dotfiles" ""
  _oshi "comment-line" "/w/worktree-#-ab12" ""
  _eq "[oshi no-marks-file]" "$(oshi_for_toplevel /w/worktree-suisei-ab12 "$_tmp/none.tsv")" ""

  rm -rf "$_tmp"
  [ "$_fail" = 0 ] && echo "selftest: all passed"
  return "$_fail"
}

if [ "${1:-}" = "--selftest" ]; then
  selftest
  exit $?
fi

action="${1:-}"
hook_input_file="$(mktemp "${TMPDIR:-/tmp}/herdr-copilot-metadata.XXXXXX")" || exit 0
trap 'rm -f "$hook_input_file"' EXIT HUP INT TERM
cat >"$hook_input_file" 2>/dev/null || true

valid_action "$action" || exit 0

[ "${HERDR_ENV:-}" = "1" ] || exit 0
[ -n "${HERDR_SOCKET_PATH:-}" ] || exit 0
[ -n "${HERDR_PANE_ID:-}" ] || exit 0
command -v jq >/dev/null 2>&1 || exit 0
command -v python3 >/dev/null 2>&1 || exit 0

model=""
branch=""
oshi=""
if [ "$action" = "report" ]; then
  cwd="$(payload_cwd "$hook_input_file")"
  # worktree/ プレフィクスは表示幅節約のため落とす。取得失敗は単に空 —
  # herdr 外・非 git cwd でも無害。
  if [ -n "$cwd" ]; then
    branch="$(git -C "$cwd" branch --show-current 2>/dev/null || true)"
    branch="${branch#worktree/}"

    # worktree ディレクトリ名(worktree-<name>-<hex4>、
    # patches/herdr-worktree-names.patch が生成する hololive タレント名)から
    # ファンマーク(推しマーク)絵文字を引く。branch はリネームされうるが
    # ディレクトリ名は不変なのでこちらから抽出する。詳細は
    # docs/claude/herdr-sidebar-metadata.md。
    top="$(git -C "$cwd" rev-parse --show-toplevel 2>/dev/null || true)"
    oshi="$(oshi_for_toplevel "$top" "${XDG_CONFIG_HOME:-$HOME/.config}/herdr/oshi-marks.tsv")"
  fi
  model="$(settings_model "${HOME:-}/.copilot/settings.json")"
fi

HCM_MODEL="$model" HCM_BRANCH="$branch" HCM_OSHI="$oshi" HCM_ACTION="$action" python3 - <<'PY'
import json
import os
import random
import socket
import time

action = os.environ["HCM_ACTION"]
model = os.environ.get("HCM_MODEL") or None
branch = os.environ.get("HCM_BRANCH") or None
oshi = os.environ.get("HCM_OSHI") or None
pane_id = os.environ["HERDR_PANE_ID"]
socket_path = os.environ["HERDR_SOCKET_PATH"]

params = {
    "pane_id": pane_id,
    "source": "copilot-hook",
    "seq": time.time_ns(),
    "tokens": {"model": model, "branch": branch, "oshi": oshi},
}
if action == "report":
    params["ttl_ms"] = 14_400_000  # 4h — clear 取りこぼしの保険

request = {
    "id": f"copilot-hook:{int(time.time() * 1000)}:{random.randrange(1_000_000):06d}",
    "method": "pane.report_metadata",
    "params": params,
}

try:
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.settimeout(0.5)
    client.connect(socket_path)
    client.sendall((json.dumps(request) + "\n").encode())
    try:
        client.recv(4096)
    except Exception:
        pass
    client.close()
except Exception:
    pass
PY
