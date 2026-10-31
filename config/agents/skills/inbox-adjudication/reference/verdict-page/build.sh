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
  # 差し込み点は 1 か所ずつ(コメントなどに同じ語が残ると、置換が別の場所に当たる)。
  for ph in __DATA__ __FORMAT_VERSION__; do
    n=$(grep -o "$ph" "$TEMPLATE" | wc -l || true)
    [ "$n" -eq 1 ] || die "template.html に $ph が $n 個ある(1 個でなければならない)"
  done
  # 版を先に置き換える: データ側の文字列に __FORMAT_VERSION__ があっても、書き換わらない。
  jq -nj --rawfile tpl "$TEMPLATE" --slurpfile d "$data" '
    ($d[0] | tojson | gsub("<"; "\\u003c")) as $json
    | $tpl
    | sub("__FORMAT_VERSION__"; ($d[0].format_version | tostring))
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
  if [ "$name" = valid ] && grep -q '__DATA__\|__FORMAT_VERSION__' "$html"; then
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
  if [ "$(grep '^const DATA = ' "$html" | sed 's/^const DATA = //; s/;$//' | jq -S .)" = "$(jq -S . "$src")" ]; then
    ok "$name: DATA が入力と一致する"
  else
    ng "$name: DATA が入力と違う"
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
  for f in bad-key bad-key-dot dup-id dup-key missing-required missing-run-label extra-top extra-item wrong-version bad-when-class bad-id bad-draft-key; do
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
