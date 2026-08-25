#!/usr/bin/env bash
# plan-view.sh — プランを Markdown レンダリング済みの HTML にして Chrome の専用窓へ飛ばす。
#
# 設計と根拠: docs/plan-view.md（このリポジトリ内）
#
# プランは端末内のプレーンテキストとして読まされる。この repo のプランは 1 万字前後
# あり、plan-review gate が deny するたび書き直された版を読み直すことになる。長文を
# 未レンダリングのまま読むのが苦痛だという体験の問題を、機械変換だけで解く。
#
# **構造の再解釈はしない。** 入力の Markdown を 1:1 で HTML に写すだけである。LLM は
# 一切呼ばない（毎回のトークンコストと待ち時間に見合わない）。
#
# 使い方:
#   hook として:  settings.json の PreToolUse (matcher: ExitPlanMode) から stdin JSON で呼ばれる
#   CLI として:   plan-view [FILE|-] [--title T] [--no-open] [--out PATH]
#                 FILE 省略時は ~/.claude/plans/ の最新を使う
#   自己検査:     plan-view --selftest
#
# hook モードの不変条件（selftest が守る）:
#
#   stdout に何も出さない。
#
#   PreToolUse hook が stdout に JSON を出すと permissionDecision として解釈される。
#   このフックは承認フローに一切干渉してはならない（表示するだけの道具である）ので、
#   成功しようが失敗しようが無音で exit 0 する。既存の codex-plan-review gate とは
#   完全に独立した別エントリとして登録され、並列に走る。
#
# スキップ手段:
#   touch ~/.claude/plan-views/skip   または   PLAN_VIEW_SKIP=1
#
# 既知の罠:
#   - ブラウザを同期起動すると承認ダイアログが出ない。PreToolUse hook は同期実行で、
#     既存 Chrome プロセスが無いホストでは Chrome 本体が hook を掴んだまま居座る。
#     必ず setsid + & + disown で切り離すこと。
#   - pandoc の組み込み CSS と skylighting の色は自前 CSS より前に出る。上書きは
#     --include-in-header に依存している（plan-view.css の先頭コメント参照）。
#   - --metadata pagetitle= は <title> だけを設定する。--metadata title= にすると
#     pandoc が title-block を描き、本文の h1 と二重になる。
set -u

# 生成物はプラン本文そのものを含むため、既定 umask に任せず 0600 / 0700 に落とす。
umask 077

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"

VIEW_DIR="${PLAN_VIEW_DIR:-$HOME/.claude/plan-views}"
PANDOC_BIN="${PLAN_VIEW_PANDOC:-pandoc}"
BROWSER_BIN="${PLAN_VIEW_BROWSER:-google-chrome}"
HIGHLIGHT_STYLE="${PLAN_VIEW_HIGHLIGHT:-breezeDark}"
WINDOW_SIZE="${PLAN_VIEW_WINDOW_SIZE:-1000,900}"
RETENTION_DAYS="${PLAN_VIEW_RETENTION_DAYS:-30}"
CSS_OVERRIDE="${PLAN_VIEW_CSS:-}"

ensure_dirs() {
  mkdir -p "$VIEW_DIR"
  chmod 700 "$VIEW_DIR" 2>/dev/null || true
  # 緩い権限で残っている生成物を締める（skip は残置してよい空ファイル）。
  find "$VIEW_DIR" -maxdepth 1 -type f ! -name skip -perm /077 \
    -exec chmod 600 {} + 2>/dev/null || true
}

# 保持期限より古い生成物を掃除する。skip はエスケープハッチなので絶対に消さない
# （消すと無効化が黙って解除される）。
prune_old() {
  case "$RETENTION_DAYS" in
    '' | 0 | *[!0-9]*) return 0 ;;
  esac
  find "$VIEW_DIR" -maxdepth 1 -type f ! -name skip -mtime "+$RETENTION_DAYS" \
    -delete 2>/dev/null || true
}

# ---------------------------------------------------------------------------
# 素材の組み立て
# ---------------------------------------------------------------------------

# CSS の探索。配備後は ~/.claude/hooks/ に .sh と .css が並ぶので SCRIPT_DIR 相対で
# 足りる（リポジトリ上でも同じ隣接関係を保っている）。見つからなければ pandoc の
# 既定スタイルのまま描く — CSS の不在で描画そのものを落とすのは筋が悪い。
find_css() {
  local c
  for c in "$CSS_OVERRIDE" "$SCRIPT_DIR/plan-view.css" \
    "$HOME/.claude/hooks/plan-view.css"; do
    if [[ -n "$c" && -r "$c" ]]; then
      printf '%s' "$c"
      return 0
    fi
  done
  return 1
}

html_escape() {
  sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g'
}

# 本文の最初の h1 を返す（無ければ空文字）。
#
# フェンスの内側は見ない。プランは bash ブロックを多用し、その中の `# コメント` が
# h1 と同じ形をしているため、素朴な grep だとコメントをタイトルに採ってしまう。
extract_h1() { # $1=md file
  awk '
    /^[[:space:]]*(```|~~~)/ { fence = !fence; next }
    !fence && /^#[[:space:]]+/ {
      sub(/^#[[:space:]]+/, "")
      sub(/[[:space:]]+$/, "")
      print
      exit
    }
  ' "$1" 2>/dev/null
}

# メタ行（リポジトリ名 · 時刻）を h1 の直下に差し込む。h1 が無ければ先頭に置く。
# gfm リーダは raw HTML を素通しするので、クラス付きの <p> をそのまま書ける。
insert_meta() { # $1=md file $2=meta html ; stdout: 加工後の markdown
  local src="$1" meta="$2"
  if [[ -z "$(extract_h1 "$src")" ]]; then
    printf '%s\n\n' "$meta"
    cat "$src"
    return 0
  fi
  awk -v meta="$meta" '
    /^[[:space:]]*(```|~~~)/ { fence = !fence; print; next }
    {
      print
      if (!done && !fence && $0 ~ /^#[[:space:]]+/) {
        print ""
        print meta
        done = 1
      }
    }
  ' "$src"
}

# ---------------------------------------------------------------------------
# レンダリングと発射
# ---------------------------------------------------------------------------

# $1=src md $2=title $3=out html
render_html() {
  local src="$1" title="$2" out="$3" css hdr="" rc
  local -a args=(
    --from=gfm --to=html5 --standalone
    --highlight-style="$HIGHLIGHT_STYLE"
    --metadata pagetitle="$title"
  )
  if css="$(find_css)"; then
    # --css= で <link> にすると単一ファイルにならない。<style> に包んで
    # --include-in-header で渡すと、pandoc の組み込み CSS と skylighting の色より
    # 後ろに入り、かつ 1 ファイルで完結する（--embed-resources 不要 = pandoc の
    # バージョン差に強い）。
    hdr="$(mktemp "$VIEW_DIR/.hdr.XXXXXX.html")" || return 1
    {
      printf '<style>\n'
      cat "$css"
      printf '\n</style>\n'
    } > "$hdr"
    args+=(--include-in-header="$hdr")
  fi
  "$PANDOC_BIN" "${args[@]}" -o "$out" "$src" >/dev/null 2>&1
  rc=$?
  [[ -n "$hdr" ]] && rm -f "$hdr"
  return "$rc"
}

# ブラウザを切り離して起動し、即座に戻る。ここを同期にすると承認ダイアログが出ない。
open_window() { # $1=html path
  local url="file://$1"
  if command -v setsid >/dev/null 2>&1; then
    setsid "$BROWSER_BIN" --app="$url" --window-size="$WINDOW_SIZE" \
      >/dev/null 2>&1 </dev/null &
  else
    "$BROWSER_BIN" --app="$url" --window-size="$WINDOW_SIZE" \
      >/dev/null 2>&1 </dev/null &
  fi
  disown 2>/dev/null || true
  return 0
}

has_display() {
  [[ -n "${DISPLAY:-}" || -n "${WAYLAND_DISPLAY:-}" ]]
}

latest_plan() {
  ls -t "$HOME/.claude/plans/"*.md 2>/dev/null | head -1
}

# プラン 1 本を HTML にして（必要なら）発射する。
# $1=src md $2=title override(空可) $3=meta label(空可) $4=out html $5=open? (1/0)
view_plan() {
  local src="$1" title="$2" label="$3" out="$4" want_open="$5"
  local meta tmp

  if [[ -z "$title" ]]; then
    title="$(extract_h1 "$src")"
  fi
  if [[ -z "$title" ]]; then
    title="$(basename -- "$src")"
    title="${title%.md}"
  fi

  meta="<p class=\"plan-view-meta\">"
  if [[ -n "$label" ]]; then
    meta+="$(printf '%s' "$label" | html_escape) · "
  fi
  meta+="$(date '+%Y-%m-%d %H:%M')</p>"

  tmp="$(mktemp "$VIEW_DIR/.plan.XXXXXX.md")" || return 1
  insert_meta "$src" "$meta" > "$tmp"

  if ! render_html "$tmp" "$title" "$out"; then
    rm -f "$tmp"
    return 1
  fi
  rm -f "$tmp"
  chmod 600 "$out" 2>/dev/null || true

  [[ "$want_open" == "1" ]] && open_window "$out"
  return 0
}

# ---------------------------------------------------------------------------
# selftest
# ---------------------------------------------------------------------------

if [[ "${1:-}" == "--selftest" ]]; then
  selftest_fail=0
  selftest_dir="$(mktemp -d)"
  trap 'rm -rf "$selftest_dir"' EXIT

  VIEW_DIR="$selftest_dir/views"
  ensure_dirs

  ok() { printf '  ok   %s\n' "$1"; }
  ng() {
    printf '  FAIL %s\n' "$1"
    selftest_fail=1
  }
  check() { # $1=label $2=expected $3=actual
    if [[ "$2" == "$3" ]]; then ok "$1"; else ng "$1 (expected=$2 actual=$3)"; fi
  }

  have_pandoc=0
  command -v "$PANDOC_BIN" >/dev/null 2>&1 && have_pandoc=1

  # 偽ブラウザ: 渡された引数を記録するだけ。実際に窓は開かない。
  fake_browser="$selftest_dir/fake-browser"
  browser_log="$selftest_dir/browser.log"
  cat > "$fake_browser" <<'FAKEEOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$FAKE_BROWSER_LOG"
exit 0
FAKEEOF
  chmod +x "$fake_browser"

  echo "タイトル抽出:"

  t1="$selftest_dir/t1.md"
  printf '# 本当のタイトル\n\n本文\n' > "$t1"
  check "h1 を採る" "本当のタイトル" "$(extract_h1 "$t1")"

  # フェンス内の `# コメント` を拾わないこと。プランは bash ブロックを多用するので
  # これを落とすとタイトルがコードのコメントになる。
  t2="$selftest_dir/t2.md"
  printf '```bash\n# これはコメント\necho hi\n```\n\n# 本当のタイトル\n' > "$t2"
  check "フェンス内の # を拾わない" "本当のタイトル" "$(extract_h1 "$t2")"

  t3="$selftest_dir/t3.md"
  printf '## Context\n\n見出しは h2 から\n' > "$t3"
  check "h1 なしなら空" "" "$(extract_h1 "$t3")"

  t4="$selftest_dir/t4.md"
  printf '#!/usr/bin/env bash\n\n# 本当のタイトル\n' > "$t4"
  check "shebang を h1 と誤認しない" "本当のタイトル" "$(extract_h1 "$t4")"

  echo "メタ行の挿入:"

  meta_probe='<p class="plan-view-meta">repo · now</p>'
  # h1(1) / 空行(2) / メタ行(3)。空行は必須で、詰めると gfm リーダが raw HTML を
  # 独立したブロックとして扱わず h1 の続きに飲み込む。
  out="$(insert_meta "$t1" "$meta_probe")"
  check "h1 の直後（空行を挟んで）に入る" "3" \
    "$(printf '%s\n' "$out" | grep -n 'plan-view-meta' | cut -d: -f1)"
  check "h1 とメタ行の間に空行が入る" "" "$(printf '%s\n' "$out" | sed -n '2p')"
  check "h1 は消えない" 1 "$(printf '%s\n' "$out" | grep -c '^# 本当のタイトル$')"

  out="$(insert_meta "$t3" "$meta_probe")"
  check "h1 なしなら先頭に入る" "1" \
    "$(printf '%s\n' "$out" | grep -n 'plan-view-meta' | cut -d: -f1)"
  check "h1 なしでも本文は残る" 1 "$(printf '%s\n' "$out" | grep -c '^## Context$')"

  out="$(insert_meta "$t2" "$meta_probe")"
  check "フェンスを跨いでも 1 回だけ挿入" 1 \
    "$(printf '%s\n' "$out" | grep -c 'plan-view-meta')"

  echo "CSS の解決:"

  css_found=""
  css_found="$(find_css || true)"
  if [[ -n "$css_found" && -r "$css_found" ]]; then
    ok "SCRIPT_DIR 隣接の CSS が見つかる ($(basename -- "$css_found"))"
  else
    ng "SCRIPT_DIR 隣接の CSS が見つかる"
  fi

  # CSS は style 要素の中に埋め込まれる。ブラウザは style 要素の中では CSS の
  # コメント構文を解釈せず、終了タグの並びを見た時点で要素を閉じるので、コメントに
  # 書いただけでもそこから下の CSS 全部が本文に漏れて表示される。実際に一度これで
  # 壊した。CSS 側にその並びが無いことを検査する。
  if [[ -n "$css_found" ]] && grep -qiE '</[[:space:]]*style' "$css_found"; then
    ng "CSS に style 要素の終了タグが混じっていない"
  else
    ok "CSS に style 要素の終了タグが混じっていない"
  fi

  # skylighting はクラスごとの色のほかに `code span` (Normal) と `div.sourceCode` に
  # 基底色 #cfcfc2 を置く。ライト側でこの 2 つを上書きしないと、span の付かない
  # 素のトークンが白背景で消える。実測した欠陥の回帰テスト。
  if [[ -n "$css_found" ]] && grep -qE '^[[:space:]]*code span \{' "$css_found" &&
    grep -qE '^[[:space:]]*div\.sourceCode,' "$css_found"; then
    ok "ライト側で skylighting の基底色を上書きしている"
  else
    ng "ライト側で skylighting の基底色を上書きしている"
  fi

  echo "hook 経路:"

  fixture="$selftest_dir/input.json"
  mkhookinput() { # $1=session_id $2=plan text (空なら plan なし) $3=planFilePath (空可)
    jq -n --arg s "$1" --arg p "$2" --arg f "$3" --arg c "$selftest_dir" \
      '{session_id: $s, cwd: $c, hook_event_name: "PreToolUse",
        tool_name: "ExitPlanMode", permission_mode: "plan",
        tool_input: ({}
          + (if $p == "" then {} else {plan: $p} end)
          + (if $f == "" then {} else {planFilePath: $f} end))}' > "$fixture"
  }
  runhook() { # 残りの引数は env 代入として渡す
    env PLAN_VIEW_DIR="$VIEW_DIR" PLAN_VIEW_BROWSER="$fake_browser" \
      FAKE_BROWSER_LOG="$browser_log" DISPLAY=":99" \
      "$@" bash "${BASH_SOURCE[0]}" < "$fixture"
  }
  reset_probe() {
    : > "$browser_log"
    rm -f "$VIEW_DIR"/*.html 2>/dev/null || true
  }
  html_count() { find "$VIEW_DIR" -maxdepth 1 -name '*.html' | wc -l | tr -d ' '; }
  # grep -c は 0 件のとき「0 を出力しつつ exit 1」なので、`|| echo 0` と書くと
  # 出力が "0\n0" になる。代入の失敗でだけ 0 に落とす。
  browser_calls() {
    local n
    n="$(grep -c . "$browser_log" 2>/dev/null)" || n=0
    printf '%s' "${n:-0}"
  }

  if [[ "$have_pandoc" == "1" ]]; then
    # 1) tool_input.plan から本文を取る
    reset_probe
    # コードブロックを必ず含めること。pandoc は本文にハイライト対象が無いと
    # skylighting の CSS を一切出さないので、下の順序テストが空振りする。
    mkhookinput s-plan '# インラインのプラン

本文である。

```bash
if true; then echo hi; fi
```
' ''
    hook_out="$(runhook)"
    check "tool_input.plan → stdout は空" "" "$hook_out"
    check "tool_input.plan → HTML が 1 本できる" 1 "$(html_count)"
    check "tool_input.plan → ブラウザを 1 回呼ぶ" 1 "$(browser_calls)"
    case "$(cat "$browser_log")" in
      *--app=file://*) ok "ブラウザに --app=file:// が渡る" ;;
      *) ng "ブラウザに --app=file:// が渡る ($(cat "$browser_log"))" ;;
    esac
    case "$(cat "$browser_log")" in
      *--window-size=*) ok "ブラウザに --window-size が渡る" ;;
      *) ng "ブラウザに --window-size が渡る" ;;
    esac
    generated="$(find "$VIEW_DIR" -maxdepth 1 -name '*.html' | head -1)"
    check "生成物の権限" 600 "$(stat -c '%a' "$generated")"
    check "VIEW_DIR の権限" 700 "$(stat -c '%a' "$VIEW_DIR")"
    case "$(cat "$generated")" in
      *"<title>インラインのプラン</title>"*) ok "h1 が <title> になる" ;;
      *) ng "h1 が <title> になる" ;;
    esac
    if grep -q 'title-block-header' "$generated"; then
      ng "title-block で h1 が二重にならない"
    else
      ok "title-block で h1 が二重にならない"
    fi
    if grep -q 'plan-view-meta' "$generated"; then
      ok "メタ行が HTML に入る"
    else
      ng "メタ行が HTML に入る"
    fi
    # 自前 CSS が skylighting より後ろに来ていること（順序依存の回帰テスト）。
    # マーカーは双方に固有のものを使う: `/* Keyword */` は skylighting が付ける
    # コメントで、`--measure:` は自前 CSS だけが定義するカスタムプロパティ。
    # `code span.kw` を目印にすると自前 CSS 側の上書き規則にも当たって空振りする。
    hl_line="$(grep -n '/\* Keyword \*/' "$generated" | head -1 | cut -d: -f1)"
    own_line="$(grep -n -- '--measure:' "$generated" | head -1 | cut -d: -f1)"
    if [[ -n "$hl_line" && -n "$own_line" && "$own_line" -gt "$hl_line" ]]; then
      ok "自前 CSS が skylighting より後ろに入る"
    else
      ng "自前 CSS が skylighting より後ろに入る (hl=$hl_line own=$own_line)"
    fi
    # CSS が本文に漏れていないこと。style 要素が本文の前で閉じ切っていれば、
    # CSS はスタイルとして生きている。漏れると CSS 全文がページに文字として出る
    # （grep ベースの検査は「文字列が在る」しか見ないので全部通ってしまう）。
    last_style_end="$(grep -n '</style>' "$generated" | tail -1 | cut -d: -f1)"
    body_line="$(grep -n '<body' "$generated" | head -1 | cut -d: -f1)"
    if [[ -n "$last_style_end" && -n "$body_line" && "$last_style_end" -lt "$body_line" ]]; then
      ok "style 要素が本文の前で閉じている（CSS が本文に漏れていない）"
    else
      ng "style 要素が本文の前で閉じている (last </style>=$last_style_end body=$body_line)"
    fi

    # ライト側のハイライト上書きが本当に載っていること。ここが抜けると白背景で
    # keyword/operator が消える（breezeDark の #cfcfc2）。
    if grep -q 'prefers-color-scheme: light' "$generated" &&
      grep -q 'code span.kw' "$generated"; then
      ok "ライト用のハイライト上書きが載る"
    else
      ng "ライト用のハイライト上書きが載る"
    fi

    # 2) planFilePath から取る
    reset_probe
    pf="$selftest_dir/from-file.md"
    printf '# ファイル経由のプラン\n\n本文\n' > "$pf"
    mkhookinput s-file '' "$pf"
    hook_out="$(runhook)"
    check "planFilePath → stdout は空" "" "$hook_out"
    check "planFilePath → HTML ができる" 1 "$(html_count)"
    generated="$(find "$VIEW_DIR" -maxdepth 1 -name '*.html' | head -1)"
    case "$(cat "$generated")" in
      *"<title>ファイル経由のプラン</title>"*) ok "planFilePath の h1 を採る" ;;
      *) ng "planFilePath の h1 を採る" ;;
    esac

    # 3) どちらも無ければ ~/.claude/plans/ の最新
    reset_probe
    fake_home="$selftest_dir/home"
    mkdir -p "$fake_home/.claude/plans"
    printf '# 最新のプラン\n\n本文\n' > "$fake_home/.claude/plans/newest.md"
    mkhookinput s-latest '' ''
    hook_out="$(runhook HOME="$fake_home")"
    check "plans/ 最新 → stdout は空" "" "$hook_out"
    check "plans/ 最新 → HTML ができる" 1 "$(html_count)"

    # 4) 取得元が何も無い → 無音で何もしない
    reset_probe
    empty_home="$selftest_dir/empty-home"
    mkdir -p "$empty_home/.claude/plans"
    mkhookinput s-none '' ''
    hook_out="$(runhook HOME="$empty_home")"
    check "取得元なし → stdout は空" "" "$hook_out"
    check "取得元なし → HTML を作らない" 0 "$(html_count)"
    check "取得元なし → ブラウザを呼ばない" 0 "$(browser_calls)"
  else
    echo "  skip pandoc 不在のため HTML 生成系のケースをスキップ"
  fi

  echo "無効化と縮退:"

  # 5) PLAN_VIEW_SKIP=1
  reset_probe
  mkhookinput s-skip '# skip されるプラン
' ''
  hook_out="$(runhook PLAN_VIEW_SKIP=1)"
  check "PLAN_VIEW_SKIP=1 → stdout は空" "" "$hook_out"
  check "PLAN_VIEW_SKIP=1 → HTML を作らない" 0 "$(html_count)"
  check "PLAN_VIEW_SKIP=1 → ブラウザを呼ばない" 0 "$(browser_calls)"

  # 6) skip フラグファイル
  reset_probe
  : > "$VIEW_DIR/skip"
  hook_out="$(runhook)"
  check "skip フラグ → stdout は空" "" "$hook_out"
  check "skip フラグ → HTML を作らない" 0 "$(html_count)"
  check "skip フラグ → ブラウザを呼ばない" 0 "$(browser_calls)"
  rm -f "$VIEW_DIR/skip"

  # 7) pandoc 不在
  reset_probe
  hook_out="$(runhook PLAN_VIEW_PANDOC=definitely-not-a-real-binary)"
  check "pandoc 不在 → stdout は空" "" "$hook_out"
  check "pandoc 不在 → HTML を作らない" 0 "$(html_count)"
  check "pandoc 不在 → ブラウザを呼ばない" 0 "$(browser_calls)"

  # 8) ブラウザ不在
  reset_probe
  hook_out="$(runhook PLAN_VIEW_BROWSER=definitely-not-a-real-browser)"
  check "ブラウザ不在 → stdout は空" "" "$hook_out"
  check "ブラウザ不在 → HTML を作らない" 0 "$(html_count)"

  # 9) DISPLAY / WAYLAND_DISPLAY の双方なし（SSH 経由セッション）
  reset_probe
  hook_out="$(env -u DISPLAY -u WAYLAND_DISPLAY \
    PLAN_VIEW_DIR="$VIEW_DIR" PLAN_VIEW_BROWSER="$fake_browser" \
    FAKE_BROWSER_LOG="$browser_log" \
    bash "${BASH_SOURCE[0]}" < "$fixture")"
  check "DISPLAY なし → stdout は空" "" "$hook_out"
  check "DISPLAY なし → HTML を作らない" 0 "$(html_count)"
  check "DISPLAY なし → ブラウザを呼ばない" 0 "$(browser_calls)"

  echo "CLI 経路:"

  if [[ "$have_pandoc" == "1" ]]; then
    reset_probe
    cli_out="$selftest_dir/cli.html"
    if env PLAN_VIEW_DIR="$VIEW_DIR" PLAN_VIEW_BROWSER="$fake_browser" \
      FAKE_BROWSER_LOG="$browser_log" \
      bash "${BASH_SOURCE[0]}" --no-open --out "$cli_out" "$t1" >/dev/null 2>&1; then
      ok "CLI: --no-open --out が成功する"
    else
      ng "CLI: --no-open --out が成功する"
    fi
    if [[ -s "$cli_out" ]]; then
      ok "CLI: --out の HTML ができる"
    else
      ng "CLI: --out の HTML ができる"
    fi
    check "CLI: --no-open ならブラウザを呼ばない" 0 "$(browser_calls)"

    reset_probe
    cli_out2="$selftest_dir/cli-stdin.html"
    if printf '# 標準入力のプラン\n\n本文\n' \
      | env PLAN_VIEW_DIR="$VIEW_DIR" PLAN_VIEW_BROWSER="$fake_browser" \
        FAKE_BROWSER_LOG="$browser_log" \
        bash "${BASH_SOURCE[0]}" --no-open --out "$cli_out2" - >/dev/null 2>&1; then
      ok "CLI: stdin (-) から読める"
    else
      ng "CLI: stdin (-) から読める"
    fi
    case "$(cat "$cli_out2" 2>/dev/null)" in
      *"<title>標準入力のプラン</title>"*) ok "CLI: stdin でも h1 を採る" ;;
      *) ng "CLI: stdin でも h1 を採る" ;;
    esac

    reset_probe
    cli_out3="$selftest_dir/cli-title.html"
    env PLAN_VIEW_DIR="$VIEW_DIR" bash "${BASH_SOURCE[0]}" \
      --no-open --title '明示タイトル' --out "$cli_out3" "$t1" >/dev/null 2>&1
    case "$(cat "$cli_out3" 2>/dev/null)" in
      *"<title>明示タイトル</title>"*) ok "CLI: --title が h1 より優先される" ;;
      *) ng "CLI: --title が h1 より優先される" ;;
    esac
  else
    echo "  skip pandoc 不在のため CLI の生成系ケースをスキップ"
  fi

  # CLI は hook と違って黙ってはいけない。存在しないファイルは非ゼロで落ちる。
  if env PLAN_VIEW_DIR="$VIEW_DIR" bash "${BASH_SOURCE[0]}" \
    --no-open /definitely/not/a/file.md >/dev/null 2>&1; then
    ng "CLI: 存在しないファイルは非ゼロで落ちる"
  else
    ok "CLI: 存在しないファイルは非ゼロで落ちる"
  fi

  if env PLAN_VIEW_DIR="$VIEW_DIR" bash "${BASH_SOURCE[0]}" \
    --unknown-flag >/dev/null 2>&1; then
    ng "CLI: 未知のフラグは非ゼロで落ちる"
  else
    ok "CLI: 未知のフラグは非ゼロで落ちる"
  fi

  echo "ログ衛生:"

  touch -d '40 days ago' "$VIEW_DIR/20250101-000000-deadbeef.html"
  touch -d '40 days ago' "$VIEW_DIR/skip"
  prune_old
  if [[ -e "$VIEW_DIR/20250101-000000-deadbeef.html" ]]; then
    ng "保持期限より古い生成物が掃除される"
  else
    ok "保持期限より古い生成物が掃除される"
  fi
  if [[ -e "$VIEW_DIR/skip" ]]; then
    ok "skip フラグは掃除されない"
  else
    ng "skip フラグは掃除されない"
  fi
  rm -f "$VIEW_DIR/skip"

  echo
  if [[ "$selftest_fail" == "0" ]]; then
    echo "selftest: すべて通過"
    exit 0
  fi
  echo "selftest: 失敗あり" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# CLI モード（引数があれば CLI。無ければ hook として stdin を読む）
# ---------------------------------------------------------------------------

if [[ $# -gt 0 ]]; then
  cli_src=""
  cli_title=""
  cli_out=""
  cli_open=1

  die() {
    printf 'plan-view: %s\n' "$1" >&2
    exit 1
  }

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --title)
        [[ $# -ge 2 ]] || die "--title に値がありません"
        cli_title="$2"
        shift 2
        ;;
      --out)
        [[ $# -ge 2 ]] || die "--out に値がありません"
        cli_out="$2"
        shift 2
        ;;
      --no-open)
        cli_open=0
        shift
        ;;
      -h | --help)
        cat <<'USAGE'
使い方: plan-view [FILE|-] [--title T] [--no-open] [--out PATH]

  FILE      Markdown ファイル。`-` で標準入力。省略時は ~/.claude/plans/ の最新。
  --title   <title> と表示タイトル。既定は本文の最初の h1、無ければファイル名。
  --no-open ブラウザを開かず HTML を作るだけ。
  --out     HTML の出力先。既定は ~/.claude/plan-views/ 配下。

環境変数: PLAN_VIEW_DIR PLAN_VIEW_PANDOC PLAN_VIEW_BROWSER PLAN_VIEW_CSS
          PLAN_VIEW_HIGHLIGHT PLAN_VIEW_WINDOW_SIZE PLAN_VIEW_RETENTION_DAYS
無効化:   PLAN_VIEW_SKIP=1 または ~/.claude/plan-views/skip
USAGE
        exit 0
        ;;
      --)
        shift
        [[ $# -gt 0 ]] && cli_src="$1"
        break
        ;;
      # `-` は標準入力を意味する。下の -*) より前に置くこと（後ろに置くと
      # 「未知のフラグ」で落ちる）。
      -)
        [[ -z "$cli_src" ]] || die "ファイルは 1 つだけ指定してください"
        cli_src="-"
        shift
        ;;
      -*)
        die "未知のフラグ: $1"
        ;;
      *)
        [[ -z "$cli_src" ]] || die "ファイルは 1 つだけ指定してください"
        cli_src="$1"
        shift
        ;;
    esac
  done

  command -v "$PANDOC_BIN" >/dev/null 2>&1 ||
    die "pandoc が見つかりません (PLAN_VIEW_PANDOC=$PANDOC_BIN)"

  ensure_dirs
  prune_old

  cli_tmp=""
  if [[ "$cli_src" == "-" ]]; then
    cli_tmp="$(mktemp "$VIEW_DIR/.stdin.XXXXXX.md")" || die "一時ファイルを作れません"
    cat > "$cli_tmp"
    cli_src="$cli_tmp"
  elif [[ -z "$cli_src" ]]; then
    cli_src="$(latest_plan)"
    [[ -n "$cli_src" ]] || die "~/.claude/plans/ にプランがありません"
  fi
  [[ -f "$cli_src" ]] || die "ファイルがありません: $cli_src"

  if [[ -z "$cli_out" ]]; then
    cli_out="$VIEW_DIR/cli-$(date +%Y%m%d-%H%M%S).html"
  fi

  if [[ "$cli_open" == "1" ]]; then
    command -v "$BROWSER_BIN" >/dev/null 2>&1 ||
      die "ブラウザが見つかりません (PLAN_VIEW_BROWSER=$BROWSER_BIN)"
    has_display ||
      die "DISPLAY / WAYLAND_DISPLAY がありません（--no-open なら HTML だけ作れます）"
  fi

  cli_label="$(basename -- "$PWD")"
  if ! view_plan "$cli_src" "$cli_title" "$cli_label" "$cli_out" "$cli_open"; then
    [[ -n "$cli_tmp" ]] && rm -f "$cli_tmp"
    die "HTML の生成に失敗しました"
  fi
  [[ -n "$cli_tmp" ]] && rm -f "$cli_tmp"

  printf '%s\n' "$cli_out"
  exit 0
fi

# ---------------------------------------------------------------------------
# hook モード
# ---------------------------------------------------------------------------
#
# ここから下は何があっても stdout に書かず exit 0 する。stdout に JSON を出すと
# permissionDecision として解釈され、承認フローに干渉してしまう。

INPUT="$(cat)"

# --- エスケープハッチ ---
if [[ -e "$VIEW_DIR/skip" || "${PLAN_VIEW_SKIP:-0}" == "1" ]]; then
  exit 0
fi

# --- バイナリ存在でゲート（ADR-0005）。認証情報ではなく存在だけを見る ---
command -v "$PANDOC_BIN" >/dev/null 2>&1 || exit 0
command -v "$BROWSER_BIN" >/dev/null 2>&1 || exit 0

# --- 画面が無いセッション（SSH 経由など）では何もしない ---
has_display || exit 0

ensure_dirs
prune_old

SESSION_ID="$(jq -r '.session_id // "unknown"' <<<"$INPUT" 2>/dev/null || echo unknown)"
SESSION_ID="${SESSION_ID//[^A-Za-z0-9._-]/_}"
CWD="$(jq -r '.cwd // empty' <<<"$INPUT" 2>/dev/null || echo '')"
[[ -d "$CWD" ]] || CWD="$HOME"

# --- プラン本文の取得: tool_input.plan → planFilePath → 最新の ~/.claude/plans/*.md ---
# 既存の codex-plan-review.sh と同じ順序。実測では ExitPlanMode で plan と
# planFilePath の両方が来るが、片方だけのケースに備えて 3 段で落とす。
plan_tmp=""
plan_file=""
plan_text="$(jq -r '.tool_input.plan // empty' <<<"$INPUT" 2>/dev/null || echo '')"
plan_path="$(jq -r '.tool_input.planFilePath // empty' <<<"$INPUT" 2>/dev/null || echo '')"
if [[ -n "$plan_text" ]]; then
  plan_tmp="$(mktemp "$VIEW_DIR/.hook.XXXXXX.md")" || exit 0
  printf '%s\n' "$plan_text" > "$plan_tmp"
  plan_file="$plan_tmp"
elif [[ -n "$plan_path" && -f "$plan_path" ]]; then
  plan_file="$plan_path"
else
  plan_file="$(latest_plan)"
  [[ -n "$plan_file" ]] || exit 0
fi

out_file="$VIEW_DIR/$(date +%Y%m%d-%H%M%S)-${SESSION_ID:0:8}.html"

view_plan "$plan_file" "" "$(basename -- "$CWD")" "$out_file" 1 || true

[[ -n "$plan_tmp" ]] && rm -f "$plan_tmp"

exit 0
