#!/usr/bin/env bash
# codex-plan-review.sh — ExitPlanMode 直前に Codex CLI でプランを自動レビューする hook。
#
# 設計（~/.claude/plans/claude-plan-approve-groovy-clock.md で合意）:
#   - REQUEST_CHANGES → deny（指摘全文を Claude に注入してプラン修正させる）
#   - APPROVE / エラー / タイムアウト / 上限超過 / スキップ → 何も決定しない（exit 0）
#     ＝ 通常の Approve ダイアログに進む。hook がプランを自動承認することは決してない。
#   - セッションあたり最大 2 レビュー。fail-open が原則。
#
# 使い方:
#   hook として:      settings.json の PreToolUse (matcher: ExitPlanMode) から stdin JSON で呼ばれる
#   advisory として:  codex-plan-review.sh --advisory <plan.md> [cwd]  → レビュー全文を stdout に出す
#
# スキップ手段:
#   touch ~/.claude/plan-reviews/skip   または   SKIP_PLAN_REVIEW=1
#
# 既知バグ対策:
#   - codex exec は非 TTY で stdin を待ち続ける → 必ず </dev/null (openai/codex#20919)
#   - ExitPlanMode hook は cwd=~ で走る → stdin JSON の .cwd へ明示 cd (anthropics/claude-code#22343)
set -u

REVIEW_DIR="$HOME/.claude/plan-reviews"
STATE_DIR="$REVIEW_DIR/state"
mkdir -p "$STATE_DIR"

CODEX_BIN="${CODEX_BIN:-codex}"
MAX_REVIEWS="${MAX_PLAN_REVIEWS:-2}"
CODEX_TIMEOUT="${CODEX_PLAN_REVIEW_TIMEOUT:-280}"

REVIEW_PROMPT_TEMPLATE() {
  local plan_file="$1"
  cat <<EOF
あなたはシニアエンジニアとして、AI コーディングエージェント (Claude) が書いた実装プランをレビューする。

プラン本文: ${plan_file} を読むこと。
作業ディレクトリは対象リポジトリである。プランの前提（ファイル・関数・設定の存在、既存パターンとの整合）を read-only で自由に探索して検証してよい。
プランが他のリポジトリやディレクトリ（例: ~/.ghr/github.com/ 配下の別リポジトリ、\$HOME 直下の設定ファイル）を参照している場合は、それらも read-only で探索して検証してよい。ただしプランが参照していない場所の探索に迷い込まないこと。
プランが外部事実（API 仕様・ライブラリのバージョン・公式推奨）に依拠している場合、必要に応じて web 検索で最新性を検証してよい。

レビュー観点: 論理的な欠陥、抜け漏れ、誤った前提、より単純な代替案、リスクの見落とし。
些細なスタイル指摘や好みの問題は挙げないこと。修正を要求するのは実装の成否や安全性に関わる指摘のみ。

出力: 指摘を簡潔な箇条書きで（重要度順、各指摘に根拠）。日本語で書くこと。
各指摘の行頭に必ず次のいずれかのタグを付けること:
- [技術] … リポジトリや公式ドキュメントの証拠で白黒がつく欠陥（誤った前提、存在しないファイル/関数、論理矛盾、既知バグの見落とし）
- [要判断] … プラン作成者では決められず、人間のユーザーの判断・追加情報が必要な事項（要件の欠落、トレードオフの選択、スコープの解釈、運用上の好み）
最終行に必ず次のいずれか一方だけを出力すること:
VERDICT: APPROVE
VERDICT: REQUEST_CHANGES
EOF
}

run_codex_review() { # $1=plan_file $2=workdir → stdout: レビュー全文 / return: 0=成功
  local plan_file="$1" workdir="$2"
  local out
  out="$(mktemp "$REVIEW_DIR/.codex-out.XXXXXX")"
  local prompt
  prompt="$(REVIEW_PROMPT_TEMPLATE "$plan_file")"
  (
    cd "$workdir" 2>/dev/null || cd "$HOME"
    timeout "$CODEX_TIMEOUT" "$CODEX_BIN" --search exec -s read-only --skip-git-repo-check \
      -o "$out" "$prompt" </dev/null >/dev/null 2>&1
  )
  local rc=$?
  if [[ $rc -ne 0 || ! -s "$out" ]]; then
    rm -f "$out"
    return 1
  fi
  cat "$out"
  rm -f "$out"
  return 0
}

# ---------- advisory モード（手動中間レビュー: /codex-plan-review から呼ばれる） ----------
if [[ "${1:-}" == "--advisory" ]]; then
  plan_file="${2:?usage: codex-plan-review.sh --advisory <plan.md> [cwd]}"
  workdir="${3:-$PWD}"
  if review="$(run_codex_review "$plan_file" "$workdir")"; then
    printf '%s\n' "$review"
    exit 0
  else
    echo "codex レビューの実行に失敗しました（タイムアウト・未ログイン・ネットワーク等）。" >&2
    exit 1
  fi
fi

# ---------- hook モード ----------
INPUT="$(cat)"
printf '%s' "$INPUT" > "$REVIEW_DIR/debug-last-input.json"

EVENT="$(jq -r '.hook_event_name // "PreToolUse"' <<<"$INPUT")"
SESSION_ID="$(jq -r '.session_id // "unknown"' <<<"$INPUT")"
CWD="$(jq -r '.cwd // empty' <<<"$INPUT")"
[[ -d "$CWD" ]] || CWD="$HOME"

# 何も決定せず通常フローに進める（必要なら警告を transcript に残す）
pass_through() { # $1=warning message (optional)
  local msg="${1:-}"
  if [[ -n "$msg" ]]; then
    jq -n --arg m "$msg" '{systemMessage: $m}'
  fi
  exit 0
}

deny_with() { # $1=reason (Claude に届く)
  local reason="$1"
  if [[ "$EVENT" == "PermissionRequest" ]]; then
    jq -n --arg m "$reason" \
      '{hookSpecificOutput: {hookEventName: "PermissionRequest", decision: {behavior: "deny", message: $m}}}'
  else
    jq -n --arg m "$reason" \
      '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $m}}'
  fi
  exit 0
}

# --- codex 不在のマシンでは黙って素通り（ADR-0005: バイナリ存在でゲート） ---
if ! command -v "$CODEX_BIN" >/dev/null 2>&1; then
  pass_through
fi

# --- エスケープハッチ ---
if [[ -e "$REVIEW_DIR/skip" || "${SKIP_PLAN_REVIEW:-0}" == "1" ]]; then
  pass_through
fi

# --- ラウンド上限（セッションあたり MAX_REVIEWS 回まで） ---
COUNT_FILE="$STATE_DIR/${SESSION_ID}.count"
count="$(cat "$COUNT_FILE" 2>/dev/null || echo 0)"
if [[ "$count" -ge "$MAX_REVIEWS" ]]; then
  pass_through "Codex プランレビュー: このセッションの上限 (${MAX_REVIEWS} 回) に達したため素通しします。直近のレビューは $REVIEW_DIR/ を参照。"
fi

# --- プラン本文の取得: tool_input.plan → 最新の ~/.claude/plans/*.md ---
plan_text="$(jq -r '.tool_input.plan // empty' <<<"$INPUT")"
plan_file="$(mktemp "$REVIEW_DIR/.plan.XXXXXX.md")"
if [[ -n "$plan_text" ]]; then
  printf '%s\n' "$plan_text" > "$plan_file"
else
  latest_plan="$(ls -t "$HOME/.claude/plans/"*.md 2>/dev/null | head -1)"
  if [[ -z "$latest_plan" ]]; then
    rm -f "$plan_file"
    pass_through "Codex プランレビュー: プラン本文を取得できなかったためスキップしました（tool_input.plan なし、~/.claude/plans/ も空）。"
  fi
  cp "$latest_plan" "$plan_file"
fi

echo $((count + 1)) > "$COUNT_FILE"

# --- レビュー実行 ---
if ! review="$(run_codex_review "$plan_file" "$CWD")"; then
  rm -f "$plan_file"
  pass_through "Codex プランレビュー: 実行に失敗しました（タイムアウト ${CODEX_TIMEOUT}s・未ログイン・ネットワーク等）。fail-open で通過させます。"
fi
rm -f "$plan_file"

# --- 保存 ---
ts="$(date +%Y%m%d-%H%M%S)"
review_log="$REVIEW_DIR/${ts}-${SESSION_ID:0:8}.md"
printf '%s\n' "$review" > "$review_log"

# --- 判定 ---
verdict="$(grep -oE 'VERDICT: *(APPROVE|REQUEST_CHANGES)' <<<"$review" | tail -1)"
case "$verdict" in
  *REQUEST_CHANGES*)
    deny_with "Codex によるプランレビューの結果、修正が要求されました（ラウンド $((count + 1))/${MAX_REVIEWS}）。指摘は種別に応じて次のように扱うこと:

- [技術] タグの指摘: リポジトリ等の証拠で検証し、妥当なら反映、誤りなら反証の根拠をプランに明記してよい（自律対応可）。
- [要判断] タグの指摘、および追加情報・要件確認を求める指摘: 勝手に採否を判断してプランに反映してはならない。レビュアーの意見はユーザーの決定ではない。必ず AskUserQuestion でユーザーに論点と選択肢（あなたの推奨付き）を提示し、回答を得てからプランを修正すること。
- タグのない指摘は [要判断] として扱うこと。

対応が済んでから再度 ExitPlanMode を呼ぶこと。

--- Codex レビュー ---
$review"
    ;;
  *APPROVE*)
    pass_through "Codex プランレビュー: APPROVE（ラウンド $((count + 1))/${MAX_REVIEWS}）。全文: $review_log"
    ;;
  *)
    pass_through "Codex プランレビュー: 判定行 (VERDICT:) を検出できませんでした。fail-open で通過させます。全文: $review_log"
    ;;
esac
