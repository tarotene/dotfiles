#!/usr/bin/env bash
# copilot-plan-review.sh — ExitPlanMode 直前に GitHub Copilot CLI でプランを
# 自動レビューする hook。
#
# 設計と根拠: docs/claude/copilot-plan-review.md（このリポジトリ内。旧 Codex 版の
# 設計経緯も同ファイルに履歴として残す）
#
# 停止条件は「critic が黙ること」ではなく「実装をブロックする defect がゼロであること」。
# critic / judge / acceptance oracle を分離する:
#
#   critic : copilot -p --agent plan-reviewer --silent  … findings と carry-over
#            判定を JSON で出すだけ。Copilot CLI には Codex の `exec --output-schema`
#            に相当する強制出力スキーマ機構が無いため、契約は
#            copilot-plan-review.schema.json に文書として残しつつ、judge の直前で
#            このスクリプトの jq validator が構造(top-level keys 完全一致・readiness
#            の bool・findings/carryover の必須キー・型・enum・additionalProperties
#            禁止)を厳格検証する。不適合は critic failure として扱う(fail-open)。
#   judge  : このスクリプト + jq         … gate 適格性を決定論的に判定し open set を更新
#   oracle : gate = open set が非空
#
# critic は VERDICT を出さない。verdict は judge が計算する。
#
#   - open set が非空 → deny（open set の指摘だけを Claude に注入してプラン修正させる）
#   - open set が空 / エラー / タイムアウト / スキーマ不適合 / スキップ
#       → 何も決定しない（exit 0）＝ 通常の Approve ダイアログに進む
#   - hook がプランを自動承認することは決してない（allow を返さない）
#   - セッションあたり最大 MAX_PLAN_REVIEWS (既定 3) レビュー。fail-open が原則。
#
# 最終ラウンド (round == MAX_PLAN_REVIEWS) は「発見」ではなく closer である。
# lens Z が carry-over 判定だけを行い、新規 finding は severity に関わらず gate 対象外
# （backlog 行き）になる。これがないと最終ラウンドで生えた指摘を判定するラウンドが
# 存在せず、open set = ∅ が原理的に到達不能になる（docs/claude/copilot-plan-review.md の
# 「第二次の非収束」）。不変条件は:
#
#   gate 適格な finding は、修正後の再判定を最低 1 回受ける
#
#   - closer 後も open set が残っていたら、deny を 1 回だけ返して人間の GO/NO-GO を
#     強制する（state/<sid>.escalated で 1 回に限定）。以後そのセッションは素通る。
#
# critic の read-only 境界: `--agent plan-reviewer` (config/copilot/agents/
# plan-reviewer.agent.md, ~/.copilot/agents/ に配備) が tools を view/grep/glob
# だけに絞る。write/execute/web/GitHub MCP は与えない。非対話実行は
# --silent（最終応答だけを stdout に出す）+ --no-custom-instructions（リポジトリの
# AGENTS.md 等を読ませない）+ --disable-builtin-mcps（GitHub MCP 等を無効化）+
# --no-ask-user（ask_user tool を無効化し、質問で停止させない）で行う。
# --allow-all-tools / --yolo / --allow-all-paths / --allow-all-urls は critic に
# 変更権限・無制限ファイルアクセス・外部送信経路を与えるため使わない。
# plan file がリポジトリ外にある場合は、その直接の親ディレクトリだけを
# --add-dir で追加読み取り許可する。
#
# 使い方:
#   hook として:      settings.json の PreToolUse (matcher: ExitPlanMode) から stdin JSON で呼ばれる
#   advisory として:  copilot-plan-review.sh --advisory <plan.md> [cwd]  → レビュー全文を stdout に出す
#   自己検査:         copilot-plan-review.sh --selftest                  → judge と hook 経路の回帰テスト
#
# スキップ手段:
#   touch ~/.claude/plan-reviews/skip   または   SKIP_PLAN_REVIEW=1
#
# 既知バグ対策:
#   - copilot -p は非 TTY で stdin を待ち続ける可能性があるため必ず </dev/null にする。
#   - ExitPlanMode hook は cwd=~ で走る → stdin JSON の .cwd へ明示 cd (anthropics/claude-code#22343)
#   - 認証切れ・モデル利用不可・タイムアウトはすべて fail-open（critic failure と
#     同じ扱い）。ゲートを止めない代わりに warn として transcript に残す。
set -u

# 生成物（レビュー本文・生 JSON・backlog・state）はプラン本文由来のテキストを含むため、
# 既定 umask に任せず 0600 / 0700 に落とす。
umask 077

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"

REVIEW_DIR="${COPILOT_PLAN_REVIEW_DIR:-$HOME/.claude/plan-reviews}"
STATE_DIR="$REVIEW_DIR/state"
BACKLOG_DIR="$REVIEW_DIR/backlog"
# schema.json は CLI に渡す強制出力スキーマではなく契約文書。実際の検証は
# CRITIC_SCHEMA_JQ(このスクリプト内の jq validator)がハードコードして行う。
SCHEMA_FILE="${COPILOT_PLAN_REVIEW_SCHEMA:-$SCRIPT_DIR/copilot-plan-review.schema.json}"

COPILOT_BIN="${COPILOT_BIN:-copilot}"
MAX_REVIEWS="${MAX_PLAN_REVIEWS:-3}"
COPILOT_TIMEOUT="${COPILOT_PLAN_REVIEW_TIMEOUT:-280}"
GATE_SEVERITIES="${COPILOT_PLAN_REVIEW_GATE_SEVERITIES:-BLOCKER,MAJOR}"
PARALLEL="${COPILOT_PLAN_REVIEW_PARALLEL:-1}"
RETENTION_DAYS="${COPILOT_PLAN_REVIEW_RETENTION_DAYS:-30}"
# 調査時点で利用可能な最新 GPT の具体 ID に固定する。model catalog の変更で
# 利用不可になった場合も fail-open するが、ログ警告で model failure を識別できる
# ようにする(docs/claude/copilot-plan-review.md)。
COPILOT_MODEL="${COPILOT_PLAN_REVIEW_MODEL:-gpt-5.6-sol}"
# read-only custom agent。ファイル名(拡張子抜き)がそのまま --agent の値になる。
COPILOT_AGENT="${COPILOT_PLAN_REVIEW_AGENT:-plan-reviewer}"

ensure_dirs() {
  mkdir -p "$STATE_DIR" "$BACKLOG_DIR"
  chmod 700 "$REVIEW_DIR" "$STATE_DIR" "$BACKLOG_DIR" 2>/dev/null || true
  # 既存ファイルが緩い権限で残っていたら締める（skip は残置してよい空ファイル）
  find "$REVIEW_DIR" -maxdepth 2 -type f ! -name skip -perm /077 \
    -exec chmod 600 {} + 2>/dev/null || true
}

# 保持期限より古い log / json / backlog / state を掃除する。
# skip はエスケープハッチなので絶対に消さない（消すとゲートが黙って復活する）。
prune_old() {
  case "$RETENTION_DAYS" in
    '' | 0 | *[!0-9]*) return 0 ;;
  esac
  find "$REVIEW_DIR" -maxdepth 2 -type f ! -name skip -mtime "+$RETENTION_DAYS" \
    -delete 2>/dev/null || true
}

# ---------------------------------------------------------------------------
# レビュープロンプト
# ---------------------------------------------------------------------------

# 全 lens 共通の前置き。$1=plan_file
prompt_preamble() {
  local plan_file="$1"
  cat <<EOF
あなたはシニアエンジニアとして、AI コーディングエージェント (Claude) が書いた実装プランをレビューする。

プラン本文: ${plan_file} を読むこと。
作業ディレクトリは対象リポジトリである。プランの前提（ファイル・関数・設定の存在、既存パターンとの整合）を read-only で自由に探索して検証してよい。
プランが他のリポジトリやディレクトリ（例: ~/.ghr/github.com/ 配下の別リポジトリ、\$HOME 直下の設定ファイル）を参照している場合は、それらも read-only で探索して検証してよい。ただしプランが参照していない場所の探索に迷い込まないこと。
ネットワークアクセスはできない。外部事実（API 仕様・ライブラリのバージョン・公式推奨）は、プランまたはリポジトリに記録された根拠だけで判定し、追加の web 検索は行わないこと。

## 受入基準

このレビューの目的は「完璧なプラン」を作ることではなく、プランが実装可能な状態
(implementation-ready) に達しているかを判定することである。

  Ready(P) = R かつ S かつ I かつ T
    R (requirements)    要件が特定されている
    S (scope)           スコープ / 非スコープが明確
    I (implementation)  実装手順が具体化されている
    T (verification)    検証方法が定義されている

「もっと良いプランが存在する」ことは指摘理由にならない。

## 報告してよいもの / いけないもの

報告するのは、実装をブロックする問題だけである。

報告してはいけない:
  - 同等な代替設計（どちらでも成立する設計上の選択）
  - 将来の拡張案・任意の改善提案
  - スタイル・命名・体裁の好み
  - 「〜した方がよい」で終わり、具体的な失敗を示せないもの

**findings が空配列であることは正常かつ望ましい結果である。**
指摘を捻り出す必要はない。ブロックすべき問題がなければ空配列を返せ。

## severity

  BLOCKER  このまま実装すると壊れる / 要件を満たさない
  MAJOR    実装前に決めないと手戻りが確実
  MINOR    直した方がよいが実装をブロックしない
  NIT      好み・体裁

BLOCKER と MAJOR だけがプラン修正を要求する。確信が持てないものは MINOR に落とせ。

## kind

  TECHNICAL       リポジトリや公式ドキュメントの証拠で白黒がつく欠陥
  NEEDS_DECISION  プラン作成者では決められず、人間のユーザーの判断・追加情報が必要

## 各 finding に必須のフィールド

  summary         一行要約
  failure_mode    具体的にどう失敗するか
  trigger         どういう条件で発生するか
  evidence        リポジトリ上の path[:line] / プランの該当箇所 / 外部仕様の出典
  readiness_axis  その指摘が壊す軸 (REQUIREMENTS / SCOPE / IMPLEMENTATION / VERIFICATION / NONE)

これらを具体的に埋められない指摘は報告しないこと。空文字や「不明」で埋めた BLOCKER /
MAJOR は機械的に破棄される（黙って通過扱いになる）。

## readiness

readiness の 4 boolean を出すこと。false にした軸は必ず BLOCKER または MAJOR の finding で
裏付けること。裏付けのない false は機械的に無視される。

## 出力

最終メッセージは以下の JSON にちょうど一致する 1 オブジェクトだけにすること。
コードフェンス(\`\`\`)・前置き・要約・末尾のコメントは一切出力しない。
キーの過不足・型不一致・enum 外の値は機械的に破棄され、critic failure として
扱われる（黙って通過にはならないが、その critic の指摘は今ラウンドで失われる）。

  {
    "readiness": {
      "requirements": <boolean>, "scope": <boolean>,
      "implementation": <boolean>, "verification": <boolean>
    },
    "findings": [
      { "severity": "BLOCKER"|"MAJOR"|"MINOR"|"NIT",
        "kind": "TECHNICAL"|"NEEDS_DECISION",
        "readiness_axis": "REQUIREMENTS"|"SCOPE"|"IMPLEMENTATION"|"VERIFICATION"|"NONE",
        "summary": <string>, "failure_mode": <string>,
        "trigger": <string>, "evidence": <string> }, ...
    ],
    "carryover": [
      { "id": <string>, "status": "RESOLVED"|"UNRESOLVED"|"REFUTED_BY_PLAN",
        "rationale": <string> }, ...
    ]
  }

上記 3 キー(readiness/findings/carryover)以外は出力しないこと。各オブジェクトも
挙げたキーだけを持ち、余計なキーを足さないこと。
EOF
}

# lens ごとの観点。$1=lens
prompt_lens() {
  case "$1" in
    A)
      cat <<'EOF'

## このレビューの観点 (lens A)

要件 (R) とスコープ (S)、およびプランが置いている前提の誤りだけを見る。

  - プランが満たすべき要件が特定されているか。暗黙の要件を取り違えていないか
  - スコープと非スコープが切れているか。やると書いたことが実際にゴールを達成するか
  - プランが「存在する」と仮定しているファイル・関数・設定・挙動が実在するか
  - 参照している既存パターンや過去の決定と矛盾していないか

実装手順の粒度や検証方法の不足は別の critic が見るので、ここでは扱わない。
EOF
      ;;
    B)
      cat <<'EOF'

## このレビューの観点 (lens B)

実装可能性 (I) と検証戦略 (T) だけを見る。

  - 書かれた手順で実際に実装できるか。決まっていない分岐が残っていないか
  - 手順の順序・依存関係が成立しているか。片方だけ適用すると壊れる箇所はないか
  - 検証方法が定義されているか。それが本当に「できた」ことを判定できるか
  - 検証が通っても壊れたままになる経路が残っていないか

要件やスコープの妥当性は別の critic が見るので、ここでは扱わない。
EOF
      ;;
    C)
      cat <<'EOF'

## このレビューの観点 (lens C)

adversarial に見る。「このプランを実装した結果、何が壊れるか」を探す。

  - 手順どおりに実装したとき、既存の動作を壊す経路はどこか
  - エラー・タイムアウト・並行実行・部分失敗のときにどうなるか
  - プランが導入する新しい状態・ファイル・権限が、既存の前提と衝突しないか
  - プランが自分で塞いだつもりの穴が、実際には塞がっていない箇所はないか

新しい観点を無理に増やす必要はない。ブロックすべき問題がなければ findings は空配列でよい。
EOF
      ;;
    Z)
      cat <<'EOF'

## このレビューの観点 (lens Z — closer)

これはこのセッションの最終ラウンドである。あなたの職責は **carry-over の判定だけ** で
あり、それ以外にはない。

  - 新しい欠陥を探すな。プランを読み直して別の問題を見つけ出す作業はしない
  - 下の carry-over 節にある各 id について、改訂プランを読んで判定を返すことに専念せよ
  - **findings は空配列を返せ。** ここで報告した新規指摘は severity に関わらず
    gate 対象にならず backlog に退避されるだけなので、探す労力に見合わない

carry-over の判定精度がこのラウンドの唯一の成果物である。
EOF
      ;;
    *)
      cat <<'EOF'

## このレビューの観点 (lens M)

要件 (R) / スコープ (S) / 実装可能性 (I) / 検証戦略 (T) を一度に見る。
加えて、実装した結果何が壊れるかを adversarial に探す。
EOF
      ;;
  esac
}

# carry-over 節。$1=open set JSON（空配列ならラウンド 1 用の指示を出す）
prompt_carryover() {
  local open_json="$1" n
  n="$(jq 'length' <<<"$open_json" 2>/dev/null || echo 0)"
  if [[ "$n" == "0" ]]; then
    cat <<'EOF'

## carry-over

前ラウンドからの未解決指摘はない。carryover は空配列を返すこと。
EOF
    return 0
  fi
  cat <<'EOF'

## carry-over（前ラウンドからの未解決指摘）

以下は前ラウンドでブロック要因と判定された指摘である。改訂されたプランを読み、
**各 id について carryover に必ず 1 件返すこと**。

  RESOLVED         改訂プランで解消された（rationale にプランのどこで解消されたかを書く）
  REFUTED_BY_PLAN  プランに書かれた反証が妥当で、指摘自体が誤りだった（rationale に理由）
  UNRESOLVED       まだ解消していない

判定を落とした id は保守的に UNRESOLVED として扱われる。落としても通過はしない。
同じ指摘を findings に再掲する必要はない。carryover で UNRESOLVED と答えれば足りる。

判定は **改訂後プランの現行本文** を根拠に述べること。下に添えた根拠は前ラウンド時点の
ものであり、改訂で行番号がずれたり記述そのものが消えていることがある。

  - 行番号の一致で照合するな。記述内容で照合せよ
  - 前ラウンドの根拠が指していた記述が現行プランに存在しないなら、その指摘は
    RESOLVED（書き換えで解消された）または REFUTED_BY_PLAN と判定せよ。
    存在しない記述を根拠に UNRESOLVED を返してはならない

EOF
  jq -r '.[] | "- id: " + .id + " [" + .severity + "] " + .summary
    + "\n  失敗モード: " + (.failure_mode // "")
    + "\n  根拠: " + (.evidence // "")' <<<"$open_json"
}

# $1=lens $2=plan_file $3=open set JSON
build_prompt() {
  prompt_preamble "$2"
  prompt_lens "$1"
  prompt_carryover "$3"
}

# ---------------------------------------------------------------------------
# critic 実行
# ---------------------------------------------------------------------------

# CRITIC_SCHEMA_JQ: copilot-plan-review.schema.json と一致する構造検証。
# Copilot CLI には Codex の `--output-schema` に相当する強制出力スキーマが無い
# ため、この jq validator が唯一の構造ゲートになる。top-level keys の完全一致・
# readiness の bool 4 種・findings/carryover の必須キー・型・enum・
# additionalProperties 相当(想定外キーの禁止)を検証する。コードフェンス付き
# 出力や部分的な JSON は手前の `jq -e '.'` で弾かれる（フェンス込みの文字列は
# それ自体が妥当な JSON ではないため）。
CRITIC_SCHEMA_JQ=$(
  cat <<'JQEOF'
def has_exact_keys(spec):
  type == "object" and ((keys_unsorted | sort) == (spec | sort));
def valid_readiness:
  has_exact_keys(["requirements", "scope", "implementation", "verification"])
  and (.requirements | type == "boolean")
  and (.scope | type == "boolean")
  and (.implementation | type == "boolean")
  and (.verification | type == "boolean");
def valid_finding:
  has_exact_keys(["severity", "kind", "readiness_axis", "summary",
                   "failure_mode", "trigger", "evidence"])
  and (.severity | type == "string" and IN("BLOCKER", "MAJOR", "MINOR", "NIT"))
  and (.kind | type == "string" and IN("TECHNICAL", "NEEDS_DECISION"))
  and (.readiness_axis | type == "string"
       and IN("REQUIREMENTS", "SCOPE", "IMPLEMENTATION", "VERIFICATION", "NONE"))
  and (.summary | type == "string")
  and (.failure_mode | type == "string")
  and (.trigger | type == "string")
  and (.evidence | type == "string");
def valid_carry:
  has_exact_keys(["id", "status", "rationale"])
  and (.id | type == "string")
  and (.status | type == "string" and IN("RESOLVED", "UNRESOLVED", "REFUTED_BY_PLAN"))
  and (.rationale | type == "string");

has_exact_keys(["readiness", "findings", "carryover"])
and (.readiness | valid_readiness)
and (.findings | type == "array") and (.findings | map(valid_finding) | all)
and (.carryover | type == "array") and (.carryover | map(valid_carry) | all)
JQEOF
)

# stdin: critic の生出力ファイルの中身候補(jq でパース済みの JSON 値) →
# 終了コードでスキーマ適合を返す。値そのものは呼び出し側が既に持っている。
validate_critic_schema() {
  jq -e "$CRITIC_SCHEMA_JQ" >/dev/null 2>&1
}

# 1 プロセスだけ copilot を回し、最終メッセージ (JSON) を $4 に書く。
# ログ・state には一切書かない（並列実行時の競合を構造的に避けるため）。
# $1=lens $2=plan_file $3=workdir $4=out_file $5=open set JSON
run_critic() {
  local lens="$1" plan_file="$2" workdir="$3" out="$4" open_json="$5"
  local prompt
  prompt="$(build_prompt "$lens" "$plan_file" "$open_json")"
  (
    cd "$workdir" 2>/dev/null || cd "$HOME" || exit 1
    # plan file がこの workdir の外にある場合だけ、その直接の親ディレクトリを
    # 追加で読み取り許可する（--allow-all-paths は使わない）。cwd 自体は
    # デフォルトで読める。
    local -a add_dir_args=()
    local plan_dir cur_dir
    plan_dir="$(cd -- "$(dirname -- "$plan_file")" >/dev/null 2>&1 && pwd)" || plan_dir=""
    cur_dir="$(pwd)"
    if [[ -n "$plan_dir" ]]; then
      case "$plan_dir" in
        "$cur_dir" | "$cur_dir"/*) : ;;
        *) add_dir_args=(--add-dir "$plan_dir") ;;
      esac
    fi
    timeout "$COPILOT_TIMEOUT" "$COPILOT_BIN" -p "$prompt" \
      --agent "$COPILOT_AGENT" --model "$COPILOT_MODEL" \
      --silent --no-custom-instructions --disable-builtin-mcps --no-ask-user \
      "${add_dir_args[@]}" </dev/null > "$out" 2>/dev/null
  )
}

# lens 群を（必要なら並列に）回し、結果を次の形で stdout に出す:
#   { "critics": [ {lens: "A", data: {...}}, ... ], "failed": ["B"] }
# コマンド置換のサブシェル越しに失敗 lens を伝えるため、グローバル変数ではなく
# 出力 JSON に載せる。
# $1=plan_file $2=workdir $3=open set JSON $4.. = lens 群
run_critics() {
  local plan_file="$1" workdir="$2" open_json="$3"
  shift 3
  local -a lenses=("$@")
  local -a outs=()
  local -a pids=()
  local -a rcs=()
  local lens out pid i

  for lens in "${lenses[@]}"; do
    out="$(mktemp "$REVIEW_DIR/.copilot-out.XXXXXX")"
    outs+=("$out")
  done

  if [[ "$PARALLEL" == "1" && ${#lenses[@]} -gt 1 ]]; then
    i=0
    for lens in "${lenses[@]}"; do
      run_critic "$lens" "$plan_file" "$workdir" "${outs[$i]}" "$open_json" &
      pids+=("$!")
      i=$((i + 1))
    done
    # 終了コードは捨てない。copilot が stdout に有効な JSON を書いた後で
    # 非ゼロ終了する（タイムアウト・後処理エラー等）ケースを成功扱いしないため。
    for pid in "${pids[@]}"; do
      if wait "$pid"; then rcs+=(0); else rcs+=(1); fi
    done
  else
    i=0
    for lens in "${lenses[@]}"; do
      if run_critic "$lens" "$plan_file" "$workdir" "${outs[$i]}" "$open_json"; then
        rcs+=(0)
      else
        rcs+=(1)
      fi
      i=$((i + 1))
    done
  fi

  local results="[]" failed="[]" data rc
  i=0
  for lens in "${lenses[@]}"; do
    out="${outs[$i]}"
    rc="${rcs[$i]}"
    i=$((i + 1))
    if [[ "$rc" == "0" ]] && [[ -s "$out" ]] && data="$(jq -e '.' "$out" 2>/dev/null)" &&
      validate_critic_schema <<<"$data"; then
      results="$(jq --arg l "$lens" --argjson d "$data" \
        '. + [{lens: $l, data: $d}]' <<<"$results")"
    else
      failed="$(jq --arg l "$lens" '. + [$l]' <<<"$failed")"
    fi
    rm -f "$out"
  done

  jq -n --argjson c "$results" --argjson f "$failed" '{critics: $c, failed: $f}'
}

# ---------------------------------------------------------------------------
# judge（決定論的）
# ---------------------------------------------------------------------------

JUDGE_JQ=$(
  cat <<'JQEOF'
def nonblank: (((. // "") | tostring | gsub("[[:space:]]+"; "")) | length) > 0;
def norm: ((. // "") | tostring | ascii_downcase | gsub("[[:space:]]+"; " ")
           | sub("^ +"; "") | sub(" +$"; ""));
def filled: (.summary | nonblank) and (.failure_mode | nonblank)
            and (.trigger | nonblank) and (.evidence | nonblank);

($gate | split(",") | map(select(length > 0))) as $gates
| ["BLOCKER", "MAJOR", "MINOR", "NIT"] as $valid
| [ .[] as $c | (($c.data.findings // []) | to_entries[]) as $e
    | ($e.value + { _lens: $c.lens, _n: ($e.key + 1) }) ] as $flat_raw
| [ $flat_raw[] | select(.severity | IN($valid[])) ] as $flat
| (($flat_raw | length) - ($flat | length)) as $invalid_severity
| [ $flat[] | select(.severity | IN($gates[])) ] as $gsev
| [ $gsev[] | select(filled) ] as $conforming
| (($gsev | length) - ($conforming | length)) as $nonconforming
| ( reduce $conforming[] as $x ({ seen: {}, out: [] };
      ($x.summary | norm) as $k
      | if (.seen | has($k)) then . else (.seen[$k] = true | .out += [$x]) end)
    | .out ) as $dedup
| (($conforming | length) - ($dedup | length)) as $dup_dropped
| [ $dedup[] | . + { id: ("R" + $round + "-" + ._lens + "-" + (._n | tostring)) } ] as $new_eligible
| [ .[] | (.data.carryover // [])[] ] as $co_raw
| ($open | map(.id)) as $open_ids
| [ $co_raw[] | select(.id | IN($open_ids[])) ] as $co
| (($co_raw | length) - ($co | length)) as $unknown_carryover
| (reduce $co[] as $c ({};
     if has($c.id)
     then .[$c.id] = {
       id: $c.id,
       status: "UNRESOLVED",
       rationale: "critic が同じ id に重複した判定を返した"
     }
     else .[$c.id] = $c
     end)) as $co_map
| [ $open[] | . as $o | ($co_map[$o.id] // null) as $c
    | if ($c == null)
      then ($o + { _carry: "UNRESOLVED", _carry_missing: true, _carry_rationale: "" })
      elif ($c.status == "UNRESOLVED")
      then ($o + { _carry: "UNRESOLVED", _carry_missing: false,
                   _carry_rationale: ($c.rationale // "") })
      else empty end ] as $carried
| [ $open[] | . as $o | ($co_map[$o.id] // null) as $c
    | if ($c != null and ($c.status == "RESOLVED" or $c.status == "REFUTED_BY_PLAN"))
      then ($o + { _carry: $c.status, _carry_rationale: ($c.rationale // "") })
      else empty end ] as $closed
| ([ $carried[] | select(._carry_missing == true) ] | length) as $missing_carryover
| ($carried + $new_eligible) as $newopen
| [ $flat[] | select((.severity | IN($gates[])) | not) ] as $backlog
| [ .[] | .data.readiness | select(. != null) ] as $rs
| ( if ($rs | length) == 0
    then { requirements: true, scope: true, implementation: true, verification: true }
    else reduce $rs[] as $r
      ({ requirements: true, scope: true, implementation: true, verification: true };
       { requirements: (.requirements and ($r.requirements != false)),
         scope: (.scope and ($r.scope != false)),
         implementation: (.implementation and ($r.implementation != false)),
         verification: (.verification and ($r.verification != false)) })
    end ) as $readiness
| ([ $readiness | to_entries[] | select(.value == false) ] | length > 0) as $not_ready
| { gate: (($newopen | length) > 0),
    round: ($round | tonumber),
    lenses: [ .[] | .lens ],
    open: $newopen,
    new_eligible: $new_eligible,
    carried: $carried,
    closed: $closed,
    backlog: $backlog,
    readiness: $readiness,
    warn: { invalid_severity: $invalid_severity,
            nonconforming: $nonconforming,
            dup_dropped: $dup_dropped,
            unknown_carryover: $unknown_carryover,
            missing_carryover: $missing_carryover,
            readiness_inconsistent: ($not_ready and (($newopen | length) == 0)) } }
JQEOF
)

# $1=round $2=open set JSON [$3=gate severities] ; stdin: critics 配列 JSON
# → stdout: judged JSON
#
# $3 の既定値展開は `${3-...}` でなければならない（`${3:-...}` ではない）。
# closer ラウンドは「gate 適格な severity は無い」を空文字で表現するので、空文字が
# 既定値 BLOCKER,MAJOR に置換されると closer が新規流入を止められなくなる。
judge() {
  local gate="${3-$GATE_SEVERITIES}"
  jq --arg round "$1" --arg gate "$gate" --argjson open "$2" "$JUDGE_JQ"
}

# closer ラウンド用に judged JSON を整える。
# gate 適格 severity が空なので readiness の未充足軸は必ず「裏付ける finding なし」に
# なるが、それは critic の不備ではなく closer の仕様である。偽陽性の警告を潰す。
# stdin: judged JSON → stdout: judged JSON
suppress_closer_warn() {
  jq '.warn.readiness_inconsistent = false'
}

# ---------------------------------------------------------------------------
# レンダリング
# ---------------------------------------------------------------------------

RENDER_DEFS=$(
  cat <<'JQEOF'
def block:
  "### [" + (.severity // "?") + "][" + (.kind // "?") + "] " + (.summary // "") + "\n"
  + (if ((.id // "") != "")
     then "- id: " + .id + " / 軸: " + (.readiness_axis // "NONE") + "\n" else "" end)
  + (if ((._carry // "") == "UNRESOLVED")
     then "- **前ラウンドから未解消**"
          + (if (._carry_missing == true)
             then "（critic が判定を返さなかったため保守的に未解消として扱った）"
             else "" end) + "\n"
     else "" end)
  + (if ((._carry // "") == "RESOLVED" or (._carry // "") == "REFUTED_BY_PLAN")
     then "- " + ._carry + ": " + (._carry_rationale // "") + "\n" else "" end)
  + "- 失敗モード: " + (.failure_mode // "") + "\n"
  + "- 発生条件: " + (.trigger // "") + "\n"
  + "- 根拠: " + (.evidence // "") + "\n";
def blocks: if length == 0 then "（なし）\n" else (map(block) | join("\n")) end;
def readiness_line:
  "R=" + (if .requirements then "yes" else "NO" end)
  + " S=" + (if .scope then "yes" else "NO" end)
  + " I=" + (if .implementation then "yes" else "NO" end)
  + " T=" + (if .verification then "yes" else "NO" end);
JQEOF
)

# stdin: judged JSON → stdout: open set のみを並べた markdown（deny メッセージ用）
render_open() {
  jq -r "$RENDER_DEFS"'.open | blocks'
}

# stdin: judged JSON → stdout: レビュー log の markdown
render_log() {
  jq -r "$RENDER_DEFS"'
    "## GATE: " + (if .gate then "DENY" else "PASS" end) + "\n"
    + "\n"
    + "- ラウンド: " + (.round | tostring) + " / lens: " + (.lenses | join(", ")) + "\n"
    + "- readiness: " + (.readiness | readiness_line) + "\n"
    + "- open set: " + (.open | length | tostring) + " 件"
      + " (新規 " + (.new_eligible | length | tostring)
      + " / 未解消 " + (.carried | length | tostring) + ")\n"
    + "- backlog (MINOR/NIT): " + (.backlog | length | tostring) + " 件\n"
    + "- 不適合で破棄: " + (.warn.nonconforming | tostring)
      + " / 重複除去: " + (.warn.dup_dropped | tostring)
      + " / 不正 severity: " + (.warn.invalid_severity | tostring) + "\n"
    + "- carry-over: 未応答 " + (.warn.missing_carryover | tostring)
      + " / 未知 id " + (.warn.unknown_carryover | tostring) + "\n"
    + "\n## open set（ゲート対象）\n\n" + (.open | blocks)
    + "\n## 今ラウンドで解消 / 却下\n\n" + (.closed | blocks)
    + "\n## backlog (MINOR/NIT)\n\n" + (.backlog | blocks)'
}

# stdin: judged JSON → stdout: backlog 追記用 markdown（0 件なら空文字）
render_backlog() {
  jq -r "$RENDER_DEFS"'
    if (.backlog | length) == 0 then "" else
      "## ラウンド " + (.round | tostring) + "\n\n" + (.backlog | blocks)
    end'
}

# stdin: judged JSON → stdout: 警告文（空文字なら警告なし）
warn_text() {
  jq -r '
    [ (if .warn.nonconforming > 0 then
         "必須フィールドが空の BLOCKER/MAJOR " + (.warn.nonconforming | tostring)
         + " 件を破棄しました" else empty end),
      (if .warn.invalid_severity > 0 then
         "未知の severity " + (.warn.invalid_severity | tostring) + " 件を破棄しました"
         else empty end),
      (if .warn.missing_carryover > 0 then
         "carry-over " + (.warn.missing_carryover | tostring)
         + " 件に判定が返らなかったため未解消として扱いました" else empty end),
      (if .warn.unknown_carryover > 0 then
         "open set に無い carry-over id " + (.warn.unknown_carryover | tostring)
         + " 件を無視しました" else empty end),
      (if .warn.readiness_inconsistent then
         "readiness に未充足の軸があるのに、それを裏付ける証拠付き finding がありません（critic 出力の不備として素通しします）"
         else empty end)
    ] | join("。")'
}

# ---------------------------------------------------------------------------
# advisory モード（手動中間レビュー: /copilot-plan-review から呼ばれる）
# ---------------------------------------------------------------------------

if [[ "${1:-}" == "--advisory" ]]; then
  ensure_dirs
  adv_plan="${2:?usage: copilot-plan-review.sh --advisory <plan.md> [cwd]}"
  adv_workdir="${3:-$PWD}"
  if ! command -v "$COPILOT_BIN" >/dev/null 2>&1; then
    echo "copilot が見つかりません (COPILOT_BIN=$COPILOT_BIN)。" >&2
    exit 1
  fi
  if [[ "$PARALLEL" == "1" ]]; then
    adv_lenses=(A B)
  else
    adv_lenses=(M)
  fi
  adv_raw="$(run_critics "$adv_plan" "$adv_workdir" '[]' "${adv_lenses[@]}")"
  adv_critics="$(jq '.critics' <<<"$adv_raw")"
  adv_failed="$(jq -r '.failed | join(", ")' <<<"$adv_raw")"
  if [[ "$(jq 'length' <<<"$adv_critics")" == "0" ]]; then
    echo "copilot レビューの実行に失敗しました（タイムアウト・未ログイン・モデル利用不可・ネットワーク等）。" >&2
    exit 1
  fi
  if ! adv_judged="$(judge 1 '[]' <<<"$adv_critics")"; then
    echo "レビュー結果の解析に失敗しました。" >&2
    exit 1
  fi
  render_log <<<"$adv_judged"
  [[ -n "$adv_failed" ]] &&
    echo "（注意: lens $adv_failed が失敗したため残りの critic のみで判定しています）" >&2
  adv_warn="$(warn_text <<<"$adv_judged")"
  [[ -n "$adv_warn" ]] && echo "（注意: $adv_warn）" >&2
  exit 0
fi

# ---------------------------------------------------------------------------
# selftest（judge と hook 経路の回帰テスト。CI から呼ばれる）
# ---------------------------------------------------------------------------

if [[ "${1:-}" == "--selftest" ]]; then
  selftest_fail=0

  # 隔離(#56 / #58): ホストの home-manager sessionVariables 等が既に export した
  # COPILOT_* / MAX_PLAN_REVIEWS がテストの前提を汚染しないよう、判定に使う
  # グローバルをここで既知値へ固定し直す。これらは script 冒頭で一度だけ
  # 環境変数から読まれているため、後段で env を unset しても手遅れ — この
  # ブロック自身が「真の初期値」を再代入する。
  COPILOT_BIN="true"
  MAX_REVIEWS=3
  COPILOT_TIMEOUT=280
  GATE_SEVERITIES="BLOCKER,MAJOR"
  PARALLEL=1
  RETENTION_DAYS=30
  COPILOT_MODEL="gpt-5.6-sol"
  COPILOT_AGENT="plan-reviewer"
  unset SKIP_PLAN_REVIEW

  selftest_dir="$(mktemp -d)"
  trap 'rm -rf "$selftest_dir"' EXIT
  REVIEW_DIR="$selftest_dir"
  STATE_DIR="$REVIEW_DIR/state"
  BACKLOG_DIR="$REVIEW_DIR/backlog"
  ensure_dirs

  READY_ALL='{"requirements":true,"scope":true,"implementation":true,"verification":true}'

  ok() { printf '  ok   %s\n' "$1"; }
  ng() {
    printf '  FAIL %s\n' "$1"
    selftest_fail=1
  }
  check() { # $1=label $2=expected $3=actual
    if [[ "$2" == "$3" ]]; then ok "$1"; else ng "$1 (expected=$2 actual=$3)"; fi
  }
  mkfinding() { # $1=severity $2=summary [$3=evidence]
    jq -n --arg s "$1" --arg m "$2" --arg e "${3-evidence.md:1}" \
      '{severity: $s, kind: "TECHNICAL", readiness_axis: "IMPLEMENTATION",
        summary: $m, failure_mode: "壊れる", trigger: "常に", evidence: $e}'
  }
  mkcritic() { # $1=lens $2=findings JSON $3=carryover JSON [$4=readiness JSON]
    jq -n --arg l "$1" --argjson f "$2" --argjson c "$3" \
      --argjson r "${4:-$READY_ALL}" \
      '{lens: $l, data: {readiness: $r, findings: $f, carryover: $c}}'
  }

  echo "judge:"

  # 1) BLOCKER 1 件 → deny、open set にその 1 件だけ
  c="$(mkcritic A "[$(mkfinding BLOCKER '存在しない関数を呼んでいる')]" '[]')"
  j="$(judge 1 '[]' <<<"[$c]")"
  check "BLOCKER 1 件 → gate=true" true "$(jq -r '.gate' <<<"$j")"
  check "BLOCKER 1 件 → open=1" 1 "$(jq -r '.open | length' <<<"$j")"
  check "BLOCKER 1 件 → id が振られる" "R1-A-1" "$(jq -r '.open[0].id' <<<"$j")"
  check "BLOCKER 1 件 → deny 本文にその指摘だけ" 1 \
    "$(render_open <<<"$j" | grep -c '^### ')"

  # 2) MINOR + NIT のみ → pass、backlog に 2 件
  c="$(mkcritic A "[$(mkfinding MINOR '命名が惜しい'), $(mkfinding NIT '句点が揺れている')]" '[]')"
  j="$(judge 1 '[]' <<<"[$c]")"
  check "MINOR+NIT のみ → gate=false" false "$(jq -r '.gate' <<<"$j")"
  check "MINOR+NIT のみ → backlog=2" 2 "$(jq -r '.backlog | length' <<<"$j")"
  check "MINOR+NIT のみ → backlog 本文に 2 件" 2 \
    "$(render_backlog <<<"$j" | grep -c '^### ')"

  # 3) evidence が空の BLOCKER → 非適格として破棄、pass + 警告
  c="$(mkcritic A "[$(mkfinding BLOCKER '根拠なし' '   ')]" '[]')"
  j="$(judge 1 '[]' <<<"[$c]")"
  check "evidence 空 → gate=false" false "$(jq -r '.gate' <<<"$j")"
  check "evidence 空 → nonconforming=1" 1 "$(jq -r '.warn.nonconforming' <<<"$j")"
  case "$(warn_text <<<"$j")" in
    *破棄*) ok "evidence 空 → 警告が出る" ;;
    *) ng "evidence 空 → 警告が出る" ;;
  esac

  # 4) readiness false かつ適格 0 件 → fail-open 警告
  READY_NO_R='{"requirements":false,"scope":true,"implementation":true,"verification":true}'
  c="$(mkcritic A '[]' '[]' "$READY_NO_R")"
  j="$(judge 1 '[]' <<<"[$c]")"
  check "readiness 不整合 → gate=false" false "$(jq -r '.gate' <<<"$j")"
  check "readiness 不整合 → 検出される" true \
    "$(jq -r '.warn.readiness_inconsistent' <<<"$j")"
  case "$(warn_text <<<"$j")" in
    *readiness*) ok "readiness 不整合 → 警告が出る" ;;
    *) ng "readiness 不整合 → 警告が出る" ;;
  esac

  # 4b) readiness false かつ適格 1 件 → 不整合ではない
  c="$(mkcritic A "[$(mkfinding MAJOR '要件が未定義')]" '[]' "$READY_NO_R")"
  j="$(judge 1 '[]' <<<"[$c]")"
  check "readiness false + 裏付けあり → 不整合ではない" false \
    "$(jq -r '.warn.readiness_inconsistent' <<<"$j")"

  # 5) 不正な severity トークン → 除外 + 警告
  c="$(mkcritic A "[$(mkfinding CRITICAL '未知の severity')]" '[]')"
  j="$(judge 1 '[]' <<<"[$c]")"
  check "不正 severity → gate=false" false "$(jq -r '.gate' <<<"$j")"
  check "不正 severity → invalid_severity=1" 1 "$(jq -r '.warn.invalid_severity' <<<"$j")"

  # 6) 同一ラウンドの A/B 重複 summary → 1 件に dedup
  ca="$(mkcritic A "[$(mkfinding BLOCKER '同じ  問題を   指摘')]" '[]')"
  cb="$(mkcritic B "[$(mkfinding BLOCKER '同じ 問題を 指摘')]" '[]')"
  j="$(judge 1 '[]' <<<"[$ca, $cb]")"
  check "A/B 重複 → open=1" 1 "$(jq -r '.open | length' <<<"$j")"
  check "A/B 重複 → dup_dropped=1" 1 "$(jq -r '.warn.dup_dropped' <<<"$j")"

  # open set のフィクスチャ（ラウンド 2 用）
  OPEN1='[{"id":"R1-A-1","severity":"BLOCKER","kind":"TECHNICAL",
           "readiness_axis":"IMPLEMENTATION","summary":"前ラウンドの指摘",
           "failure_mode":"壊れる","trigger":"常に","evidence":"a.sh:1"}]'

  # 7) carry-over UNRESOLVED + 新規 0 件 → deny（初版の穴の回帰テスト）
  c="$(mkcritic C '[]' '[{"id":"R1-A-1","status":"UNRESOLVED","rationale":"まだ直っていない"}]')"
  j="$(judge 2 "$OPEN1" <<<"[$c]")"
  check "carry-over UNRESOLVED + 新規 0 → gate=true" true "$(jq -r '.gate' <<<"$j")"
  check "carry-over UNRESOLVED + 新規 0 → open=1" 1 "$(jq -r '.open | length' <<<"$j")"
  check "carry-over UNRESOLVED → id が保持される" "R1-A-1" "$(jq -r '.open[0].id' <<<"$j")"

  # 8) carry-over 全 RESOLVED + 新規 0 件 → pass
  c="$(mkcritic C '[]' '[{"id":"R1-A-1","status":"RESOLVED","rationale":"手順 3 で解消"}]')"
  j="$(judge 2 "$OPEN1" <<<"[$c]")"
  check "carry-over RESOLVED + 新規 0 → gate=false" false "$(jq -r '.gate' <<<"$j")"
  check "carry-over RESOLVED → closed=1" 1 "$(jq -r '.closed | length' <<<"$j")"

  # 9) 同じ id への重複・矛盾判定は順序によらず保守的に UNRESOLVED
  c="$(mkcritic C '[]' \
    '[{"id":"R1-A-1","status":"RESOLVED","rationale":"解消した"},
      {"id":"R1-A-1","status":"UNRESOLVED","rationale":"まだ残る"}]')"
  j="$(judge 2 "$OPEN1" <<<"[$c]")"
  check "carry-over 重複 → gate=true" true "$(jq -r '.gate' <<<"$j")"
  check "carry-over 重複 → UNRESOLVED が優先される" "UNRESOLVED" \
    "$(jq -r '.open[0]._carry' <<<"$j")"

  # 10) carry-over REFUTED_BY_PLAN → open set から外れる
  c="$(mkcritic C '[]' '[{"id":"R1-A-1","status":"REFUTED_BY_PLAN","rationale":"反証が妥当"}]')"
  j="$(judge 2 "$OPEN1" <<<"[$c]")"
  check "carry-over REFUTED_BY_PLAN → gate=false" false "$(jq -r '.gate' <<<"$j")"
  check "carry-over REFUTED_BY_PLAN → closed=1" 1 "$(jq -r '.closed | length' <<<"$j")"

  # 11) open set の id に判定が返らない → UNRESOLVED 扱い + 警告
  c="$(mkcritic C '[]' '[]')"
  j="$(judge 2 "$OPEN1" <<<"[$c]")"
  check "carry-over 未応答 → gate=true" true "$(jq -r '.gate' <<<"$j")"
  check "carry-over 未応答 → missing_carryover=1" 1 \
    "$(jq -r '.warn.missing_carryover' <<<"$j")"
  case "$(render_open <<<"$j")" in
    *前ラウンドから未解消*) ok "carry-over 未応答 → 本文に未解消と明示" ;;
    *) ng "carry-over 未応答 → 本文に未解消と明示" ;;
  esac

  # 12) open set に無い id を返してきた → 無視 + 警告
  c="$(mkcritic C '[]' '[{"id":"R1-A-1","status":"RESOLVED","rationale":"ok"},
                         {"id":"R9-Z-9","status":"UNRESOLVED","rationale":"知らない id"}]')"
  j="$(judge 2 "$OPEN1" <<<"[$c]")"
  check "未知 carry-over id → 無視される" false "$(jq -r '.gate' <<<"$j")"
  check "未知 carry-over id → unknown_carryover=1" 1 \
    "$(jq -r '.warn.unknown_carryover' <<<"$j")"

  # 12) ゲート閾値を BLOCKER のみに絞ると MAJOR は backlog に落ちる
  c="$(mkcritic A "[$(mkfinding MAJOR '手戻りが確実')]" '[]')"
  j="$(judge 1 '[]' BLOCKER <<<"[$c]")"
  check "閾値=BLOCKER のみ → gate=false" false "$(jq -r '.gate' <<<"$j")"
  check "閾値=BLOCKER のみ → MAJOR が backlog へ" 1 "$(jq -r '.backlog | length' <<<"$j")"
  # 既定の閾値が汚染されていないこと
  j="$(judge 1 '[]' <<<"[$c]")"
  check "既定の閾値では MAJOR がゲート対象" true "$(jq -r '.gate' <<<"$j")"

  # 12b) closer: gate 適格 severity を空文字で明示すると gate が無効化される。
  # judge() の $3 展開が `${3:-...}` に戻ると既定値 BLOCKER,MAJOR が復活してこれが落ちる。
  c="$(mkcritic Z "[$(mkfinding BLOCKER 'closer が見つけた新規'), $(mkfinding MAJOR 'もう一件')]" '[]')"
  j="$(judge 3 '[]' "" <<<"[$c]")"
  check "closer: 空 gate → gate=false" false "$(jq -r '.gate' <<<"$j")"
  check "closer: 空 gate → new_eligible=0" 0 "$(jq -r '.new_eligible | length' <<<"$j")"
  check "closer: 新規 BLOCKER/MAJOR は backlog へ" 2 "$(jq -r '.backlog | length' <<<"$j")"

  # 12c) closer: carry-over が UNRESOLVED なら新規流入なしでも gate は張る
  c="$(mkcritic Z "[$(mkfinding BLOCKER 'closer が見つけた新規')]" \
    '[{"id":"R1-A-1","status":"UNRESOLVED","rationale":"まだ直っていない"}]')"
  j="$(judge 3 "$OPEN1" "" <<<"[$c]")"
  check "closer: carry-over 未解消 → gate=true" true "$(jq -r '.gate' <<<"$j")"
  check "closer: carry-over 未解消 → open=1" 1 "$(jq -r '.open | length' <<<"$j")"
  check "closer: carry-over 未解消 → new_eligible=0" 0 \
    "$(jq -r '.new_eligible | length' <<<"$j")"

  # 12d) closer: carry-over 全閉なら PASS する（収束経路が到達可能であること）
  c="$(mkcritic Z '[]' '[{"id":"R1-A-1","status":"RESOLVED","rationale":"直した"}]')"
  j="$(judge 3 "$OPEN1" "" <<<"[$c]")"
  check "closer: carry-over 全閉 → gate=false" false "$(jq -r '.gate' <<<"$j")"
  check "closer: carry-over 全閉 → closed=1" 1 "$(jq -r '.closed | length' <<<"$j")"

  # 12e) closer: readiness 不整合の偽陽性を潰す
  c="$(mkcritic Z '[]' '[{"id":"R1-A-1","status":"RESOLVED","rationale":"直した"}]' \
    "$READY_NO_R")"
  j="$(judge 3 "$OPEN1" "" <<<"[$c]")"
  check "closer: 抑止前は readiness 不整合が立つ" true \
    "$(jq -r '.warn.readiness_inconsistent' <<<"$j")"
  j="$(suppress_closer_warn <<<"$j")"
  check "closer: 抑止後は readiness 不整合が消える" false \
    "$(jq -r '.warn.readiness_inconsistent' <<<"$j")"
  check "closer: 抑止で警告文が空になる" "" "$(warn_text <<<"$j")"

  # 13) 旧形式の markdown レビューは非 JSON として弾かれる
  legacy="$REVIEW_DIR/.legacy-out"
  printf -- '- [技術] 旧形式のレビュー本文\n\nVERDICT: REQUEST_CHANGES\n' > "$legacy"
  if jq -e '.' "$legacy" >/dev/null 2>&1; then
    ng "旧形式 markdown は非 JSON として弾かれる"
  else
    ok "旧形式 markdown は非 JSON として弾かれる"
  fi
  rm -f "$legacy"

  echo "ログ衛生:"

  # 14) 保持期限の掃除。skip は残す
  touch -d '40 days ago' "$REVIEW_DIR/20250101-000000-deadbeef.md"
  touch -d '40 days ago' "$REVIEW_DIR/skip"
  prune_old
  if [[ -e "$REVIEW_DIR/20250101-000000-deadbeef.md" ]]; then
    ng "保持期限より古い log が掃除される"
  else
    ok "保持期限より古い log が掃除される"
  fi
  if [[ -e "$REVIEW_DIR/skip" ]]; then
    ok "skip フラグは掃除されない"
  else
    ng "skip フラグは掃除されない"
  fi
  rm -f "$REVIEW_DIR/skip"

  # 15) 生成物の権限
  printf 'x\n' > "$REVIEW_DIR/perm-probe.md"
  check "生成物の権限" 600 "$(stat -c '%a' "$REVIEW_DIR/perm-probe.md")"
  check "REVIEW_DIR の権限" 700 "$(stat -c '%a' "$REVIEW_DIR")"
  rm -f "$REVIEW_DIR/perm-probe.md"

  echo "hook 経路:"

  self="${BASH_SOURCE[0]}"
  fixture="$selftest_dir/input.json"
  mkhookinput() { # $1=session_id
    jq -n --arg s "$1" '{session_id: $s, cwd: ".", hook_event_name: "PreToolUse",
                         tool_name: "ExitPlanMode", permission_mode: "plan",
                         tool_input: {plan: "# plan\n"}}' > "$fixture"
  }
  # 隔離(#56 / #58): サブプロセスにもホストの ambient env(home-manager の
  # sessionVariables 等)を継承させない。env -u で関連キーを明示的に消してから
  # $@ で上書きするテスト用の値だけを渡す。
  runhook() { # 残りの引数は env 代入として渡す
    env \
      -u COPILOT_BIN -u COPILOT_PLAN_REVIEW_DIR -u COPILOT_PLAN_REVIEW_MODEL \
      -u COPILOT_PLAN_REVIEW_AGENT -u COPILOT_PLAN_REVIEW_TIMEOUT \
      -u COPILOT_PLAN_REVIEW_GATE_SEVERITIES -u COPILOT_PLAN_REVIEW_PARALLEL \
      -u COPILOT_PLAN_REVIEW_RETENTION_DAYS -u COPILOT_PLAN_REVIEW_SCHEMA \
      -u MAX_PLAN_REVIEWS -u SKIP_PLAN_REVIEW \
      COPILOT_PLAN_REVIEW_DIR="$REVIEW_DIR" "$@" bash "$self" < "$fixture"
  }
  decision() { jq -r '.hookSpecificOutput.permissionDecision // "none"'; }
  reason() { jq -r '.hookSpecificOutput.permissionDecisionReason // ""'; }

  # 16) 上限到達 + open set 非空 → deny 1 回 → 2 回目は素通り
  mkhookinput selftest-cap
  printf '%s\n' "$MAX_REVIEWS" > "$STATE_DIR/selftest-cap.count"
  printf '%s\n' "$OPEN1" > "$STATE_DIR/selftest-cap.open.json"
  out="$(runhook COPILOT_BIN=true)"
  check "上限到達 + open 非空 → deny" deny "$(decision <<<"$out")"
  case "$(reason <<<"$out")" in
    *AskUserQuestion*GO*NO-GO*) ok "上限到達 → GO/NO-GO を要求する" ;;
    *) ng "上限到達 → GO/NO-GO を要求する" ;;
  esac
  case "$(reason <<<"$out")" in
    *前ラウンドの指摘*) ok "上限到達 → 残存指摘の本文が入る" ;;
    *) ng "上限到達 → 残存指摘の本文が入る" ;;
  esac
  if [[ -e "$STATE_DIR/selftest-cap.escalated" ]]; then
    ok "上限到達 → escalated フラグが立つ"
  else
    ng "上限到達 → escalated フラグが立つ"
  fi
  out="$(runhook COPILOT_BIN=true)"
  check "escalated 済み → 素通り" none "$(decision <<<"$out")"

  # 17) 上限到達 + open set 空 → 素通り
  mkhookinput selftest-cap2
  printf '%s\n' "$MAX_REVIEWS" > "$STATE_DIR/selftest-cap2.count"
  printf '[]\n' > "$STATE_DIR/selftest-cap2.open.json"
  out="$(runhook COPILOT_BIN=true)"
  check "上限到達 + open 空 → 素通り" none "$(decision <<<"$out")"

  # 18) copilot 不在 → 無言で素通り、カウンタも作らない
  mkhookinput selftest-nocopilot
  out="$(runhook COPILOT_BIN=definitely-not-a-real-binary)"
  check "copilot 不在 → 出力なし" "" "$out"
  if [[ -e "$STATE_DIR/selftest-nocopilot.count" ]]; then
    ng "copilot 不在 → カウンタを作らない"
  else
    ok "copilot 不在 → カウンタを作らない"
  fi

  # 19) SKIP_PLAN_REVIEW=1 / skip フラグ → 無言で素通り
  out="$(runhook COPILOT_BIN=true SKIP_PLAN_REVIEW=1)"
  check "SKIP_PLAN_REVIEW=1 → 出力なし" "" "$out"
  : > "$REVIEW_DIR/skip"
  out="$(runhook COPILOT_BIN=true)"
  check "skip フラグ → 出力なし" "" "$out"
  rm -f "$REVIEW_DIR/skip"

  # 20) debug ダンプにプラン本文が残らない
  if jq -e 'has("tool_input")' "$REVIEW_DIR/debug-last-input.json" >/dev/null 2>&1; then
    ng "debug ダンプにプラン本文が残らない"
  else
    ok "debug ダンプにプラン本文が残らない"
  fi
  check "debug ダンプに文字数だけが残る" 7 \
    "$(jq -r '.plan_chars' "$REVIEW_DIR/debug-last-input.json")"

  # 21) 偽 copilot で並列経路を通す。lens B だけ失敗させる。
  # 実機の copilot 1.0.82 の引数形(-p, --agent, --model, --silent,
  # --no-custom-instructions, --disable-builtin-mcps, --no-ask-user,
  # --add-dir)を受け、固定契約(model/agent/read-only
  # 境界・危険フラグの不在)を自ら検証してから応答する。
  fake_dir="$selftest_dir/fake"
  mkdir -p "$fake_dir"
  cat > "$fake_dir/copilot" <<'FAKEEOF'
#!/usr/bin/env bash
set -u
prompt="" agent="" model=""
saw_silent=0 saw_no_custom=0 saw_disable_mcps=0 saw_no_ask_user=0
add_dirs=()
while [ $# -gt 0 ]; do
  case "$1" in
    -p) prompt="${2:-}"; shift 2 ;;
    --agent) agent="${2:-}"; shift 2 ;;
    --model) model="${2:-}"; shift 2 ;;
    --add-dir) add_dirs+=("${2:-}"); shift 2 ;;
    --silent) saw_silent=1; shift ;;
    --no-custom-instructions) saw_no_custom=1; shift ;;
    --disable-builtin-mcps) saw_disable_mcps=1; shift ;;
    --no-ask-user) saw_no_ask_user=1; shift ;;
    --allow-all-tools | --yolo | --allow-all | --allow-all-paths | --allow-all-urls | --allow-url*)
      echo "FAKE_COPILOT: dangerous flag $1 must never be passed" >&2
      exit 99
      ;;
    *) shift ;;
  esac
done
if [ "$agent" != "${FAKE_COPILOT_EXPECT_AGENT:-plan-reviewer}" ]; then
  echo "FAKE_COPILOT: unexpected --agent '$agent'" >&2
  exit 98
fi
if [ "$model" != "${FAKE_COPILOT_EXPECT_MODEL:-gpt-5.6-sol}" ]; then
  echo "FAKE_COPILOT: unexpected --model '$model'" >&2
  exit 97
fi
if [ "$saw_silent$saw_no_custom$saw_disable_mcps$saw_no_ask_user" != "1111" ]; then
  echo "FAKE_COPILOT: missing required noninteractive/read-only flag(s)" >&2
  exit 96
fi
lens=M
case "$prompt" in
  *"(lens A)"*) lens=A ;;
  *"(lens B)"*) lens=B ;;
  *"(lens C)"*) lens=C ;;
  *"(lens Z"*) lens=Z ;;
esac
{
  printf 'agent=%s\n' "$agent"
  printf 'model=%s\n' "$model"
  printf 'silent=%s custom=%s mcps=%s askuser=%s\n' \
    "$saw_silent" "$saw_no_custom" "$saw_disable_mcps" "$saw_no_ask_user"
  printf 'add_dirs=%s\n' "${add_dirs[*]-}"
} > "$FAKE_COPILOT_DIR/last-invocation-$lens.txt" 2>/dev/null
[ -e "$FAKE_COPILOT_DIR/$lens.fail" ] && exit 1
if [ -e "$FAKE_COPILOT_DIR/$lens.fence" ]; then
  printf '```json\n'
  cat "$FAKE_COPILOT_DIR/$lens.json"
  printf '\n```\n'
else
  cat "$FAKE_COPILOT_DIR/$lens.json"
fi
# 有効な JSON を書いたうえで非ゼロ終了する（タイムアウト・後処理エラーの再現）
[ -e "$FAKE_COPILOT_DIR/$lens.rcfail" ] && exit 1
exit 0
FAKEEOF
  chmod +x "$fake_dir/copilot"
  mkcritic A "[$(mkfinding BLOCKER 'lens A の指摘')]" '[]' | jq '.data' > "$fake_dir/A.json"
  : > "$fake_dir/B.fail"
  mkhookinput selftest-parallel
  out="$(runhook COPILOT_BIN="$fake_dir/copilot" FAKE_COPILOT_DIR="$fake_dir" \
    COPILOT_PLAN_REVIEW_TIMEOUT=20)"
  check "片側失敗 → deny" deny "$(decision <<<"$out")"
  case "$(reason <<<"$out")" in
    *"lens B"*) ok "片側失敗 → 失敗した lens を報告する" ;;
    *) ng "片側失敗 → 失敗した lens を報告する" ;;
  esac
  check "片側失敗 → ラウンドを消費する" 1 \
    "$(cat "$STATE_DIR/selftest-parallel.count" 2>/dev/null || echo missing)"
  check "片側失敗 → open set を保存する" 1 \
    "$(jq -r 'length' "$STATE_DIR/selftest-parallel.open.json" 2>/dev/null || echo missing)"

  # 22) ラウンド 2 は lens C で carry-over 判定を行う
  cat > "$fake_dir/C.json" <<'CJSON'
{"readiness": {"requirements": true, "scope": true, "implementation": true,
               "verification": true},
 "findings": [],
 "carryover": [{"id": "R1-A-1", "status": "RESOLVED", "rationale": "修正済み"}]}
CJSON
  out="$(runhook COPILOT_BIN="$fake_dir/copilot" FAKE_COPILOT_DIR="$fake_dir" \
    COPILOT_PLAN_REVIEW_TIMEOUT=20)"
  check "ラウンド 2 で carry-over 解消 → 素通り" none "$(decision <<<"$out")"
  check "ラウンド 2 → open set が空になる" 0 \
    "$(jq -r 'length' "$STATE_DIR/selftest-parallel.open.json")"
  check "ラウンド 2 → カウンタが 2 になる" 2 \
    "$(cat "$STATE_DIR/selftest-parallel.count")"

  # 22b) 最終ラウンドは closer。carry-over 全閉 → PASS
  #      （収束経路が実際に到達可能であることの回帰テスト。旧設計ではここが 0% だった）
  cat > "$fake_dir/Z.json" <<'ZJSON'
{"readiness": {"requirements": true, "scope": true, "implementation": true,
               "verification": true},
 "findings": [],
 "carryover": [{"id": "R1-A-1", "status": "RESOLVED", "rationale": "改訂で解消"}]}
ZJSON
  mkhookinput selftest-closer-pass
  printf '%s\n' "$((MAX_REVIEWS - 1))" > "$STATE_DIR/selftest-closer-pass.count"
  printf '%s\n' "$OPEN1" > "$STATE_DIR/selftest-closer-pass.open.json"
  out="$(runhook COPILOT_BIN="$fake_dir/copilot" FAKE_COPILOT_DIR="$fake_dir" \
    COPILOT_PLAN_REVIEW_TIMEOUT=20)"
  check "closer: carry-over 全閉 → 素通り" none "$(decision <<<"$out")"
  check "closer: carry-over 全閉 → open set が空になる" 0 \
    "$(jq -r 'length' "$STATE_DIR/selftest-closer-pass.open.json")"
  check "closer: ラウンドを消費する" "$MAX_REVIEWS" \
    "$(cat "$STATE_DIR/selftest-closer-pass.count")"
  if [[ -e "$STATE_DIR/selftest-closer-pass.escalated" ]]; then
    ng "closer: PASS ならエスカレーションしない"
  else
    ok "closer: PASS ならエスカレーションしない"
  fi

  # 22c) closer で carry-over が残る → 通常 deny を飛ばして即エスカレーション。
  #      同時に出た新規 BLOCKER は gate 対象にせず backlog へ回す。
  cat > "$fake_dir/Z.json" <<'ZJSON'
{"readiness": {"requirements": true, "scope": true, "implementation": true,
               "verification": true},
 "findings": [{"severity": "BLOCKER", "kind": "TECHNICAL",
               "readiness_axis": "IMPLEMENTATION", "summary": "closer が見つけた新規",
               "failure_mode": "壊れる", "trigger": "常に", "evidence": "z.sh:1"}],
 "carryover": [{"id": "R1-A-1", "status": "UNRESOLVED", "rationale": "まだ直っていない"}]}
ZJSON
  mkhookinput selftest-closer-deny
  printf '%s\n' "$((MAX_REVIEWS - 1))" > "$STATE_DIR/selftest-closer-deny.count"
  printf '%s\n' "$OPEN1" > "$STATE_DIR/selftest-closer-deny.open.json"
  out="$(runhook COPILOT_BIN="$fake_dir/copilot" FAKE_COPILOT_DIR="$fake_dir" \
    COPILOT_PLAN_REVIEW_TIMEOUT=20)"
  check "closer: carry-over 未解消 → deny" deny "$(decision <<<"$out")"
  case "$(reason <<<"$out")" in
    *AskUserQuestion*GO*NO-GO*) ok "closer: GO/NO-GO を要求する" ;;
    *) ng "closer: GO/NO-GO を要求する" ;;
  esac
  case "$(reason <<<"$out")" in
    *"再度 ExitPlanMode を呼んでください"*)
      ng "closer: 通常 deny（直して来い）には落ちない" ;;
    *) ok "closer: 通常 deny（直して来い）には落ちない" ;;
  esac
  if [[ -e "$STATE_DIR/selftest-closer-deny.escalated" ]]; then
    ok "closer: escalated フラグが立つ"
  else
    ng "closer: escalated フラグが立つ"
  fi
  check "closer: 新規 BLOCKER は open set に入らない" 1 \
    "$(jq -r 'length' "$STATE_DIR/selftest-closer-deny.open.json")"
  check "closer: open set は carry-over だけ" "R1-A-1" \
    "$(jq -r '.[0].id' "$STATE_DIR/selftest-closer-deny.open.json")"
  case "$(cat "$BACKLOG_DIR/selftest-closer-deny.md" 2>/dev/null)" in
    *"closer が見つけた新規"*) ok "closer: 新規 BLOCKER は backlog に残る" ;;
    *) ng "closer: 新規 BLOCKER は backlog に残る" ;;
  esac

  # 22d) escalated 済みなら上限未到達でも素通る（早期 exit の回帰テスト）
  mkhookinput selftest-escalated
  printf '1\n' > "$STATE_DIR/selftest-escalated.count"
  printf '%s\n' "$OPEN1" > "$STATE_DIR/selftest-escalated.open.json"
  : > "$STATE_DIR/selftest-escalated.escalated"
  out="$(runhook COPILOT_BIN="$fake_dir/copilot" FAKE_COPILOT_DIR="$fake_dir")"
  check "escalated 済み + 上限未到達 → 素通り" none "$(decision <<<"$out")"
  check "escalated 済み → ラウンドを消費しない" 1 \
    "$(cat "$STATE_DIR/selftest-escalated.count")"

  # 22e) MAX_PLAN_REVIEWS=1 の退化ケースではラウンド 1 を closer 化しない
  mkcritic M "[$(mkfinding BLOCKER 'lens M の指摘')]" '[]' | jq '.data' > "$fake_dir/M.json"
  mkhookinput selftest-max1
  out="$(runhook COPILOT_BIN="$fake_dir/copilot" FAKE_COPILOT_DIR="$fake_dir" \
    COPILOT_PLAN_REVIEW_TIMEOUT=20 MAX_PLAN_REVIEWS=1 COPILOT_PLAN_REVIEW_PARALLEL=0)"
  check "MAX=1: ラウンド 1 は closer 化しない → deny" deny "$(decision <<<"$out")"
  check "MAX=1: 新規 BLOCKER が open set に入る" 1 \
    "$(jq -r 'length' "$STATE_DIR/selftest-max1.open.json")"
  if [[ -e "$STATE_DIR/selftest-max1.escalated" ]]; then
    ng "MAX=1: ラウンド 1 でエスカレーションしない"
  else
    ok "MAX=1: ラウンド 1 でエスカレーションしない"
  fi

  # 23) 有効な JSON を書いた後に非ゼロ終了 → 失敗として扱う
  #     （rc を捨てていると成功扱いになり、不完全な結果で gate を張ってしまう）
  : > "$fake_dir/A.rcfail"
  mkhookinput selftest-rcfail
  out="$(runhook COPILOT_BIN="$fake_dir/copilot" FAKE_COPILOT_DIR="$fake_dir" \
    COPILOT_PLAN_REVIEW_TIMEOUT=20)"
  check "JSON を書いて非ゼロ終了 → 素通り" none "$(decision <<<"$out")"
  if [[ -e "$STATE_DIR/selftest-rcfail.count" ]]; then
    ng "JSON を書いて非ゼロ終了 → ラウンドを消費しない"
  else
    ok "JSON を書いて非ゼロ終了 → ラウンドを消費しない"
  fi

  # 24) 両方失敗 → fail-open、ラウンド非消費
  : > "$fake_dir/A.fail"
  mkhookinput selftest-bothfail
  out="$(runhook COPILOT_BIN="$fake_dir/copilot" FAKE_COPILOT_DIR="$fake_dir" \
    COPILOT_PLAN_REVIEW_TIMEOUT=20)"
  check "両方失敗 → 素通り" none "$(decision <<<"$out")"
  if [[ -e "$STATE_DIR/selftest-bothfail.count" ]]; then
    ng "両方失敗 → ラウンドを消費しない"
  else
    ok "両方失敗 → ラウンドを消費しない"
  fi
  rm -f "$fake_dir"/*.fail "$fake_dir"/*.rcfail

  echo "copilot 呼び出し契約:"

  # 25) 固定契約: --agent / --model / 非対話・read-only フラグが実際に渡っている。
  # ラウンド 1 を PARALLEL=0 で lens M 単独にし、fake copilot が書いた argv dump を検査する。
  mkcritic M "[$(mkfinding BLOCKER 'lens M の指摘')]" '[]' | jq '.data' > "$fake_dir/M.json"
  mkhookinput selftest-contract
  out="$(runhook COPILOT_BIN="$fake_dir/copilot" FAKE_COPILOT_DIR="$fake_dir" \
    COPILOT_PLAN_REVIEW_TIMEOUT=20 COPILOT_PLAN_REVIEW_PARALLEL=0)"
  check "契約: --agent は plan-reviewer 固定" "agent=plan-reviewer" \
    "$(sed -n 1p "$fake_dir/last-invocation-M.txt" 2>/dev/null)"
  check "契約: --model の既定は gpt-5.6-sol" "model=gpt-5.6-sol" \
    "$(sed -n 2p "$fake_dir/last-invocation-M.txt" 2>/dev/null)"
  check "契約: --silent/--no-custom-instructions/--disable-builtin-mcps/--no-ask-user が揃う" \
    "silent=1 custom=1 mcps=1 askuser=1" \
    "$(sed -n 3p "$fake_dir/last-invocation-M.txt" 2>/dev/null)"

  # 26) COPILOT_PLAN_REVIEW_MODEL で model を上書きできる
  mkhookinput selftest-model-override
  out="$(runhook COPILOT_BIN="$fake_dir/copilot" FAKE_COPILOT_DIR="$fake_dir" \
    COPILOT_PLAN_REVIEW_TIMEOUT=20 COPILOT_PLAN_REVIEW_PARALLEL=0 \
    COPILOT_PLAN_REVIEW_MODEL=gpt-9-test FAKE_COPILOT_EXPECT_MODEL=gpt-9-test)"
  check "契約: COPILOT_PLAN_REVIEW_MODEL で上書きできる" "model=gpt-9-test" \
    "$(sed -n 2p "$fake_dir/last-invocation-M.txt" 2>/dev/null)"

  # 27) COPILOT_PLAN_REVIEW_AGENT で agent を上書きできる
  mkhookinput selftest-agent-override
  out="$(runhook COPILOT_BIN="$fake_dir/copilot" FAKE_COPILOT_DIR="$fake_dir" \
    COPILOT_PLAN_REVIEW_TIMEOUT=20 COPILOT_PLAN_REVIEW_PARALLEL=0 \
    COPILOT_PLAN_REVIEW_AGENT=custom-reviewer FAKE_COPILOT_EXPECT_AGENT=custom-reviewer)"
  check "契約: COPILOT_PLAN_REVIEW_AGENT で上書きできる" "agent=custom-reviewer" \
    "$(sed -n 1p "$fake_dir/last-invocation-M.txt" 2>/dev/null)"

  # 28) plan file がリポジトリ外にあるとき、その親ディレクトリだけ --add-dir する
  #    (mkhookinput は cwd="." + tool_input.plan を渡すので plan_tmp は必ず
  #    $REVIEW_DIR 配下 = workdir(".") の外になり、常に add-dir されるはず)
  mkhookinput selftest-adddir
  out="$(runhook COPILOT_BIN="$fake_dir/copilot" FAKE_COPILOT_DIR="$fake_dir" \
    COPILOT_PLAN_REVIEW_TIMEOUT=20 COPILOT_PLAN_REVIEW_PARALLEL=0)"
  case "$(sed -n 4p "$fake_dir/last-invocation-M.txt" 2>/dev/null)" in
    "add_dirs=$REVIEW_DIR"*) ok "契約: plan file の親ディレクトリを --add-dir する" ;;
    *) ng "契約: plan file の親ディレクトリを --add-dir する" ;;
  esac

  echo "critic の JSON 厳格検証:"

  # 29) コードフェンス付き JSON → 非 JSON として critic failure(fail-open)
  : > "$fake_dir/M.fence"
  mkhookinput selftest-fence
  out="$(runhook COPILOT_BIN="$fake_dir/copilot" FAKE_COPILOT_DIR="$fake_dir" \
    COPILOT_PLAN_REVIEW_TIMEOUT=20 COPILOT_PLAN_REVIEW_PARALLEL=0)"
  check "フェンス付き JSON → fail-open" none "$(decision <<<"$out")"
  if [[ -e "$STATE_DIR/selftest-fence.count" ]]; then
    ng "フェンス付き JSON → ラウンドを消費しない"
  else
    ok "フェンス付き JSON → ラウンドを消費しない"
  fi
  rm -f "$fake_dir/M.fence"

  # $1=変換後の JSON を書く jq フィルタ $2=ラベル $3=session id
  check_schema_reject() {
    local filter="$1" label="$2" sid="$3"
    jq "$filter" "$fake_dir/M.json" > "$fake_dir/M.json.mut" && mv "$fake_dir/M.json.mut" "$fake_dir/M.json"
    mkhookinput "$sid"
    out="$(runhook COPILOT_BIN="$fake_dir/copilot" FAKE_COPILOT_DIR="$fake_dir" \
      COPILOT_PLAN_REVIEW_TIMEOUT=20 COPILOT_PLAN_REVIEW_PARALLEL=0)"
    check "$label → fail-open" none "$(decision <<<"$out")"
    if [[ -e "$STATE_DIR/$sid.count" ]]; then
      ng "$label → ラウンドを消費しない"
    else
      ok "$label → ラウンドを消費しない"
    fi
    # 次のテストのため常に正当な baseline に戻す
    mkcritic M "[$(mkfinding BLOCKER 'lens M の指摘')]" '[]' | jq '.data' > "$fake_dir/M.json"
  }

  # 30) top-level キー欠落(carryover が無い) → additionalProperties/required 違反
  check_schema_reject 'del(.carryover)' "top-level キー欠落" selftest-missingkey

  # 31) top-level に想定外キーを追加 → additionalProperties 違反
  check_schema_reject '. + {extra: "unexpected"}' "top-level 余剰キー" selftest-extrakey

  # 32) finding の severity が enum 外 → 破棄され critic failure
  check_schema_reject '.findings[0].severity = "CRITICAL"' "severity enum 違反" selftest-badenum

  # 33) readiness の型が boolean でない → 破棄され critic failure
  check_schema_reject '.readiness.requirements = "true"' "readiness 型不正" selftest-badtype

  # 34) finding に想定外キーが混じる → additionalProperties 違反
  check_schema_reject '.findings[0].extra = "nope"' "finding 余剰キー" selftest-findingextrakey

  echo
  if [[ "$selftest_fail" == "0" ]]; then
    echo "selftest: すべて通過"
    exit 0
  fi
  echo "selftest: 失敗あり" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# hook モード
# ---------------------------------------------------------------------------

ensure_dirs
prune_old

INPUT="$(cat)"

EVENT="$(jq -r '.hook_event_name // "PreToolUse"' <<<"$INPUT")"
SESSION_ID="$(jq -r '.session_id // "unknown"' <<<"$INPUT")"
SESSION_ID="${SESSION_ID//[^A-Za-z0-9._-]/_}"
CWD="$(jq -r '.cwd // empty' <<<"$INPUT")"
[[ -d "$CWD" ]] || CWD="$HOME"

# デバッグ用ダンプはメタデータのみ。プラン本文と transcript_path は残さない。
jq '{session_id, cwd, hook_event_name, tool_name, permission_mode,
     plan_chars: ((.tool_input.plan // "") | length),
     has_plan_file_path: ((.tool_input.planFilePath // "") != "")}' \
  <<<"$INPUT" > "$REVIEW_DIR/debug-last-input.json" 2>/dev/null || true

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

# 未解消の指摘を残したまま最終ラウンドを終えた → 人間の GO/NO-GO を強制する。
# deny は 1 回だけ（ESCALATED_FILE で限定）。以後そのセッションは素通る。
# $1=judged JSON（.open を持つ） $2=状況の説明行 $3=レビュー全文のパス（空可）
escalate_with() {
  local judged="$1" situation="$2" log_path="$3" n residual
  n="$(jq '.open | length' <<<"$judged")"
  residual="$(render_open <<<"$judged")"
  : > "$ESCALATED_FILE"
  deny_with "${situation}実装をブロックする指摘が ${n} 件未解消のまま残っています。

追加のレビューは行いません。**AskUserQuestion で、この状態のまま実装に進んでよいか (GO / NO-GO) をユーザーに確認すること。** あなたの判断で未解消のまま進めてはならない。

- GO なら、そのまま再度 ExitPlanMode を呼ぶ（次回は素通ります）。
- NO-GO なら、ExitPlanMode を呼ばずにプランの修正を続けること。

--- 未解消の指摘 ---

$residual
MINOR/NIT は $BACKLOG_FILE を参照。レビュー全文: ${log_path:-$REVIEW_DIR}"
}

# --- copilot 不在のマシンでは黙って素通り（ADR-0005: バイナリ存在でゲート） ---
if ! command -v "$COPILOT_BIN" >/dev/null 2>&1; then
  pass_through
fi

# --- エスケープハッチ ---
if [[ -e "$REVIEW_DIR/skip" || "${SKIP_PLAN_REVIEW:-0}" == "1" ]]; then
  pass_through
fi

COUNT_FILE="$STATE_DIR/${SESSION_ID}.count"
OPEN_FILE="$STATE_DIR/${SESSION_ID}.open.json"
ESCALATED_FILE="$STATE_DIR/${SESSION_ID}.escalated"
BACKLOG_FILE="$BACKLOG_DIR/${SESSION_ID}.md"

count="$(cat "$COUNT_FILE" 2>/dev/null || echo 0)"
case "$count" in
  '' | *[!0-9]*) count=0 ;;
esac

open_set="$(cat "$OPEN_FILE" 2>/dev/null || echo '[]')"
jq -e 'type == "array"' <<<"$open_set" >/dev/null 2>&1 || open_set='[]'
open_n="$(jq 'length' <<<"$open_set")"

# --- 既にエスカレーション済み → 無条件に素通る（ループしない） ---
# ラウンド上限の判定より前に置く。エスカレーションは closer ラウンドの直後に起きるので、
# 上限到達を待たずに立つフラグである。
if [[ -e "$ESCALATED_FILE" ]]; then
  latest_log="$(ls -t "$REVIEW_DIR"/*.md 2>/dev/null | head -1)"
  pass_through "Copilot プランレビュー: このセッションでは既に人間の GO/NO-GO を要求したため素通しします。直近のレビュー: ${latest_log:-$REVIEW_DIR}"
fi

# --- ラウンド上限 ---
# 通常フローでは closer ラウンドが escalate_with を呼ぶのでここには未解消が残らない。
# MAX_PLAN_REVIEWS を途中で下げた等で古い state が上限を超えている場合の安全網。
if [[ "$count" -ge "$MAX_REVIEWS" ]]; then
  latest_log="$(ls -t "$REVIEW_DIR"/*.md 2>/dev/null | head -1)"
  if [[ "$open_n" != "0" ]]; then
    # copilot は呼ばない。deny を 1 回だけ返して人間の GO/NO-GO を強制する。
    escalate_with "$(jq -n --argjson o "$open_set" '{open: $o}')" \
      "Copilot プランレビューの上限 (${MAX_REVIEWS} ラウンド) に到達しましたが、" \
      "$latest_log"
  fi
  pass_through "Copilot プランレビュー: このセッションの上限 (${MAX_REVIEWS} ラウンド) に達したため素通しします。直近のレビュー: ${latest_log:-$REVIEW_DIR}"
fi

# --- プラン本文の取得: tool_input.plan → planFilePath → 最新の ~/.claude/plans/*.md ---
plan_tmp=""
plan_file=""
plan_text="$(jq -r '.tool_input.plan // empty' <<<"$INPUT")"
plan_path="$(jq -r '.tool_input.planFilePath // empty' <<<"$INPUT")"
if [[ -n "$plan_text" ]]; then
  plan_tmp="$(mktemp "$REVIEW_DIR/.plan.XXXXXX.md")"
  printf '%s\n' "$plan_text" > "$plan_tmp"
  plan_file="$plan_tmp"
elif [[ -n "$plan_path" && -f "$plan_path" ]]; then
  plan_file="$plan_path"
else
  latest_plan="$(ls -t "$HOME/.claude/plans/"*.md 2>/dev/null | head -1)"
  if [[ -z "$latest_plan" ]]; then
    pass_through "Copilot プランレビュー: プラン本文を取得できなかったためスキップしました（tool_input.plan / planFilePath なし、~/.claude/plans/ も空）。"
  fi
  plan_file="$latest_plan"
fi

round=$((count + 1))

# --- lens の選択 ---
# ラウンド 1 は観点の異なる 2 critic を並列（相関した見落としを減らす）。
# 中間ラウンドは adversarial 1 本 + carry-over 判定。
# 最終ラウンドは closer（carry-over 判定専用、新規は gate 対象外）。
#
# MAX_PLAN_REVIEWS=1 に絞られた退化ケースでは closer 化しない。発見ラウンドが 1 本も
# 無くなり、レビューが carry-over 判定だけの空回りになるため。
closer=0
if [[ "$round" -ge "$MAX_REVIEWS" && "$round" -gt 1 ]]; then
  lenses=(Z)
  closer=1
elif [[ "$round" == "1" ]]; then
  if [[ "$PARALLEL" == "1" ]]; then
    lenses=(A B)
  else
    lenses=(M)
  fi
else
  lenses=(C)
fi

# --- レビュー実行 ---
critic_raw="$(run_critics "$plan_file" "$CWD" "$open_set" "${lenses[@]}")"
[[ -n "$plan_tmp" ]] && rm -f "$plan_tmp"

critics="$(jq '.critics' <<<"$critic_raw" 2>/dev/null || echo '[]')"
critic_failed="$(jq -r '.failed | join(", ")' <<<"$critic_raw" 2>/dev/null || echo '')"

if [[ "$(jq 'length' <<<"$critics" 2>/dev/null || echo 0)" == "0" ]]; then
  pass_through "Copilot プランレビュー: 実行に失敗しました（タイムアウト ${COPILOT_TIMEOUT}s・未ログイン・ネットワーク等）。fail-open で通過させます。ラウンドは消費していません。"
fi

# closer ラウンドは gate 適格 severity を空にする（= 新規 finding は全て backlog へ）。
# judge() の $3 は `${3-...}` 展開なので、空文字が既定値に戻ることはない。
if [[ "$closer" == "1" ]]; then
  judged="$(judge "$round" "$open_set" "" <<<"$critics" | suppress_closer_warn)" ||
    judged=""
else
  judged="$(judge "$round" "$open_set" <<<"$critics")" || judged=""
fi
if [[ -z "$judged" ]]; then
  pass_through "Copilot プランレビュー: レビュー結果の解析に失敗しました。fail-open で通過させます。ラウンドは消費していません。"
fi

# --- ここから先はレビューが成立した = ラウンドを消費する ---
echo "$round" > "$COUNT_FILE"

ts="$(date +%Y%m%d-%H%M%S)"
review_log="$REVIEW_DIR/${ts}-${SESSION_ID:0:8}.md"
review_json="$REVIEW_DIR/${ts}-${SESSION_ID:0:8}.json"
render_log <<<"$judged" > "$review_log"
jq -n --argjson j "$judged" --argjson c "$critics" '{judged: $j, critics: $c}' > "$review_json"

jq '.open' <<<"$judged" > "$OPEN_FILE"

backlog_md="$(render_backlog <<<"$judged")"
if [[ -n "$backlog_md" ]]; then
  printf '%s\n' "$backlog_md" >> "$BACKLOG_FILE"
fi

warn="$(warn_text <<<"$judged")"
if [[ -n "$critic_failed" ]]; then
  warn="${warn:+$warn。}lens $critic_failed が失敗したため残りの critic のみで判定しました"
fi

gate="$(jq -r '.gate' <<<"$judged")"
open_n="$(jq -r '.open | length' <<<"$judged")"
new_n="$(jq -r '.new_eligible | length' <<<"$judged")"
carried_n="$(jq -r '.carried | length' <<<"$judged")"
backlog_n="$(jq -r '.backlog | length' <<<"$judged")"

if [[ "$gate" == "true" ]]; then
  # closer は最後の言葉である。ここで通常 deny（= 直してもう一度来い）を返すと、その
  # 修正を判定するラウンドがまた無くなる。人間の GO/NO-GO に直行する。
  if [[ "$closer" == "1" ]]; then
    escalate_with "$judged" \
      "Copilot プランレビューの最終ラウンド (closer) で改訂プランに対して再判定した結果、" \
      "$review_log"
  fi
  deny_with "Copilot によるプランレビューの結果、実装をブロックする指摘が ${open_n} 件あります（ラウンド ${round}/${MAX_REVIEWS}、新規 ${new_n} 件 / 前ラウンドから未解消 ${carried_n} 件）。

対応の方針:

- **BLOCKER / MAJOR だけが対応対象**です。MINOR/NIT は $BACKLOG_FILE に退避済みで、いま直す必要はありません。
- [TECHNICAL] の指摘: リポジトリ等の証拠で検証し、妥当なら反映してください。**反証できた指摘は、反証の根拠をプランに明記して却下してよい**（却下は正当な帰結です）。
- [NEEDS_DECISION] の指摘: 勝手に採否を判断してプランに反映してはいけません。レビュアーの意見はユーザーの決定ではありません。必ず AskUserQuestion でユーザーに論点と選択肢（あなたの推奨付き）を提示し、回答を得てから修正してください。
- 「もっと良いプランが存在する」ことは指摘理由になりません。任意の改善提案として書かれているものがあれば無視してよい。

対応が済んでから再度 ExitPlanMode を呼んでください。次のラウンドでは、ここに挙がった各指摘が解消されたか / 反証されたか / 未解消かが判定されます。${warn:+

（注意: $warn）}

--- 未解消の指摘 ---

$(render_open <<<"$judged")
レビュー全文: $review_log"
fi

if [[ "$closer" == "1" ]]; then
  # closer は gate 適格 severity を空にして走るので、backlog には BLOCKER/MAJOR も
  # 混じる。黙って飲まず、件数を人間の Approve ダイアログまで持ち上げる。
  serious_n="$(jq -r '[.backlog[] | select(.severity == "BLOCKER" or .severity == "MAJOR")] | length' <<<"$judged")"
  blocker_n="$(jq -r '[.backlog[] | select(.severity == "BLOCKER")] | length' <<<"$judged")"
  closer_note="前ラウンドの指摘はすべて解消 / 却下されました（closer ラウンド ${round}/${MAX_REVIEWS}）。"
  if [[ "$serious_n" != "0" ]]; then
    closer_note="${closer_note}closer は carry-over 判定専用なので、新たに報告された BLOCKER/MAJOR ${serious_n} 件（うち BLOCKER ${blocker_n} 件）は gate 対象にせず backlog に退避しました。実装前に $BACKLOG_FILE を確認してください。"
  fi
  pass_through "Copilot プランレビュー: ${closer_note}backlog は計 ${backlog_n} 件。全文: $review_log${warn:+

（注意: $warn）}"
fi

pass_through "Copilot プランレビュー: 実装をブロックする指摘はありません（ラウンド ${round}/${MAX_REVIEWS}）。MINOR/NIT ${backlog_n} 件は $BACKLOG_FILE に退避しました。全文: $review_log${warn:+

（注意: $warn）}"
