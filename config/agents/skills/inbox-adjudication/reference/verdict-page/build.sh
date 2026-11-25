#!/usr/bin/env bash
# build.sh — メール裁定ページの生成器。項目の表(その回のデータ)を、形式の正本
# (template.html)に差し込んで、完成した HTML を出す。
#
# 使い方:
#   build.sh --data <items.json> [--out <file>]   検証して HTML を標準出力(または --out)へ
#   build.sh --selftest                           架空の fixture で自己検査する
#
# 手順:
#   1. 入力を items.schema.json で検証する(schema-validate.jq。対応外のキーワードは違反扱い)。
#      JSON Schema で書けない items[].id と items[].key の一意性も、ここで検査する。
#   2. template.html の __FORMAT_VERSION__ を入力の format_version に、__DATA__ を入力の JSON に
#      置き換える。JSON の < は < に換える(</script> や <!-- で script が途切れない)。
#
# 依存: bash, jq。--selftest はさらに node(生成物の script の構文検査)を使う。
# 形式を変えるときの決まりは FORMAT.md。保存先の形は verdicts.schema.json。
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEMPLATE="$HERE/template.html"
ITEMS_SCHEMA="$HERE/items.schema.json"
VERDICTS_SCHEMA="$HERE/verdicts.schema.json"
OPTIONS_FILE="$HERE/options.json"
VALIDATOR="$HERE/schema-validate.jq"

die() { echo "error: $*" >&2; exit 2; }

usage() {
  cat >&2 <<'EOF'
使い方:
  build.sh --data <items.json> [--out <file>]
  build.sh --selftest
EOF
}

# validate <schema> <instance>: 違反を標準エラーへ出し、適合なら 0、違反なら 1、読めなければ 2。
validate() {
  local out
  out=$(jq -nr --slurpfile s "$1" --slurpfile d "$2" -f "$VALIDATOR") || return 2
  if [ -n "$out" ]; then
    printf '%s\n' "$out" >&2
    return 1
  fi
}

# duplicates <items.json> <field>: items[].<field> で重複している値を 1 行ずつ出す。
duplicates() {
  jq -r --arg f "$2" '[.items[][$f]] | group_by(.) | map(select(length > 1) | .[0]) | .[]' "$1"
}

# verify_drafts <items.json>: drafts[] の下書きごとに `reply-coverage verify` を呼び、会話の相手の依頼すべてへの
# 応答を検査した最終稿であること(検査のあとに本文を直していないこと)を確かめる。検査していない下書きを
# 含むページは作らない。実行ファイルは環境変数 REPLY_COVERAGE_BIN で差し替えられる(--selftest が使う)。
verify_drafts() {
  local data=$1 bin="${REPLY_COVERAGE_BIN:-reply-coverage}" n i thread file
  n=$(jq '(.drafts // []) | length' "$data")
  [ "$n" -gt 0 ] || return 0
  if ! command -v "$bin" >/dev/null 2>&1; then
    echo "error: drafts[] があるのに $bin が見つかりません(返信の下書きは reply-coverage の検査が要ります)" >&2
    return 1
  fi
  for ((i = 0; i < n; i++)); do
    thread=$(jq -r ".drafts[$i].thread" "$data")
    file=$(mktemp)
    jq -j ".drafts[$i].text" "$data" > "$file"
    if ! "$bin" verify --thread "$thread" --draft "$file" 2> "$file.err"; then
      echo "error: drafts[$i](会話 $thread)は検査に合格していません: $(tr '\n' ' ' < "$file.err")" >&2
      rm -f "$file" "$file.err"
      return 1
    fi
    rm -f "$file" "$file.err"
  done
}

# build <items.json>: 完成した HTML を標準出力へ出す。
build() {
  local data=$1 field dup ph n
  [ -f "$data" ] || { echo "error: $data が読めません" >&2; return 2; }
  if ! validate "$ITEMS_SCHEMA" "$data"; then
    echo "error: $data は items.schema.json に適合しません" >&2
    return 1
  fi
  for field in id key; do
    dup=$(duplicates "$data" "$field")
    if [ -n "$dup" ]; then
      echo "error: items[].$field が重複しています: $dup" >&2
      return 1
    fi
  done
  # rec_option は、その項目の kind の選択肢にある id でなければならない(JSON Schema では書けない)。
  dup=$(jq -r --slurpfile o "$OPTIONS_FILE" '.items[] | . as $it | select($it.rec_option != null) | select(([$o[0].kinds[$it.kind].options[].id] | index($it.rec_option)) == null) | "\($it.id): \($it.kind) に \($it.rec_option) の選択肢がありません"' "$data")
  if [ -n "$dup" ]; then
    echo "error: items[].rec_option が選択肢にありません: $dup" >&2
    return 1
  fi
  # drafts[] の各下書きは、reply-coverage の検査に合格した最終稿でなければならない(#1577)。
  verify_drafts "$data" || return 1
  # 差し込み点は 1 か所ずつ(コメントなどに同じ語が残ると、置換が別の場所に当たる)。
  for ph in __DATA__ __OPTIONS__ __FORMAT_VERSION__; do
    n=$(grep -o "$ph" "$TEMPLATE" | wc -l || true)
    [ "$n" -eq 1 ] || die "template.html に $ph が $n 個ある(1 個でなければならない)"
  done
  # 版、選択肢、データの順に置き換える: データ側の文字列に差し込み点の語があっても、書き換わらない。
  jq -nj --rawfile tpl "$TEMPLATE" --slurpfile d "$data" --slurpfile o "$OPTIONS_FILE" '
    # 原文の全文(items[].source.text)は、紙と機体ローカルの PDF だけに載せる。HTML の DATA を作る前に落とす(FORMAT.md)。
    ($d[0] | (.items |= map(if has("source") then .source |= del(.text) else . end)) | tojson | gsub("<"; "\\u003c")) as $json
    | ($o[0] | del(.description) | tojson | gsub("<"; "\\u003c")) as $opts
    | $tpl
    | sub("__FORMAT_VERSION__"; ($d[0].format_version | tostring))
    | sub("__OPTIONS__"; $opts)
    | sub("__DATA__"; $json)'
}

# ---- selftest ----

PASS=0
FAIL=0
ok() { PASS=$((PASS + 1)); echo "ok   $1"; }
ng() { FAIL=$((FAIL + 1)); echo "FAIL $1" >&2; }
# expect_ok <名前> <コマンド...> / expect_fail <名前> <コマンド...>
expect_ok() { local name=$1; shift; if "$@" >/dev/null 2>&1; then ok "$name"; else ng "$name"; fi; }
expect_fail() { local name=$1; shift; if "$@" >/dev/null 2>&1; then ng "$name"; else ok "$name"; fi; }

# html_checks <名前> <生成された html> <元の入力 json>: 生成物の script が壊れていないことを確かめる。
html_checks() {
  local name=$1 html=$2 src=$3 js="$2.js" n ver
  n=$(grep -oi '</script' "$html" | wc -l || true)
  if [ "$n" -eq 1 ]; then ok "$name: </script が 1 個だけ"; else ng "$name: </script が $n 個ある"; fi
  n=$(grep -o '<!--' "$html" | wc -l || true)
  if [ "$n" -eq 0 ]; then ok "$name: <!-- が無い"; else ng "$name: <!-- が $n 個ある"; fi
  if [ "$name" = valid ] && grep -q '__DATA__\|__OPTIONS__\|__FORMAT_VERSION__' "$html"; then
    ng "$name: 差し込み点が残っている"
  else
    ok "$name: 差し込み点が残っていない"
  fi
  ver=$(jq -r '.properties.format_version.const' "$ITEMS_SCHEMA")
  if grep -qF "<meta name=\"verdict-page-format\" content=\"$ver\">" "$html"; then
    ok "$name: meta に版 $ver"
  else
    ng "$name: meta に版 $ver が無い"
  fi
  # script の中身を取り出し、JavaScript として構文を通す。
  awk '/^<script>$/{on=1; next} /^<\/script>$/{on=0} on' "$html" > "$js"
  if node --check "$js" 2>/dev/null; then ok "$name: script が JavaScript として通る"; else ng "$name: script の構文エラー"; fi
  # DATA の行を JSON として読み戻し、入力と同じであること(< が元に戻る)。
  if [ "$(grep '^const DATA = ' "$html" | sed 's/^const DATA = //; s/;$//' | jq -S .)" = "$(jq -S 'del(.items[].source.text)' "$src")" ]; then
    ok "$name: DATA が入力と一致する(原文の全文 source.text を除く)"
  else
    ng "$name: DATA が入力と違う"
  fi
  # 原文の全文は、HTML のどこにも載らない(入力に目印の文字列がある見本で確かめる)。
  if jq -e '[.items[].source.text // empty] | length > 0' "$src" >/dev/null 2>&1; then
    if grep -q 'FULLTEXT-SENTINEL-DO-NOT-RENDER' "$html"; then ng "$name: 原文の全文が HTML に載っている"; else ok "$name: 原文の全文は HTML に載らない"; fi
  fi
  # OPTIONS の行を JSON として読み戻し、options.json(description を除く)と同じであること。
  if [ "$(grep '^const OPTIONS = ' "$html" | sed 's/^const OPTIONS = //; s/;$//' | jq -S .)" = "$(jq -S 'del(.description)' "$OPTIONS_FILE")" ]; then
    ok "$name: OPTIONS が options.json と一致する"
  else
    ng "$name: OPTIONS が options.json と違う"
  fi
}

tmp=""
selftest() {
  command -v node >/dev/null 2>&1 || die "node が見つかりません(生成物の script の構文検査に使う)"
  local valid breaker f want got
  tmp=$(mktemp -d) # trap が関数の外で走るので、大域変数にする
  trap 'rm -rf "$tmp"' EXIT
  valid="$HERE/fixtures/valid.items.json"
  breaker="$HERE/fixtures/script-breaker.items.json"
  # 偽の reply-coverage: 本文に NOT-CHECKED を含む下書きだけを、検査していないものとして落とす。
  printf '#!/bin/sh\n[ "$1" = verify ] || exit 2\nwhile [ $# -gt 0 ]; do [ "$1" = --draft ] && f=$2; shift; done\n! grep -q NOT-CHECKED "$f"\n' > "$tmp/rc-stub"
  chmod +x "$tmp/rc-stub"
  export REPLY_COVERAGE_BIN="$tmp/rc-stub"

  # 1. 入力の検証
  expect_ok "入力: 適合する入力が通る" build "$valid"
  expect_ok "入力: script を壊す文字列を含む入力が通る" build "$breaker"
  mutate() { jq "$1" "$valid" > "$tmp/$2.json"; }
  mutate '.items[0].key = "x"' bad-key
  mutate '.items[0].key = "gmail:a.b/c"' bad-key-dot
  mutate '.items[1].id = .items[0].id' dup-id
  mutate '.items[1].key = .items[0].key' dup-key
  mutate 'del(.items[0].title)' missing-required
  mutate 'del(.run_label)' missing-run-label
  mutate '.extra = 1' extra-top
  mutate '.items[0].extra = 1' extra-item
  mutate '.format_version = 2' wrong-version
  mutate '.items[0].when_class = "red"' bad-when-class
  mutate '.items[0].id = "a b"' bad-id
  mutate '.drafts[0].key = "x"' bad-draft-key
  mutate 'del(.items[0].kind)' missing-kind
  mutate '.items[0].kind = "x"' bad-kind
  mutate '.items[0].rec_option = "nope"' bad-rec-option
  mutate '.items[0].rec_option = "send-me"' rec-option-of-other-kind
  mutate '.format_version = 4' old-version
  mutate '(.items[] | select(.kind == "fact") | .rec_option) = "do-me"' fact-rec-option-of-other-kind
  mutate '(.items[] | select(.kind == "report") | .rec_option) = "yes"' report-rec-option-of-fact
  mutate '.items[0].key = "gh:a.b/c"' bad-gh-key-dot
  mutate '.run_no = "a b"' bad-run-no
  mutate '.items[0].rev = "XYZ"' bad-rev
  mutate '.items[0].source = {ref: "x"}' source-without-sha
  mutate '(.items[] | select(.kind == "pr") | .rec_option) = "send-me"' pr-rec-option-of-other-kind
  mutate '.drafts[0].text = "NOT-CHECKED " + .drafts[0].text' unchecked-draft
  mutate 'del(.drafts[0].thread)' draft-without-thread
  mutate 'del(.drafts[0].asks)' draft-without-asks
  mutate '.drafts[0].thread = "x"' bad-draft-thread
  mutate '.drafts[0].asks[0].status = "unanswered"' unanswered-ask-in-draft
  mutate '.drafts[0].asks[0].extra = 1' extra-ask-field
  for f in unchecked-draft draft-without-thread draft-without-asks bad-draft-thread unanswered-ask-in-draft extra-ask-field; do
    expect_fail "入力: $f は落ちる" build "$tmp/$f.json"
  done
  expect_fail "入力: reply-coverage が無いと、drafts[] のあるページは作らない" env REPLY_COVERAGE_BIN="$tmp/none" bash "$0" --data "$valid"
  jq 'del(.drafts)' "$valid" > "$tmp/no-drafts.json"
  expect_ok "入力: drafts[] が無ければ reply-coverage は要らない" env REPLY_COVERAGE_BIN="$tmp/none" bash "$0" --data "$tmp/no-drafts.json"
  for f in bad-key bad-key-dot dup-id dup-key missing-required missing-run-label extra-top extra-item wrong-version bad-when-class bad-id bad-draft-key missing-kind bad-kind bad-rec-option rec-option-of-other-kind old-version fact-rec-option-of-other-kind report-rec-option-of-fact bad-gh-key-dot bad-run-no bad-rev source-without-sha pr-rec-option-of-other-kind; do
    expect_fail "入力: $f は落ちる" build "$tmp/$f.json"
  done
  printf '{' > "$tmp/not-json.json"
  expect_fail "入力: JSON でない入力は落ちる" build "$tmp/not-json.json"
  expect_fail "入力: 無いファイルは落ちる" build "$tmp/none.json"
  # 検証器は、対応していないキーワードのスキーマを通さない。
  jq -n '{type: "object", uniqueItems: true}' > "$tmp/unsupported.schema.json"
  expect_fail "検証器: 対応外のキーワードは違反" validate "$tmp/unsupported.schema.json" "$valid"

  # 2. 生成物
  build "$valid" > "$tmp/valid.html"
  html_checks valid "$tmp/valid.html" "$valid"
  build "$breaker" > "$tmp/breaker.html"
  html_checks script-breaker "$tmp/breaker.html" "$breaker"
  build "$valid" > "$tmp/valid2.html"
  if cmp -s "$tmp/valid.html" "$tmp/valid2.html"; then ok "生成: 同じ入力から同じ HTML"; else ng "生成: 出力が揺れる"; fi

  # 3. 保存先の形(verdicts.schema.json)
  verdict() { jq -n "$1" > "$tmp/doc.json"; validate "$VERDICTS_SCHEMA" "$tmp/doc.json"; }
  expect_ok "保存先: decide とメモの文書" verdict '{key:"gmail:t1/m1", verdict:"decide", note:"理由", at:"2026-01-02T03:04:05.678Z"}'
  expect_ok "保存先: note の無い古い文書" verdict '{key:"lapras:c1/20260102T1000", verdict:"pass", at:"2026-01-02T03:04:05Z"}'
  expect_ok "保存先: verdict が空文字の文書" verdict '{key:"none:t1/m1", verdict:"", note:"メモだけ", at:"2026-01-02T03:04:05.1+09:00"}'
  expect_ok "保存先: guidance/standing の文書" verdict '{text:"フィードバック", at:"2026-01-02T03:04:05.678Z"}'
  expect_ok "保存先: おまかせ(decide と owner と option)" verdict '{key:"gmail:t1/m1", verdict:"decide", owner:"agent", option:"send-agent", note:"", at:"2026-01-02T03:04:05.678Z"}'
  expect_ok "保存先: その他(note が指示)" verdict '{key:"gmail:t1/m1", verdict:"other", owner:"", option:"other", note:"先に条件だけ聞いてほしい", at:"2026-01-02T03:04:05.678Z"}'
  expect_ok "保存先: option と owner の無い古い decide" verdict '{key:"gmail:t1/m1", verdict:"decide", note:"理由", at:"2026-01-02T03:04:05.678Z"}'
  expect_fail "保存先: 知らない owner" verdict '{key:"gmail:t1/m1", verdict:"decide", owner:"robot", at:"2026-01-02T03:04:05Z"}'
  expect_fail "保存先: option の文字種" verdict '{key:"gmail:t1/m1", verdict:"decide", option:"Send Me", at:"2026-01-02T03:04:05Z"}'
  expect_fail "保存先: 知らない裁定の語" verdict '{key:"gmail:t1/m1", verdict:"maybe", at:"2026-01-02T03:04:05Z"}'
  expect_fail "保存先: 要旨のような余計な欄" verdict '{key:"gmail:t1/m1", verdict:"pass", summary:"x", at:"2026-01-02T03:04:05Z"}'
  expect_fail "保存先: at が ISO 8601 でない" verdict '{key:"gmail:t1/m1", verdict:"pass", at:"昨日"}'
  expect_fail "保存先: key が不正" verdict '{key:"t1", verdict:"pass", at:"2026-01-02T03:04:05Z"}'
  expect_fail "保存先: note が長すぎる" verdict '{key:"gmail:t1/m1", verdict:"pass", note:("あ" * 1001), at:"2026-01-02T03:04:05Z"}'

  # 4. テンプレートが書く文書が、保存先の形と同じであること
  want=$(jq -r '."$defs".verdictDoc.properties | keys | join(",")' "$VERDICTS_SCHEMA")
  got=$(grep 'const body = ' "$TEMPLATE" | grep -oE '[a-z]+: ' | tr -d ': ' | sort | paste -sd, -)
  if [ "$want" = "$got" ]; then ok "テンプレート: verdicts の文書の欄が Schema と一致 ($got)"; else ng "テンプレート: verdicts の文書の欄 [$got] が Schema [$want] と違う"; fi
  want=$(jq -r '."$defs".guidanceDoc.properties | keys | join(",")' "$VERDICTS_SCHEMA")
  got=$(grep -oE 'gdoc\.set\(\{[^}]*\}\)' "$TEMPLATE" | grep -oE '[a-z]+' | grep -vE '^(gdoc|set)$' | sort | paste -sd, -)
  if [ "$want" = "$got" ]; then ok "テンプレート: guidance の文書の欄が Schema と一致 ($got)"; else ng "テンプレート: guidance の文書の欄 [$got] が Schema [$want] と違う"; fi
  if grep -qF 'db.collection("verdicts")' "$TEMPLATE"; then ok "テンプレート: コレクション名 verdicts"; else ng "テンプレート: コレクション名が verdicts でない"; fi
  if grep -qF 'db.doc("guidance/standing")' "$TEMPLATE"; then ok "テンプレート: 文書 guidance/standing"; else ng "テンプレート: guidance/standing が無い"; fi
  got=$(node -e "$(grep '^function docId' "$TEMPLATE"); console.log(docId('gmail:AbC_1/dEf-2'))")
  if [ "$got" = "gmail.AbC_1.dEf-2" ]; then ok "テンプレート: docId の換え方"; else ng "テンプレート: docId が $got"; fi
  # docId は、db の文書 id が許す文字(英数字と _ - . ~ : @ +)だけで書けて、1 対 1 であること。`=` を残すと、GitHub の鍵のカードの答えが
  # 1 件も保存されない(2026-11-18・19 に実測。かつての検査は、換えの 1 対 1 だけを見て、許される文字を見ていなかった)。
  got=$(node -e "$(grep '^function docId' "$TEMPLATE"); const rows = JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8')); const ids = rows.map(r => docId('gh:' + r.thread + '/' + r.receipt)); const bad = ids.filter(i => !/^[A-Za-z0-9_.~:@+-]+\$/.test(i)); console.log(bad.length + ',' + (ids.length - new Set(ids).size))" "$HERE/fixtures/gh-keys.json")
  if [ "$got" = "0,0" ]; then ok "テンプレート: GitHub の鍵の docId が、db の許す文字だけで、衝突しない"; else ng "テンプレート: GitHub の鍵の docId に、許されない文字か衝突がある(違反,衝突=$got)"; fi
  # キー入力の仕様(FORMAT.md「キー入力の仕様」)を、状態の表で検査する。画面に触れない関数(@pure の区間)だけを取り出して、node で動かす。
  # (1) 順: 画面の順・キーの順は、入力の並び(紙の順)と同じ 1 つ。(2) 状態 × 符号: 「いま押せる」と表示するキーと、実際に選択が入るキーが、すべての状態で一致する。
  # キーが止まっている状態(確定後・入力欄・メモ欄)では、どのキーも選択にならない。(3) 件数: どの組合せでも、全項目が選んだ・あとで・未選択・指示が空のどれか 1 つに入る。
  # かつての検査は `inp.blur()` の文字列があるかだけを見て、入力の挙動と、表示との一致を見ていなかった(2026-11-23 に、キーが効かない原因が画面に出ないまま 2 回直した)。
  tjs=$(mktemp)
  {
    echo "const OPTIONS = JSON.parse(require('fs').readFileSync(process.argv[2], 'utf8')); const DATA = JSON.parse(require('fs').readFileSync(process.argv[3], 'utf8')); const ITEMS = DATA.items, ITEM = {}, state = {}; for (const it of ITEMS) ITEM[it.id] = it;"
    sed -n '/@pure:begin/,/@pure:end/p' "$TEMPLATE"
    cat <<'EOF'
const bad = [];
if (JSON.stringify(ORDER) !== JSON.stringify(ITEMS.map(it => it.id))) bad.push('順が、入力の並び(紙の順)と違う');
const letters = 'abcdefghijklmnopqrstuvwxyz'.split('').concat([' ']);
for (const it of ITEMS) {
  const cur = ORDER.indexOf(it.id);
  const shownKeys = hintsOf(it).map(h => h.key === 'Space' ? ' ' : h.key);
  /* 状態 = 画面(step・confirm・done) × 入力先(無し・メモ欄・入力欄)。選択になるのは、1 件の画面で、キーが受付中のときだけ。 */
  for (const view of ['step', 'confirm', 'done']) for (const target of [null, 'memo', 'field']) {
    const ks = keyState({ focused: true, typing: target, view });
    for (const k of letters) {
      const d = decideKey({ order: ORDER, cur, view }, { key: k, ctrl: false, meta: false, alt: false, target });
      const acts = d.act === 'choose', shown = shownKeys.includes(k);
      if (view === 'step' && ks.on && shown !== acts) bad.push(it.id + ': キー ' + JSON.stringify(k) + ' は、表示=' + shown + '、挙動=' + acts);
      if (!(view === 'step' && ks.on) && acts) bad.push(it.id + ': ' + view + '/' + target + ' で、' + JSON.stringify(k) + ' が選択になる');
      if (view === 'step' && ks.on && acts && shown) {
        const want = hintsOf(it).find(h => (h.key === 'Space' ? ' ' : h.key) === k).option;
        if (d.option.id !== want.id) bad.push(it.id + ': キー ' + JSON.stringify(k) + ' は、表示では ' + want.id + '、挙動では ' + d.option.id);
      }
    }
  }
}
/* 画面ごとの、符号以外のキー。確認画面は Backspace だけが最後の件へ戻る。完了画面は、すべて無視する。 */
const E = (key, extra) => Object.assign({ key, ctrl: false, meta: false, alt: false, target: null }, extra || {});
const dk = (view, e) => decideKey({ order: ORDER, cur: 0, view }, e).act;
if (dk('confirm', E('Backspace')) !== 'back-to-steps') bad.push('確認画面の Backspace が最後の件へ戻らない');
if (dk('confirm', E('ArrowRight')) !== 'ignore') bad.push('確認画面の → が効く');
if (dk('done', E('Backspace')) !== 'ignore') bad.push('完了画面の Backspace が効く');
if (dk('step', E('Enter', { ctrl: true })) !== 'confirm') bad.push('Ctrl+Enter が確認・確定に行かない');
if (dk('step', E('Backspace')) !== 'prev' || dk('step', E('ArrowLeft')) !== 'prev') bad.push('Backspace・← が前へでない');
if (dk('step', E('ArrowRight')) !== 'next') bad.push('→ が次へでない');
if (dk('step', E('a', { ctrl: true })) !== 'ignore') bad.push('Ctrl+a が効く');
if (dk('step', E('Escape', { target: 'memo' })) !== 'esc-memo') bad.push('メモ欄の Esc が次へ進まない');
if (dk('step', E('a', { target: 'memo' })) !== 'ignore') bad.push('メモ欄の中の a が選択になる');
if (keyState({ focused: false, typing: null, view: 'step' }).on) bad.push('フォーカスが無いのに、キーが受付中と表示される');
if (keyState({ focused: true, typing: null, view: 'done' }).on) bad.push('完了画面で、キーが受付中と表示される');
/* 選んだあとの行き先: 途中は次の件、最後の件と、確認画面から直しに来たあとは、確認画面。 */
for (let c = 0; c < ORDER.length; c++) {
  const a = advance({ cur: c, n: ORDER.length, fromConfirm: false }), b = advance({ cur: c, n: ORDER.length, fromConfirm: true });
  if (c < ORDER.length - 1 && !(a.view === 'step' && a.cur === c + 1)) bad.push('行き先: ' + c + ' の次が、次の件でない');
  if (c === ORDER.length - 1 && a.view !== 'confirm') bad.push('行き先: 最後の件の次が、確認画面でない');
  if (b.view !== 'confirm') bad.push('行き先: 直したあとが、確認画面でない');
}
for (let n = 0; n < 60; n++) {
  for (const it of ITEMS) {
    const os = optsOf(it), r = (n * 7 + it.id.length) % (os.length + 1);
    state[it.id] = r === os.length ? undefined : { verdict: os[r].verdict, owner: os[r].owner || '', option: os[r].id, at: 't', note: n % 2 ? 'x' : '' };
  }
  const p = progress(), cnt = { none: 0, later: 0, waiting: 0, chosen: 0 };
  for (const it of ITEMS) cnt[cellState(it, state[it.id])]++;
  if (p.chosen + p.later + p.none + p.waiting !== p.total || p.remaining !== p.none + p.waiting) bad.push('件数が合わない: ' + JSON.stringify(p));
  if (cnt.none !== p.none || cnt.later !== p.later || cnt.waiting !== p.waiting || cnt.chosen !== p.chosen) bad.push('概観のマスの数が、progress と違う');
}
console.log([...new Set(bad)].slice(0, 5).join(' / '));
EOF
  } > "$tjs"
  got=$(node "$tjs" "$OPTIONS_FILE" "$HERE/fixtures/valid.items.json" 2>&1); rm -f "$tjs"
  if [ -z "$got" ]; then ok "キー入力の仕様: 順が紙と同じで、表示するキーと効くキーがすべての状態で一致し、件数が合う"; else ng "キー入力の仕様: $got"; fi
  # 画面の側が、上の関数だけを使っていること(試験した関数と、実際の挙動が別物にならない)。
  if grep -qF 'decideKey({ order: ORDER, cur, view }' "$TEMPLATE" && grep -qF 'keyState({ focused: document.hasFocus()' "$TEMPLATE" && ! grep -qE 'keysOn|showCursor|cursorbar|syncCursor' "$TEMPLATE"; then ok "テンプレート: キー入力と状態の行は、画面に触れない関数(decideKey・keyState)から導く"; else ng "テンプレート: キー入力か状態の行が、画面に触れない関数を使っていない(または古い名前が残っている)"; fi
  # ショートカットのキーが、ボタン・要約などの標準のキー操作(Enter・Space)を奪わないのは、共有の keyguard の仕事(C12)。手書きの keydown を足さない。
  if grep -qF 'window.pageBehavior.keyguard(' "$TEMPLATE" && [ "$(grep -c "addEventListener(\"keydown\"" "$TEMPLATE")" -le 2 ]; then ok "テンプレート: ショートカットのキーは、共有の keyguard で受ける"; else ng "テンプレート: keyguard を使っていない、または手書きの keydown が増えている"; fi
  # 画面の構造の宣言(ステップ型)。語彙と軸の整合は repo-structure-check の web_structures が、描画との一致は visual_diff.py audit の F10 が見る(ここでは、宣言が在ることだけを見る)。
  if grep -qF '<meta name="page-structure" content="step">' "$TEMPLATE" && grep -qF '<meta name="page-axes" content="' "$TEMPLATE"; then ok "テンプレート: 画面の構造(step)と、課題の性質の軸が宣言されている"; else ng "テンプレート: 画面の構造の宣言(page-structure・page-axes)が無い"; fi
  if grep -qF 'gEdited' "$TEMPLATE"; then ok "テンプレート: 古いフィードバックを回答に含めない"; else ng "テンプレート: 古いフィードバックを回答に含める"; fi

  # 5. 選択肢の正本(options.json)が、画面・保存先の形・項目の kind と食い違わないこと
  want=$(jq -r '."$defs".item.properties.kind.enum | sort | join(",")' "$ITEMS_SCHEMA")
  got=$(jq -r '.kinds | keys | sort | join(",")' "$OPTIONS_FILE")
  if [ "$want" = "$got" ]; then ok "選択肢: kind が items.schema.json と一致 ($got)"; else ng "選択肢: kind [$got] が items.schema.json [$want] と違う"; fi
  got=$(jq -r --slurpfile v "$VERDICTS_SCHEMA" '
    ($v[0]."$defs".verdictDoc.properties.verdict.enum - [""]) as $verdicts
    | ($v[0]."$defs".verdictDoc.properties.owner.enum - [""]) as $owners
    | [.kinds | to_entries[] | .key as $k | .value.options as $os
        | ( ([$os[].id] | length) != ([$os[].id] | unique | length) ) as $dupid
        | ( [$os[] | select(.verdict == "later")] | length ) as $nl
        | ( [$os[] | select(.verdict == "other")] | length ) as $no
        | ( [$os[] | select(.verdict == "decide" and .owner == "me")] | length ) as $nme
        | ( [$os[] | select(.verdict == "pass")] | length ) as $np
        | ( [$os[] | select((.verdict as $x | $verdicts | index($x)) == null)] | length ) as $badverdict
        | ( [$os[] | select((.verdict == "decide") != (.owner != null)) ] | length ) as $badowner
        | ( [$os[] | select(.owner != null and ((.owner as $x | $owners | index($x)) == null))] | length ) as $badowner2
        | ( [$os[] | select((.id | test("^[a-z][a-z0-9-]*$") | not) or ((.label // "") == "") or ((.you // "") == "") or ((.agent // "") == ""))] | length ) as $badfield
        | select($dupid or $nl != 1 or $no != 1 or $nme < 1 or $np < 1 or $badverdict > 0 or $badowner > 0 or $badowner2 > 0 or $badfield > 0)
        | $k] | join(",")' "$OPTIONS_FILE")
  if [ -z "$got" ]; then ok "選択肢: 各 kind が later・other を 1 つずつ、decide(me)・pass を 1 つ以上持ち、語と担い手が保存先の語に収まる"; else ng "選択肢: 不整合のある kind [$got]"; fi
  # answer(事実の確認・完了報告の kind)は真偽値で、画面(テンプレート)が answer を読む。
  got=$(jq -r '[.kinds | to_entries[] | select(.value.answer != null and ((.value.answer | type) != "boolean")) | .key] | join(",")' "$OPTIONS_FILE")
  if [ -z "$got" ]; then ok "選択肢: answer は真偽値"; else ng "選択肢: answer が真偽値でない kind [$got]"; fi
  if [ "$(jq -r '[.kinds[] | select(.answer == true)] | length' "$OPTIONS_FILE")" -eq 0 ] || grep -qF 'OPTIONS.kinds[it.kind].answer' "$TEMPLATE"; then
    ok "テンプレート: answer の kind を分担の表で分ける"
  else
    ng "テンプレート: options.json に answer の kind があるのに、テンプレートが answer を読まない"
  fi
  # 見本の入力が全 kind を含む(画面の検査が、新しい kind を素通りしない)。
  want=$(jq -r '.kinds | keys | sort | join(",")' "$OPTIONS_FILE")
  got=$(jq -r '[.items[].kind] | unique | sort | join(",")' "$valid")
  if [ "$want" = "$got" ]; then ok "見本: valid.items.json が全 kind を含む ($got)"; else ng "見本: valid.items.json の kind [$got] が options.json [$want] と違う"; fi
  # 見本の項目の rec_option が、その kind の選択肢にあること(生成が通ることの裏づけ。build が検査する)。
  expect_ok "見本: 全 kind の項目を含む入力から HTML が組める" build "$valid"

  # 6. 意味層(GTD の行き先): 各選択肢が閉じた 7 語のどれかを持ち、各 kind が gtd_required の行き先にすべて到達できる。
  got=$(jq -r '. as $o | [.kinds | to_entries[] | .key as $k | .value as $v
      | ([$v.options[].gtd] | map(select(. == null or (. as $g | $o.gtd_values | index($g)) == null))) as $badg
      | ($v.gtd_required // []) as $req
      | ([$req[] | select(. as $r | ([$v.options[].gtd] | index($r)) == null)]) as $miss
      | select(($badg | length) > 0 or ($miss | length) > 0 or ($req | length) == 0) | $k] | join(",")' "$OPTIONS_FILE")
  if [ -z "$got" ]; then ok "意味層: 各選択肢が gtd を持ち、各 kind が gtd_required の行き先にすべて到達できる"; else ng "意味層: 不整合のある kind [$got]"; fi

  # 7. キー入力の符号: options.json と画面の入力が双方向に一致する(符号は重複せず、符号から選択肢へ戻れ、Space は later だけ)。
  local js
  js="$(grep '^function codesOf' "$TEMPLATE")"$'\n'"$(grep '^function optionForKey' "$TEMPLATE")"$'\n'"
    const O = JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));
    const bad = [];
    for (const [k, v] of Object.entries(O.kinds)) {
      const m = codesOf(v.options);
      const codes = Object.values(m);
      if (new Set(codes).size !== codes.length) bad.push(k + ': 符号の重複');
      for (const o of v.options) {
        const back = optionForKey(v.options, m[o.id]);
        if (!back || back.id !== o.id) bad.push(k + '/' + o.id + ': 符号から戻れない');
        if ((o.verdict === 'later') !== (m[o.id] === ' ')) bad.push(k + '/' + o.id + ': Space は later だけ');
      }
      if (optionForKey(v.options, '~') !== null) bad.push(k + ': 割り当てていない鍵が選択肢に当たる');
    }
    console.log(bad.join(','));"
  got=$(node -e "$js" "$OPTIONS_FILE")
  if [ -z "$got" ]; then ok "キー入力: options.json の並びから振った符号が、画面の入力と双方向に一致する"; else ng "キー入力: $got"; fi
  if grep -q 'keydown' "$TEMPLATE"; then ok "テンプレート: keydown の入力処理がある"; else ng "テンプレート: keydown の入力処理が無い"; fi

  # 8. GitHub の鍵(gh)の符号: owner/repo の可逆な符号で、往復し、文書 id が 1 対 1 になる(`a-b/c` と `a/b-c` が衝突しない)。
  got=$(jq -r -L "$HERE" 'include "gh-key";
    [.[] | (.ref | split("#")) as $p
      | select(($p[0] | gh_enc) != .thread or (.thread | gh_dec) != $p[0] or $p[1] != .receipt
               or (("gh:" + .thread + "/" + .receipt) | test("^gh:[A-Za-z0-9_=-]+/[A-Za-z0-9_=-]+$") | not))
      | .ref] | join(",")' "$HERE/fixtures/gh-keys.json")
  if [ -z "$got" ]; then ok "gh の鍵: 符号が往復し、鍵の文字種に収まる"; else ng "gh の鍵: 往復しない [$got]"; fi
  got=$(jq -r '[.[] | "gh." + .thread + "." + .receipt] | (length) - (unique | length)' "$HERE/fixtures/gh-keys.json")
  if [ "$got" = 0 ]; then ok "gh の鍵: 文書 id が 1 対 1(衝突しない)"; else ng "gh の鍵: 文書 id が衝突する($got 件)"; fi

  echo "pass=$PASS fail=$FAIL"
  [ "$FAIL" -eq 0 ]
}

main() {
  local data="" out="" mode=build
  while [ $# -gt 0 ]; do
    case "$1" in
      --selftest) mode=selftest ;;
      --data) [ $# -ge 2 ] || { usage; exit 2; }; data=$2; shift ;;
      --out) [ $# -ge 2 ] || { usage; exit 2; }; out=$2; shift ;;
      -h|--help) usage; exit 0 ;;
      *) usage; exit 2 ;;
    esac
    shift
  done
  command -v jq >/dev/null 2>&1 || die "jq が見つかりません(PATH を確認してください)"
  if [ "$mode" = selftest ]; then
    selftest
    return
  fi
  [ -n "$data" ] || { usage; exit 2; }
  if [ -z "$out" ]; then
    build "$data"
  else
    # 検証に落ちたとき、既存の出力を壊さない。
    if build "$data" > "$out.tmp.$$"; then
      mv "$out.tmp.$$" "$out"
    else
      rm -f "$out.tmp.$$"
      return 1
    fi
  fi
}

main "$@"
