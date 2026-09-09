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

set -eu

hook_input_file="$(mktemp "${TMPDIR:-/tmp}/herdr-codex-metadata.XXXXXX")" || exit 0
trap 'rm -f "$hook_input_file"' EXIT HUP INT TERM
cat >"$hook_input_file" 2>/dev/null || true

[ "${HERDR_ENV:-}" = "1" ] || exit 0
[ -n "${HERDR_SOCKET_PATH:-}" ] || exit 0
[ -n "${HERDR_PANE_ID:-}" ] || exit 0
command -v jq >/dev/null 2>&1 || exit 0
command -v python3 >/dev/null 2>&1 || exit 0

vals="$(jq -r '[
  (.hook_event_name // ""),
  (.model // ""),
  (if .agent_id then "1" else "0" end),
  (.cwd // "")
] | @tsv' "$hook_input_file" 2>/dev/null)" || exit 0
tab="$(printf '\t')"
IFS="$tab" read -r event model subagent cwd <<EOF
$vals
EOF

# サブエージェントは親と同じペインで走る — 親の表示を撹乱させない。
[ "$subagent" = "1" ] && exit 0

case "$event" in
  SessionEnd)
    # model/branch は取らない(payload に model が無く、表示も全クリアする)。
    ;;
  SessionStart | UserPromptSubmit | Stop)
    [ -n "$model" ] || exit 0
    ;;
  *)
    exit 0
    ;;
esac

# worktree/ プレフィクスは表示幅節約のため落とす。取得失敗は単に空 —
# herdr 外・非 git cwd でも無害。SessionEnd では取らない(すべて null で送る)。
branch=""
if [ "$event" != "SessionEnd" ] && [ -n "$cwd" ]; then
  branch="$(git -C "$cwd" branch --show-current 2>/dev/null || true)"
  branch="${branch#worktree/}"
fi

HCM_EVENT="$event" HCM_MODEL="$model" HCM_BRANCH="$branch" python3 - <<'PY'
import json
import os
import random
import socket
import time

event = os.environ["HCM_EVENT"]
model = os.environ.get("HCM_MODEL") or None
branch = os.environ.get("HCM_BRANCH") or None
pane_id = os.environ["HERDR_PANE_ID"]
socket_path = os.environ["HERDR_SOCKET_PATH"]

params = {
    "pane_id": pane_id,
    "source": "codex-hook",
    "seq": time.time_ns(),
    "tokens": {"model": model, "branch": branch},
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
