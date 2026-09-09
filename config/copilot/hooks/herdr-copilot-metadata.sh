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

set -eu

action="${1:-}"
hook_input_file="$(mktemp "${TMPDIR:-/tmp}/herdr-copilot-metadata.XXXXXX")" || exit 0
trap 'rm -f "$hook_input_file"' EXIT HUP INT TERM
cat >"$hook_input_file" 2>/dev/null || true

case "$action" in
  report | clear) ;;
  *) exit 0 ;;
esac

[ "${HERDR_ENV:-}" = "1" ] || exit 0
[ -n "${HERDR_SOCKET_PATH:-}" ] || exit 0
[ -n "${HERDR_PANE_ID:-}" ] || exit 0
command -v jq >/dev/null 2>&1 || exit 0
command -v python3 >/dev/null 2>&1 || exit 0

model=""
branch=""
if [ "$action" = "report" ]; then
  cwd="$(jq -r '.cwd // ""' "$hook_input_file" 2>/dev/null)" || cwd=""
  # worktree/ プレフィクスは表示幅節約のため落とす。取得失敗は単に空 —
  # herdr 外・非 git cwd でも無害。
  if [ -n "$cwd" ]; then
    branch="$(git -C "$cwd" branch --show-current 2>/dev/null || true)"
    branch="${branch#worktree/}"
  fi
  settings="${HOME:-}/.copilot/settings.json"
  if [ -f "$settings" ]; then
    model="$(jq -r '.model // ""' "$settings" 2>/dev/null)" || model=""
  fi
fi

HCM_MODEL="$model" HCM_BRANCH="$branch" HCM_ACTION="$action" python3 - <<'PY'
import json
import os
import random
import socket
import time

action = os.environ["HCM_ACTION"]
model = os.environ.get("HCM_MODEL") or None
branch = os.environ.get("HCM_BRANCH") or None
pane_id = os.environ["HERDR_PANE_ID"]
socket_path = os.environ["HERDR_SOCKET_PATH"]

params = {
    "pane_id": pane_id,
    "source": "copilot-hook",
    "seq": time.time_ns(),
    "tokens": {"model": model, "branch": branch},
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
