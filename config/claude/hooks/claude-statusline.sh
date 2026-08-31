#!/bin/sh
# claude-statusline — ペイン内のかわいい 1 行表示 + Herdr へのメトリクス横流し。
#
# Claude Code の statusline スクリプト。stdin の JSON からモデル・context 使用率・
# セッションコスト・effort を取り、Dracula パステルの 1 行を stdout に出す。
# 表示例: ✨ Fable 5 · 🧠 42% · 💰 $1.23 · ⚡ high
#
# 同じ値を Herdr の pane.report_metadata($model/$ctx/$cost/$effort トークン)にも
# 報告する — permission mode は statusline JSON に来ないので、そちらは
# herdr-claude-metadata.sh(hook)が担う 2 チャネル構成。報告は表示をブロック
# しないよう detach したサブシェルで行い、値が変わらない・前回送信から 2 秒未満の
# 間は送らない(statusline はストリーミング中 ~300ms 毎に再実行されるため)。
# Herdr 外では表示だけが動く。詳細は docs/claude/herdr-sidebar-metadata.md。

set -eu

input_file="$(mktemp "${TMPDIR:-/tmp}/claude-statusline.XXXXXX")" || exit 0
trap 'rm -f "$input_file"' EXIT HUP INT TERM
cat >"$input_file" 2>/dev/null || true

command -v jq >/dev/null 2>&1 || exit 0

vals="$(jq -r '[
  (.model.display_name // .model.id // ""),
  (.context_window.used_percentage // null | if . == null then "" else (round | tostring) end),
  (.cost.total_cost_usd // null | if . == null then "" else tostring end),
  (.effort.level // ""),
  (if .fast_mode == true then "1" else "0" end)
] | @tsv' "$input_file" 2>/dev/null)" || exit 0
tab="$(printf '\t')"
IFS="$tab" read -r model ctx cost effort fast <<EOF
$vals
EOF

esc="$(printf '\033')"
pink="${esc}[1;38;2;255;121;198m"    # model
green="${esc}[38;2;80;250;123m"      # ctx < 60%
yellow="${esc}[38;2;241;250;140m"    # ctx 60-79% / fast
red="${esc}[38;2;255;85;85m"         # ctx >= 80%
cyan="${esc}[38;2;139;233;253m"      # cost
purple="${esc}[38;2;189;147;249m"    # effort
comment="${esc}[38;2;98;114;164m"    # separators
reset="${esc}[0m"
sep=" ${comment}·${reset} "

ctx_color="$green"
if [ -n "$ctx" ]; then
  if [ "$ctx" -ge 80 ]; then
    ctx_color="$red"
  elif [ "$ctx" -ge 60 ]; then
    ctx_color="$yellow"
  fi
fi

line=""
append() {
  if [ -z "$line" ]; then line="$1"; else line="${line}${sep}$1"; fi
}

[ -n "$model" ] && append "✨ ${pink}${model}${reset}"
[ -n "$ctx" ] && append "🧠 ${ctx_color}${ctx}%${reset}"

# 狭いペインではモデルと context だけに畳む。
cols="${COLUMNS:-80}"
case "$cols" in '' | *[!0-9]*) cols=80 ;; esac
if [ "$cols" -ge 60 ]; then
  if [ -n "$cost" ]; then
    cost_fmt="$(LC_ALL=C printf '%.2f' "$cost" 2>/dev/null || printf '%s' "$cost")"
    append "💰 ${cyan}\$${cost_fmt}${reset}"
  fi
  if [ -n "$effort" ]; then
    if [ "$fast" = "1" ]; then
      append "⚡ ${purple}${effort}${reset} ${yellow}fast${reset}"
    else
      append "⚡ ${purple}${effort}${reset}"
    fi
  fi
fi

printf '%s\n' "$line"

# ---- ここから Herdr への報告(Herdr 外では何もしない) ----
[ "${HERDR_ENV:-}" = "1" ] || exit 0
[ -n "${HERDR_SOCKET_PATH:-}" ] || exit 0
[ -n "${HERDR_PANE_ID:-}" ] || exit 0
command -v python3 >/dev/null 2>&1 || exit 0

state_file="${XDG_RUNTIME_DIR:-/tmp}/herdr-claude-status.$(printf '%s' "$HERDR_PANE_ID" | tr -c 'A-Za-z0-9_-' '_')"
cost_token=""
[ -n "$cost" ] && cost_token="\$$(LC_ALL=C printf '%.2f' "$cost" 2>/dev/null || printf '%s' "$cost")"
ctx_token=""
[ -n "$ctx" ] && ctx_token="${ctx}%"

(
  HCS_MODEL="$model" HCS_CTX="$ctx_token" HCS_COST="$cost_token" HCS_EFFORT="$effort" \
    HCS_STATE_FILE="$state_file" python3 - <<'PY'
import json
import os
import random
import socket
import time

state_file = os.environ["HCS_STATE_FILE"]
tokens = {
    "model": os.environ["HCS_MODEL"] or None,
    "ctx": os.environ["HCS_CTX"] or None,
    "cost": os.environ["HCS_COST"] or None,
    "effort": os.environ["HCS_EFFORT"] or None,
}

fingerprint = json.dumps(tokens, sort_keys=True)
try:
    stat = os.stat(state_file)
    with open(state_file, encoding="utf-8") as handle:
        previous = handle.read()
    # 値が同じ、または前回送信から 2 秒未満なら送らない。
    if previous == fingerprint or time.time() - stat.st_mtime < 2.0:
        raise SystemExit(0)
except FileNotFoundError:
    pass

request = {
    "id": f"claude-statusline:{int(time.time() * 1000)}:{random.randrange(1_000_000):06d}",
    "method": "pane.report_metadata",
    "params": {
        "pane_id": os.environ["HERDR_PANE_ID"],
        "source": "claude-statusline",
        "seq": time.time_ns(),
        "tokens": tokens,
        "ttl_ms": 14_400_000,
    },
}

try:
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.settimeout(0.5)
    client.connect(os.environ["HERDR_SOCKET_PATH"])
    client.sendall((json.dumps(request) + "\n").encode())
    try:
        client.recv(4096)
    except Exception:
        pass
    client.close()
    with open(state_file, "w", encoding="utf-8") as handle:
        handle.write(fingerprint)
except Exception:
    pass
PY
) >/dev/null 2>&1 &

exit 0
