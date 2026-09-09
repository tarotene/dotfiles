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

set -eu

hook_input_file="$(mktemp "${TMPDIR:-/tmp}/herdr-claude-metadata.XXXXXX")" || exit 0
trap 'rm -f "$hook_input_file"' EXIT HUP INT TERM
cat >"$hook_input_file" 2>/dev/null || true

[ "${HERDR_ENV:-}" = "1" ] || exit 0
[ -n "${HERDR_SOCKET_PATH:-}" ] || exit 0
[ -n "${HERDR_PANE_ID:-}" ] || exit 0
command -v jq >/dev/null 2>&1 || exit 0
command -v python3 >/dev/null 2>&1 || exit 0

vals="$(jq -r '[
  (.hook_event_name // ""),
  (.permission_mode // ""),
  (if .agent_id then "1" else "0" end),
  (.cwd // "")
] | @tsv' "$hook_input_file" 2>/dev/null)" || exit 0
tab="$(printf '\t')"
IFS="$tab" read -r event mode subagent cwd <<EOF
$vals
EOF

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
    # Stop は ttl のリフレッシュを兼ねる。mode が来ないイベント形なら送らない。
    [ -n "$mode" ] || exit 0
    ;;
  UserPromptSubmit | PreToolUse)
    [ -n "$mode" ] || exit 0
    last="$(cat "$state_file" 2>/dev/null || true)"
    [ "$last" = "$mode" ] && exit 0
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

HCM_EVENT="$event" HCM_MODE="$mode" HCM_BRANCH="$branch" HCM_STATE_FILE="$state_file" python3 - <<'PY'
import json
import os
import random
import socket
import time

event = os.environ["HCM_EVENT"]
mode = os.environ["HCM_MODE"]
branch = os.environ.get("HCM_BRANCH") or None
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
if event != "SessionEnd":
    if mode in LABELS:
        name, label = LABELS[mode]
    else:
        # 未知のモード(auto / dontAsk など)は default 用トークンに実名で流す —
        # 古い表示を残すよりは実名表示のほうが正直。
        name, label = "mode_default", f"◆ {mode}"
    tokens[name] = label

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
