#!/usr/bin/env bash
# hook-router-canary.sh — 配備された hook-router の canary(ADR-1529 の段3)。
#
# home/modules/agents.nix の home.checks が、generation の build 時にこれを、配備後のレイアウト
# (`<home-files>/.claude/hooks/hook-router`)に対して走らせる。fixture(cases.jsonl の 1 行 1 件:
# 入力の JSON と、期待する判定 deny|ask|pass)を、登録と同じ引数(`pre|post`、Codex は `--agent codex`)で
# 流し、判定を比べる。食い違えば generation が build できない。CI は vega・arcturus・altair の 3 構成を
# build するので、「この hook が拒否するか」を、機体ごとの実機確認なしに全機体で確かめられる
# (確認の台帳に積まず、ここに fixture を 1 行足す。pr-description スキル §6 の手順 6)。
#
# 規則の判定そのものは cargo test が持つ。ここが確かめるのは、配備される binary と引数の組が、
# その判定を返すこと。設定ファイルや状態を読む規則(sandbox-compound-guard など)は、build の中では
# 判定が決まらないので fixture にしない。HOME は空の一時ディレクトリにする。
#
# 使い方: hook-router-canary.sh <hook-router の実体> <cases.jsonl>
set -euo pipefail

R="${1:?usage: hook-router-canary.sh <hook-router> <cases.jsonl>}"
C="${2:?usage: hook-router-canary.sh <hook-router> <cases.jsonl>}"
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT

fail=0
n=0
while IFS= read -r line; do
  [ -z "$line" ] && continue
  label="$(jq -r '.label' <<<"$line")"
  event="$(jq -r '.event' <<<"$line")"
  agent="$(jq -r '.agent // "claude"' <<<"$line")"
  expect="$(jq -r '.expect' <<<"$line")"
  args=("$event")
  if [ "$agent" = codex ]; then
    args+=(--agent codex)
  fi
  raw="$(jq -c '.input' <<<"$line" | env -i PATH="$PATH" HOME="$T" "$R" "${args[@]}" 2>&1 || true)"
  got="$(jq -r '.hookSpecificOutput.permissionDecision // empty' <<<"$raw" 2>/dev/null || true)"
  [ -n "$got" ] || got=pass
  n=$((n + 1))
  if [ "$got" = "$expect" ]; then
    echo "ok   $label -> $got"
  else
    echo "hook-router canary '$label': expected '$expect', got '$got'" >&2
    echo "  raw: $raw" >&2
    fail=1
  fi
done <"$C"

if [ "$n" -eq 0 ]; then
  echo "hook-router canary: $C に fixture が 1 件もありません" >&2
  exit 1
fi
exit "$fail"
