#!/bin/sh
# herdr-claude-metadata — Claude Code の permission mode(+ git branch)を
# Herdr サイドバーに流す。
#
# Herdr のトークン色は静的指定しかできないので、モード毎に別トークン
# (mode_plan / mode_default / mode_accept / mode_bypass)を使い、アクティブな
# 1 つにだけ値を入れて他は null でクリアする。config/herdr/config.toml が
# 各トークンに Catppuccin Mocha の fg を割り当てる。ラベルの記号も形で差別化
# する(色だけに頼らない状態表示): ◇ plan(低リスク・輪郭)/ ◆ default(基準・
# 塗り)/ ✓ accept / ▲ bypass(警戒)。
#
# ソケット書き込みは herdr 統合 hook(~/.claude/hooks/herdr-agent-state.sh、
# herdr 管理・編集禁止)と同じパターン: unix socket に JSON 1 行、失敗は無音。
# Herdr 外(HERDR_ENV なし)や依存欠如では黙って exit 0 する(ADR-0005 の
# binary-existence gating に倣う)。詳細は docs/claude/herdr-sidebar-metadata.md。
#
# payload に permission_mode が無い/空でも、branch と oshi は独立に送る
# (mode トークンだけ非表示になる、#305)。
#
#   自己検査: sh config/claude/hooks/herdr-claude-metadata.sh --selftest

set -eu

# payload の 4 フィールドを $event/$mode/$subagent/$cwd に取り出す。
#
# 1 行 TSV + 単一 read は使わない: タブは POSIX の IFS whitespace なので
# 連続タブが 1 個の区切りに潰れ、空フィールド(mode が無い payload など)が
# 消えてフィールドが 1 個ずつシフトする。その結果 cwd が空になり branch/oshi
# が永久に出ない回帰が実際に起きた(#305)。1 フィールド 1 行 + 逐次 read なら
# 空行がそのまま空フィールドとして残る。値に含まれる改行は jq 側で潰す。
parse_payload() {
  _vals="$(jq -r '[
    (.hook_event_name // ""),
    (.permission_mode // ""),
    (if .agent_id then "1" else "0" end),
    (.cwd // "")
  ] | map(tostring | gsub("[\r\n]"; " ")) | .[]' "$1" 2>/dev/null)" || return 1

  # 末尾フィールドが空だと $() が行を落とすため、read の失敗は空値として扱う。
  {
    IFS= read -r event || event=""
    IFS= read -r mode || mode=""
    IFS= read -r subagent || subagent=""
    IFS= read -r cwd || cwd=""
  } <<EOF
$_vals
EOF
}

# --selftest: 空フィールド入りの合成 payload でフィールド対応が崩れないことを
# 検査する。#305 の回帰(mode が空 → cwd が消える)はこの検査で必ず落ちる。
selftest() {
  command -v jq >/dev/null 2>&1 || { echo "selftest: jq required" >&2; return 1; }
  _tmp="$(mktemp -d "${TMPDIR:-/tmp}/herdr-claude-metadata-selftest.XXXXXX")" || return 1
  _fail=0

  _eq() { # label got want
    if [ "$2" != "$3" ]; then
      echo "selftest: FAIL $1: got '$2', want '$3'" >&2
      _fail=1
    fi
  }

  _case() { # name json expect_event expect_mode expect_subagent expect_cwd
    printf '%s' "$2" >"$_tmp/in.json"
    event=""; mode=""; subagent=""; cwd=""
    parse_payload "$_tmp/in.json" || true
    _eq "[$1] event" "$event" "$3"
    _eq "[$1] mode" "$mode" "$4"
    _eq "[$1] subagent" "$subagent" "$5"
    _eq "[$1] cwd" "$cwd" "$6"
  }

  _case "full" \
    '{"hook_event_name":"SessionStart","permission_mode":"plan","cwd":"/tmp/a"}' \
    "SessionStart" "plan" "0" "/tmp/a"
  # 回帰の本体: permission_mode が無い payload でも cwd が生き残ること。
  _case "no-permission_mode" \
    '{"hook_event_name":"SessionStart","cwd":"/tmp/a"}' \
    "SessionStart" "" "0" "/tmp/a"
  _case "empty-permission_mode" \
    '{"hook_event_name":"Stop","permission_mode":"","cwd":"/tmp/a"}' \
    "Stop" "" "0" "/tmp/a"
  _case "subagent" \
    '{"hook_event_name":"PreToolUse","permission_mode":"default","agent_id":"x","cwd":"/tmp/a"}' \
    "PreToolUse" "default" "1" "/tmp/a"
  _case "no-cwd" \
    '{"hook_event_name":"SessionEnd","permission_mode":"default"}' \
    "SessionEnd" "default" "0" ""
  _case "all-optional-missing" \
    '{"hook_event_name":"SessionStart"}' \
    "SessionStart" "" "0" ""
  _case "cwd-with-spaces" \
    '{"hook_event_name":"Stop","permission_mode":"","cwd":"/tmp/a b"}' \
    "Stop" "" "0" "/tmp/a b"

  rm -rf "$_tmp"
  [ "$_fail" = 0 ] && echo "selftest: all passed"
  return "$_fail"
}

if [ "${1:-}" = "--selftest" ]; then
  selftest
  exit $?
fi

hook_input_file="$(mktemp "${TMPDIR:-/tmp}/herdr-claude-metadata.XXXXXX")" || exit 0
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

# 前回報告したモードのキャッシュ。頻発イベント(PreToolUse)では、モードが
# 変わっていない限りここで抜けて python3 の起動コストを払わない。
state_file="${XDG_RUNTIME_DIR:-/tmp}/herdr-claude-mode.$(printf '%s' "$HERDR_PANE_ID" | tr -c 'A-Za-z0-9_-' '_')"

case "$event" in
  SessionEnd)
    # 全トークンをクリアする(セッション終了後の残留表示の即時解消)。
    rm -f "$state_file"
    ;;
  SessionStart | Stop)
    # 常に送る: SessionStart は初期値と前セッションの残留の上書き、
    # Stop は ttl のリフレッシュを兼ねる。mode が空でも branch/oshi は送る
    # (mode トークンだけ落ちる) — payload から permission_mode が消えても
    # ワークツリー識別が道連れで死なないようにするため(#305)。
    ;;
  UserPromptSubmit | PreToolUse)
    # 初回(state_file 無し)は mode が空でも必ず送る。2 回目以降は mode が
    # 変わっていなければ python3 の起動コストを払わずに抜ける。
    if [ -f "$state_file" ]; then
      last="$(cat "$state_file" 2>/dev/null || true)"
      [ "$last" = "$mode" ] && exit 0
    fi
    ;;
  *)
    exit 0
    ;;
esac

# 実際に送る段になってから git ブランチを取る(mode 未変化でスキップする経路
# では呼ばない)。worktree/ プレフィクスは表示幅節約のため落とす。取得失敗は
# 単に空 — herdr 外・非 git cwd でも無害。
branch=""
if [ -n "$cwd" ]; then
  branch="$(git -C "$cwd" branch --show-current 2>/dev/null || true)"
  branch="${branch#worktree/}"
fi

# worktree ディレクトリ名(worktree-<name>-<hex4>、
# patches/herdr-worktree-names.patch が生成する hololive タレント名)から
# ファンマーク(推しマーク)絵文字を引く。branch はリネームされうるが
# ディレクトリ名は不変なのでこちらから抽出する。非 worktree・未確証タレント
# では空 = トークン非表示。詳細は docs/claude/herdr-sidebar-metadata.md。
oshi=""
if [ -n "$cwd" ]; then
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

HCM_EVENT="$event" HCM_MODE="$mode" HCM_BRANCH="$branch" HCM_OSHI="$oshi" HCM_STATE_FILE="$state_file" python3 - <<'PY'
import json
import os
import random
import socket
import time

event = os.environ["HCM_EVENT"]
mode = os.environ["HCM_MODE"]
branch = os.environ.get("HCM_BRANCH") or None
oshi = os.environ.get("HCM_OSHI") or None
state_file = os.environ["HCM_STATE_FILE"]
pane_id = os.environ["HERDR_PANE_ID"]
socket_path = os.environ["HERDR_SOCKET_PATH"]

LABELS = {
    "plan": ("mode_plan", "◇ plan"),
    "default": ("mode_default", "◆ default"),
    "acceptEdits": ("mode_accept", "✓ accept"),
    "bypassPermissions": ("mode_bypass", "▲ bypass"),
}
tokens = {name: None for name, _ in LABELS.values()}
tokens["branch"] = None if event == "SessionEnd" else branch
tokens["oshi"] = None if event == "SessionEnd" else oshi
if event != "SessionEnd":
    if mode in LABELS:
        name, label = LABELS[mode]
        tokens[name] = label
    elif mode:
        # 未知のモード(auto / dontAsk など)は default 用トークンに実名で流す —
        # 古い表示を残すよりは実名表示のほうが正直。
        tokens["mode_default"] = f"◆ {mode}"
    # mode が空(payload に permission_mode が無い)なら mode トークンは全て
    # null のまま = 非表示。branch/oshi は独立に送る(#305)。

params = {
    "pane_id": pane_id,
    "source": "claude-hook",
    "seq": time.time_ns(),
    "tokens": tokens,
}
if event != "SessionEnd":
    params["ttl_ms"] = 14_400_000  # 4h — SessionEnd クリアの保険

request = {
    "id": f"claude-hook:{int(time.time() * 1000)}:{random.randrange(1_000_000):06d}",
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
    if event != "SessionEnd":
        with open(state_file, "w", encoding="utf-8") as handle:
            handle.write(mode)
except Exception:
    pass
PY
