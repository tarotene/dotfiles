#!/usr/bin/env bash
# apply-rulesets.sh — apply dotfiles' own canonical GitHub rulesets
# (rulesets/{security,quality,workflow}.json at repo root) to this
# repository, in reconcile mode by default (#349).
#
# 設計と根拠: docs/github-audit.md「rulesets」節、ADR-0021(core/review 層
# 分離)。この script は `config/claude/skills/{rust,typst,astro}-repo-
# governance/scripts/apply-rulesets.sh` とは別物 — あちらは「他リポジトリに
# 播く配布テンプレート」(プレースホルダ置換・MSRV/crate 名などを持つ)、
# こちらは「dotfiles 自身に適用する、プレースホルダの無い具体値」。dotfiles
# は Shell/Nix リポジトリで 3 skill のどの言語にも属さないため、いずれか
# 1 つを恣意的に流用するより専用の薄い script の方が素直(#388 の三重化
# 還元とは別軸 — 還元してもこの 4 つ目の適用先の性質は変わらない)。
#
# create-only(既存 ruleset を名前一致で skip)しか出来なかった先行実装の
# 限界(D4、docs/adr/0035-selection-grounding.md の 3 軸で見ると govern-
# ance skill 側は表現不可能に届いていない)を、reconcile モードで埋める:
# 名前一致なら POST でなく PUT で上書きし、正本(security/quality/workflow)
# に無い active な branch ruleset(dotfiles の実例では legacy 名
# "Ephemeral Initial")は削除せず**報告のみ**する(誤って必要な設定を
# 消さないよう、削除は常に人間の判断に残す)。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

# 既定の解決先は 3 段(#417): 明示指定 > checkout 相対 > 配備先。
# home-manager 配備先(~/.local/bin/apply-rulesets.sh)には
# $REPO_ROOT/rulesets が存在しない(SCRIPT_DIR の親は ~/.local であって
# リポジトリではない)ため、それをそのまま使うと PATH 経由の実行が必ず
# 失敗する(#417)。checkout 内 checkout で動かしたときの挙動は変えない
# (REPO_ROOT/rulesets が実在すればそちらを優先する)。
if [[ -n "${APPLY_RULESETS_DIR:-}" ]]; then
  RULESETS_DIR="$APPLY_RULESETS_DIR"
elif [[ -d "$REPO_ROOT/rulesets" ]]; then
  RULESETS_DIR="$REPO_ROOT/rulesets"
else
  RULESETS_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/rulesets"
fi

OWNER="tarotene"
REPO="dotfiles"
DRY_RUN=false
RECONCILE=false

usage() {
  cat <<'EOF'
usage: apply-rulesets.sh [--owner O] [--repo R] [--rulesets-dir DIR]
                          [--reconcile] [--dry-run]

Applies rulesets/{security,quality,workflow}.json to a GitHub repository.
Source directory resolves in this order: --rulesets-dir / APPLY_RULESETS_DIR
env override > <repo root>/rulesets when run from a checkout > the
home-manager deployed location, ${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/
rulesets (#417).

Without --reconcile: create-only — skips any ruleset whose name already
exists (same behavior as the *-repo-governance skills' apply-rulesets.sh).

With --reconcile: PUT (update) instead of skipping when a ruleset with a
canonical name already exists. Any OTHER active branch ruleset not named
Security/Quality/Workflow is reported (never deleted — that stays a human
decision) so a legacy ruleset (e.g. a renamed default "Ephemeral Initial")
is visible instead of silently coexisting.
EOF
}

main() {
while [[ $# -gt 0 ]]; do
  case "$1" in
    --owner) OWNER="$2"; shift 2 ;;
    --repo) REPO="$2"; shift 2 ;;
    --rulesets-dir) RULESETS_DIR="$2"; shift 2 ;;
    --reconcile) RECONCILE=true; shift ;;
    --dry-run) DRY_RUN=true; shift ;;
    --help | -h) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

for cmd in gh jq; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "ERROR: '$cmd' not found" >&2; exit 1; }
done

CANONICAL_FILES=("$RULESETS_DIR/security.json" "$RULESETS_DIR/quality.json" "$RULESETS_DIR/workflow.json")
CANONICAL_NAMES=()
for f in "${CANONICAL_FILES[@]}"; do
  [[ -r "$f" ]] || { echo "ERROR: missing $f" >&2; exit 1; }
  jq -e . "$f" >/dev/null 2>&1 || { echo "ERROR: invalid JSON in $f" >&2; exit 1; }
  CANONICAL_NAMES+=("$(jq -r '.name' "$f")")
done

echo "Applying rulesets to: $OWNER/$REPO (source: $RULESETS_DIR, reconcile=$RECONCILE)"
echo ""

EXISTING="$(gh api "repos/$OWNER/$REPO/rulesets" 2>/dev/null || echo '[]')"

for f in "${CANONICAL_FILES[@]}"; do
  name="$(jq -r '.name' "$f")"
  body="$(cat "$f")"
  existing_id="$(jq -r --arg n "$name" '.[] | select(.name == $n) | .id' <<<"$EXISTING" | head -1)"

  if [[ -z "$existing_id" ]]; then
    if [[ "$DRY_RUN" == "true" ]]; then
      echo "  DRY-RUN: would POST ruleset '$name'"
    else
      result="$(echo "$body" | gh api -X POST "repos/$OWNER/$REPO/rulesets" --input -)"
      id="$(jq -r '.id' <<<"$result")"
      echo "  ✓  Created '$name' (id=$id)"
    fi
    continue
  fi

  if [[ "$RECONCILE" != "true" ]]; then
    echo "  ⚠   '$name' already exists (id=$existing_id) — skipping (pass --reconcile to update)."
    continue
  fi

  if [[ "$DRY_RUN" == "true" ]]; then
    echo "  DRY-RUN: would PUT '$name' (id=$existing_id)"
  else
    echo "$body" | gh api -X PUT "repos/$OWNER/$REPO/rulesets/$existing_id" --input - >/dev/null
    echo "  ✓  Reconciled '$name' (id=$existing_id)"
  fi
done

# 正本に無い active な branch ruleset を報告する(削除はしない)。
names_json="$(printf '%s\n' "${CANONICAL_NAMES[@]}" | jq -R . | jq -s .)"
stray="$(jq --argjson canon "$names_json" \
  '[.[] | select(.target == "branch" and (.name as $n | $canon | index($n) | not)) | {id, name}]' \
  <<<"$EXISTING")"
stray_count="$(jq 'length' <<<"$stray")"
if [[ "$stray_count" -gt 0 ]]; then
  echo ""
  echo "NOTE: ${stray_count} active branch ruleset(s) not in the canonical set (Security/Quality/Workflow):"
  jq -r '.[] | "  - \(.name) (id=\(.id))"' <<<"$stray"
  echo "  These are reported, not deleted — remove manually once their rules are confirmed migrated."
fi
return 0
}

# ---------------------------------------------------------------------------
# selftest — stubs gh (no network), mirrors github-rulesets-apply's own
# selftest pattern (records invocations to a log, returns canned JSON).
# ---------------------------------------------------------------------------

selftest() {
  local fails=0 tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN

  mkdir -p "$tmp/rulesets" "$tmp/bin"
  cp "$REPO_ROOT/rulesets/security.json" "$tmp/rulesets/security.json"
  cp "$REPO_ROOT/rulesets/quality.json" "$tmp/rulesets/quality.json"
  cp "$REPO_ROOT/rulesets/workflow.json" "$tmp/rulesets/workflow.json"

  # gh スタブ: GET は $STUB_EXISTING を返す。POST/PUT は呼び出しを
  # $STUB_LOG に記録し、stdin(ruleset body)の name を添える。
  cat >"$tmp/bin/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$1" == "api" && "$2" != "-X" ]]; then
  cat "$STUB_EXISTING"
  exit 0
fi
if [[ "$1" == "api" && "$2" == "-X" ]]; then
  method="$3"
  path="$4"
  body="$(cat)"
  name="$(printf '%s' "$body" | jq -r '.name')"
  printf '%s %s %s\n' "$method" "$path" "$name" >>"$STUB_LOG"
  if [[ "$method" == "POST" ]]; then
    printf '{"id": 999}'
  fi
  exit 0
fi
echo "unexpected gh invocation: $*" >&2
exit 1
STUB
  chmod +x "$tmp/bin/gh"

  export PATH="$tmp/bin:$PATH"
  export STUB_LOG="$tmp/calls.log"
  export STUB_EXISTING="$tmp/existing.json"

  check() { # $1=label $2=期待exit $3=実exit
    if [[ "$2" == "$3" ]]; then
      echo "ok   $1"
    else
      echo "FAIL $1 (expected exit $2, got $3)" >&2
      fails=$((fails + 1))
    fi
  }
  expect_contains() { # $1=label $2=部分文字列
    grep -qF -- "$2" "$STUB_LOG" 2>/dev/null || {
      echo "FAIL($1): ログに含まれない: $2" >&2
      cat "$STUB_LOG" 2>/dev/null >&2 || true
      fails=$((fails + 1))
    }
  }
  expect_not_contains() { # $1=label $2=部分文字列
    grep -qF -- "$2" "$STUB_LOG" 2>/dev/null && {
      echo "FAIL($1): ログに含まれてはいけない: $2" >&2
      fails=$((fails + 1))
    }
    return 0
  }

  # 1) 既存 ruleset ゼロ → 3 件とも POST される(create-only でも reconcile でも同じ)
  echo '[]' >"$STUB_EXISTING"
  : >"$STUB_LOG"
  main --rulesets-dir "$tmp/rulesets" >/dev/null
  expect_contains "1 Security POST" "POST repos/tarotene/dotfiles/rulesets Security"
  expect_contains "1 Quality POST" "POST repos/tarotene/dotfiles/rulesets Quality"
  expect_contains "1 Workflow POST" "POST repos/tarotene/dotfiles/rulesets Workflow"

  # 2) 既存 Quality あり・--reconcile 無し → skip(PUT されない)
  cat >"$STUB_EXISTING" <<'EOF'
[{"id": 1, "name": "Quality", "target": "branch"}]
EOF
  : >"$STUB_LOG"
  main --rulesets-dir "$tmp/rulesets" >/dev/null
  expect_not_contains "2 reconcile 無しは PUT しない" "PUT repos/tarotene/dotfiles/rulesets/1"
  expect_contains "2 Security は POST される" "POST repos/tarotene/dotfiles/rulesets Security"

  # 3) 既存 Quality あり・--reconcile あり → PUT される
  : >"$STUB_LOG"
  main --rulesets-dir "$tmp/rulesets" --reconcile >/dev/null
  expect_contains "3 reconcile ありは PUT する" "PUT repos/tarotene/dotfiles/rulesets/1 Quality"

  # 4) dotfiles の実例(legacy 名 "Ephemeral Initial")→ 削除されず報告される
  cat >"$STUB_EXISTING" <<'EOF'
[{"id": 1, "name": "Ephemeral Initial", "target": "branch"}, {"id": 2, "name": "Quality", "target": "branch"}, {"id": 3, "name": "Workflow", "target": "branch"}]
EOF
  : >"$STUB_LOG"
  out="$(main --rulesets-dir "$tmp/rulesets" --reconcile 2>&1)"
  expect_not_contains "4 DELETE は一度も呼ばれない" "DELETE"
  echo "$out" | grep -qF "Ephemeral Initial" && echo "ok   4 legacy ruleset は報告される" || {
    echo "FAIL 4 legacy ruleset は報告される: $out" >&2
    fails=$((fails + 1))
  }

  # 5) --dry-run は gh に書き込み系(-X)を送らない
  echo '[]' >"$STUB_EXISTING"
  : >"$STUB_LOG"
  main --rulesets-dir "$tmp/rulesets" --dry-run >/dev/null
  check "5 --dry-run はログを残さない" 0 "$([[ ! -s "$STUB_LOG" ]]; echo $?)"

  if ((fails > 0)); then
    echo "selftest: ${fails} 件失敗" >&2
    return 1
  fi
  echo "selftest: OK"
}

case "${1-}" in
  --selftest) selftest ;;
  --help | -h) usage ;;
  *) main "$@" ;;
esac
