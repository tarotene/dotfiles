#!/bin/sh
# herdr-codex-metadata — Codex CLI の model(+ git branch)を Herdr サイドバーに
# 流す。仕組みは config/claude/hooks/herdr-claude-metadata.sh(Claude 版)と同じ:
# herdr 統合 hook(~/.codex/herdr-agent-state.sh、herdr 管理・編集禁止)と同じ
# パターンで unix socket に JSON 1 行、失敗は無音。Herdr 外(HERDR_ENV なし)や
# 依存欠如では黙って exit 0 する(ADR-0005 の binary-existence gating に倣う)。
#
# Codex hook input の permission_mode は lossy(実測では bypassPermissions か
# default の 2 値しか出ない)なので敢えて出さない — 古い/誤った状態を見せるより
# 出さない方が正直。agent_id 付きイベント(サブエージェント)は親と同じペインで
# 走るため無視する。
#
# PreToolUse には登録しない(全ツール呼び出しで発火するが、model はターン中に
# 変わる頻度が低く、SessionStart/UserPromptSubmit/Stop で十分足りる)ので、
# Claude 版と違い前回値のキャッシュ(debounce)は持たない。
#
# 詳細は docs/claude/herdr-sidebar-metadata.md。
#
# 自己検査: sh config/codex/hooks/herdr-codex-metadata.sh --selftest

set -eu

# payload の 4 フィールドを $event/$model/$subagent/$cwd に取り出す。
#
# 1 行 TSV + 単一 read は使わない: タブは POSIX の IFS whitespace なので
# 連続タブが 1 個の区切りに潰れ、空フィールド(model が無い payload など)が
# 消えてフィールドが 1 個ずつシフトし、cwd が空になって branch/oshi が死ぬ
# (Claude 版で実害が出た同型のバグ、#305)。1 フィールド 1 行 + 逐次 read なら
# 空行がそのまま空フィールドとして残る。値に含まれる改行は jq 側で潰す。
parse_payload() {
  _vals="$(jq -r '[
    (.hook_event_name // ""),
    (.model // ""),
    (if .agent_id then "1" else "0" end),
    (.cwd // "")
  ] | map(tostring | gsub("[\r\n]"; " ")) | .[]' "$1" 2>/dev/null)" || return 1

  # 末尾フィールドが空だと $() が行を落とすため、read の失敗は空値として扱う。
  {
    IFS= read -r event || event=""
    IFS= read -r model || model=""
    IFS= read -r subagent || subagent=""
    IFS= read -r cwd || cwd=""
  } <<EOF
$_vals
EOF
}

# --selftest: 空フィールド入りの合成 payload でフィールド対応が崩れないことを
# 検査する(#305)。
selftest() {
  command -v jq >/dev/null 2>&1 || { echo "selftest: jq required" >&2; return 1; }
  _tmp="$(mktemp -d "${TMPDIR:-/tmp}/herdr-codex-metadata-selftest.XXXXXX")" || return 1
  _fail=0

  _eq() { # label got want
    if [ "$2" != "$3" ]; then
      echo "selftest: FAIL $1: got '$2', want '$3'" >&2
      _fail=1
    fi
  }

  _case() { # name json expect_event expect_model expect_subagent expect_cwd
    printf '%s' "$2" >"$_tmp/in.json"
    event=""; model=""; subagent=""; cwd=""
    parse_payload "$_tmp/in.json" || true
    _eq "[$1] event" "$event" "$3"
    _eq "[$1] model" "$model" "$4"
    _eq "[$1] subagent" "$subagent" "$5"
    _eq "[$1] cwd" "$cwd" "$6"
  }

  _case "full" \
    '{"hook_event_name":"SessionStart","model":"gpt-5","cwd":"/tmp/a"}' \
    "SessionStart" "gpt-5" "0" "/tmp/a"
  _case "no-model" \
    '{"hook_event_name":"SessionStart","cwd":"/tmp/a"}' \
    "SessionStart" "" "0" "/tmp/a"
  _case "empty-model" \
    '{"hook_event_name":"Stop","model":"","cwd":"/tmp/a"}' \
    "Stop" "" "0" "/tmp/a"
  _case "subagent" \
    '{"hook_event_name":"Stop","model":"gpt-5","agent_id":"x","cwd":"/tmp/a"}' \
    "Stop" "gpt-5" "1" "/tmp/a"
  _case "no-cwd" \
    '{"hook_event_name":"SessionEnd","model":"gpt-5"}' \
    "SessionEnd" "gpt-5" "0" ""
  _case "all-optional-missing" \
    '{"hook_event_name":"SessionStart"}' \
    "SessionStart" "" "0" ""

  rm -rf "$_tmp"
  [ "$_fail" = 0 ] && echo "selftest: all passed"
  return "$_fail"
}

if [ "${1:-}" = "--selftest" ]; then
  selftest
  exit $?
fi

hook_input_file="$(mktemp "${TMPDIR:-/tmp}/herdr-codex-metadata.XXXXXX")" || exit 0
trap 'rm -f "$hook_input_file"' EXIT HUP INT TERM
cat >"$hook_input_file" 2>/dev/null || true

[ "${HERDR_ENV:-}" = "1" ] || exit 0
[ -n "${HERDR_SOCKET_PATH:-}" ] || exit 0
[ -n "${HERDR_PANE_ID:-}" ] || exit 0
command -v jq >/dev/null 2>&1 || exit 0
command -v python3 >/dev/null 2>&1 || exit 0

parse_payload "$hook_input_file" || exit 0

# サブエージェントは親と同じペインで走る — 親の表示を撹乱させない。
[ "$subagent" = "1" ] && exit 0

case "$event" in
  SessionEnd)
    # model/branch は取らない(payload に model が無く、表示も全クリアする)。
    ;;
  SessionStart | UserPromptSubmit | Stop)
    # model が空でも branch/oshi は送る(model トークンだけ非表示、#305)。
    ;;
  *)
    exit 0
    ;;
esac

# worktree/ プレフィクスは表示幅節約のため落とす。取得失敗は単に空 —
# herdr 外・非 git cwd でも無害。SessionEnd では取らない(すべて null で送る)。
branch=""
oshi=""
if [ "$event" != "SessionEnd" ] && [ -n "$cwd" ]; then
  branch="$(git -C "$cwd" branch --show-current 2>/dev/null || true)"
  branch="${branch#worktree/}"

  # worktree ディレクトリ名(worktree-<name>-<hex4>、
  # patches/herdr-worktree-names.patch が生成する hololive タレント名)から
  # ファンマーク(推しマーク)絵文字を引く。branch はリネームされうるが
  # ディレクトリ名は不変なのでこちらから抽出する。詳細は
  # docs/claude/herdr-sidebar-metadata.md。
  top="$(git -C "$cwd" rev-parse --show-toplevel 2>/dev/null || true)"
  case "${top##*/}" in
    worktree-*-*)
      talent="${top##*/worktree-}"
      talent="${talent%-*}"
      marks="${XDG_CONFIG_HOME:-$HOME/.config}/herdr/oshi-marks.tsv"
      [ -f "$marks" ] && oshi="$(awk -F'\t' -v n="$talent" \
        '!/^#/ && $1==n {print $2; exit}' "$marks")"
      ;;
  esac
fi

HCM_EVENT="$event" HCM_MODEL="$model" HCM_BRANCH="$branch" HCM_OSHI="$oshi" python3 - <<'PY'
import json
import os
import random
import socket
import time

event = os.environ["HCM_EVENT"]
model = os.environ.get("HCM_MODEL") or None
branch = os.environ.get("HCM_BRANCH") or None
oshi = os.environ.get("HCM_OSHI") or None
pane_id = os.environ["HERDR_PANE_ID"]
socket_path = os.environ["HERDR_SOCKET_PATH"]

params = {
    "pane_id": pane_id,
    "source": "codex-hook",
    "seq": time.time_ns(),
    "tokens": {"model": model, "branch": branch, "oshi": oshi},
}
if event != "SessionEnd":
    params["ttl_ms"] = 14_400_000  # 4h — SessionEnd クリアの保険

request = {
    "id": f"codex-hook:{int(time.time() * 1000)}:{random.randrange(1_000_000):06d}",
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
