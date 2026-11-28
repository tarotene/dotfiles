#!/usr/bin/env bash
# 各画面が持つ、デザイン正本(config/web/)の複写の固定(design-pin.toml)を検査する(ADR-1685)。
# 先行例: 裁定ページの保存先 Schema を、commit で固定した URL と sha256 で固定する bash 検査(個人の記録のリポジトリの型)。
#
# pin の形(TOML。複数の複写は [[file]] を並べる)。URL は pin に書かず、正本のリポジトリ(SOURCE_REPO。
# action の `github.action_repository`)と commit と name から組み立てる。所有者名が各画面のファイルに入らず、
# main などの浮動した参照も、別のリポジトリへの参照も、書けない。
#
#   [[file]]
#   commit = "<40 桁の小文字 16 進>"
#   name   = "variables.css"          # 正本の config/web/ の下の名前
#   sha256 = "<64 桁の小文字 16 進>"
#   path   = "web/variables.css"      # 呼び出し側のリポジトリにある複写
#
# 1 枚だけの HTML(Artifact の雛形)は、正本を別のファイルとして置けない。その複写は、HTML の中の印を付けた区間に置き、
# `begin`・`end` で区間を指す(両方か、どちらも書かない)。区間は、`begin` を含む行の次の行から、`end` を含む行の前の行まで。
# 区間の内容(行ごとに改行を付けたもの)の sha256 が、pin の sha256(= 正本のファイルの sha256)と一致しなければならない。
#
#   [[file]]
#   commit = "<40 桁の小文字 16 進>"
#   name   = "variables.css"
#   sha256 = "<64 桁の小文字 16 進>"
#   path   = "src/page.html"
#   begin  = "design-pin:begin variables.css"   # この語を含む行が 1 行だけ
#   end    = "design-pin:end variables.css"     # この語を含む行が 1 行だけ(begin より後)
#
# 検査は 3 つ。
#   1. commit が 40 桁の 16 進で、name が config/web/ の下の 1 つのファイル名であること。
#   2. path の複写(`begin`・`end` があれば、その区間)の sha256 が、pin の sha256 と一致すること。
#   3. CHECK_UPSTREAM=true のときは、組み立てた URL の内容の sha256 も pin と一致すること(pin 自体が正本と
#      別の内容を指していないこと)。取得できないとき(オフライン)は、警告を出して 3 だけを飛ばす。
#
# 使い方:
#   SOURCE_REPO=<owner>/<repo> PIN_FILE=design-pin.toml CHECK_UPSTREAM=true \
#     bash .github/actions/design-pin/check.sh
#   bash .github/actions/design-pin/check.sh --selftest
#
# 道具: bash + yq + jq + curl + sha256sum(GitHub の ubuntu runner に入っている)。
set -euo pipefail

# region <path> <begin> <end>: begin を含む行の次の行から、end を含む行の前の行までを、行ごとに改行を付けて出す。
region() {
  awk -v b="$2" -v e="$3" '
    state == 0 && index($0, b) { state = 1; next }
    state == 1 && index($0, e) { state = 2; next }
    state == 1 { print }
  ' "$1"
}

check() {
  local source_repo="$1" pin_file="$2" check_upstream="$3"
  for cmd in yq jq curl sha256sum; do
    if ! command -v "$cmd" > /dev/null 2>&1; then
      echo "::error::$cmd が見つかりません"
      return 2
    fi
  done
  if [[ ! "$source_repo" =~ ^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$ ]]; then
    echo "::error::SOURCE_REPO が <owner>/<repo> の形でない: $source_repo"
    return 2
  fi
  if [[ ! -f "$pin_file" ]]; then
    echo "::error::pin のファイル $pin_file がない"
    return 1
  fi
  local pin count
  pin="$(yq -p toml -o json "$pin_file")"
  count="$(jq -r '(.file // []) | length' <<< "$pin")"
  if [[ "$count" -lt 1 ]]; then
    echo "::error::$pin_file に [[file]] が 1 つもない"
    return 1
  fi
  local fail=0 tmp i commit name want path url got up begin end nb ne
  tmp="$(mktemp)"
  for ((i = 0; i < count; i++)); do
    commit="$(jq -r ".file[$i].commit // empty" <<< "$pin")"
    name="$(jq -r ".file[$i].name // empty" <<< "$pin")"
    want="$(jq -r ".file[$i].sha256 // empty" <<< "$pin")"
    path="$(jq -r ".file[$i].path // empty" <<< "$pin")"
    if [[ -z "$commit" || -z "$name" || -z "$want" || -z "$path" ]]; then
      echo "::error::$pin_file の [[file]] #$((i + 1)) に commit・name・sha256・path が揃っていない"
      fail=1
      continue
    fi
    if [[ ! "$commit" =~ ^[0-9a-f]{40}$ ]]; then
      echo "::error::$pin_file の commit が 40 桁の小文字 16 進でない(main などの浮動した参照は書けない): $commit"
      fail=1
      continue
    fi
    if [[ ! "$name" =~ ^[A-Za-z0-9._-]+$ || "$name" == .* ]]; then
      echo "::error::$pin_file の name が、config/web/ の下の 1 つのファイル名でない: $name"
      fail=1
      continue
    fi
    if [[ ! "$want" =~ ^[0-9a-f]{64}$ ]]; then
      echo "::error::$pin_file の sha256 が 64 桁の小文字 16 進でない: $want"
      fail=1
      continue
    fi
    if [[ ! -f "$path" ]]; then
      echo "::error::複写 $path がない($pin_file の path)"
      fail=1
      continue
    fi
    begin="$(jq -r ".file[$i].begin // empty" <<< "$pin")"
    end="$(jq -r ".file[$i].end // empty" <<< "$pin")"
    if [[ -n "$begin" || -n "$end" ]]; then
      if [[ -z "$begin" || -z "$end" ]]; then
        echo "::error::$pin_file の [[file]] #$((i + 1)) の begin と end は、両方書くか、どちらも書かない"
        fail=1
        continue
      fi
      nb="$(grep -cF -- "$begin" "$path" || true)"
      ne="$(grep -cF -- "$end" "$path" || true)"
      if [[ "$nb" != 1 || "$ne" != 1 ]]; then
        echo "::error::複写 $path に、begin($begin)を含む行が $nb 行、end($end)を含む行が $ne 行ある(どちらも 1 行だけ)"
        fail=1
        continue
      fi
      if (($(grep -nF -- "$begin" "$path" | cut -d: -f1) >= $(grep -nF -- "$end" "$path" | cut -d: -f1))); then
        echo "::error::複写 $path の end($end)が begin($begin)より前にある"
        fail=1
        continue
      fi
      got="$(region "$path" "$begin" "$end" | sha256sum | cut -d' ' -f1)"
    else
      got="$(sha256sum "$path" | cut -d' ' -f1)"
    fi
    if [[ "$got" != "$want" ]]; then
      echo "::error::複写 $path の sha256 が pin と違う(pin=$want 実際=$got)。正本から複写し直し、pin を上げる"
      fail=1
    fi
    if [[ "$check_upstream" == "true" ]]; then
      url="https://raw.githubusercontent.com/${source_repo}/${commit}/config/web/${name}"
      if curl -fsSL --max-time 20 "$url" -o "$tmp" 2> /dev/null; then
        up="$(sha256sum "$tmp" | cut -d' ' -f1)"
        if [[ "$up" != "$want" ]]; then
          echo "::error::pin の commit の正本($url)の sha256 が、pin の sha256 と違う(内容=$up pin=$want)"
          fail=1
        fi
      else
        echo "::warning::正本を取得できなかった($url)。正本の内容の照合だけを飛ばす"
      fi
    fi
  done
  rm -f "$tmp"
  return "$fail"
}

selftest() {
  local dir total=0 bad=0
  dir="$(mktemp -d)"
  trap 'rm -rf "$dir"' RETURN
  pushd "$dir" > /dev/null
  mkdir web
  printf ':root { --bg: #ffffff; }\n' > web/variables.css
  local h c out code
  h="$(sha256sum web/variables.css | cut -d' ' -f1)"
  c='0123456789abcdef0123456789abcdef01234567'
  mk() { printf '[[file]]\ncommit = "%s"\nname   = "%s"\nsha256 = "%s"\npath   = "%s"\n' "$1" "$2" "$3" "$4" > case.toml; }
  expect() { # 名前 期待する終了コード 出力に含まれるべき語
    local name="$1" want="$2" needle="$3"
    total=$((total + 1))
    set +e
    out="$(check example/source case.toml false 2>&1)"
    code=$?
    set -e
    if [[ "$code" == "$want" && "$out" == *"$needle"* ]]; then
      echo "ok   $name"
    else
      echo "FAIL $name (exit=$code, 期待 $want): $out" >&2
      bad=$((bad + 1))
    fi
  }
  mk "$c" "variables.css" "$h" "web/variables.css"
  expect "複写と pin が一致する" 0 ""
  cp web/variables.css web/tampered.css
  printf '/* 手で直した */\n' >> web/tampered.css
  mk "$c" "variables.css" "$h" "web/tampered.css"
  expect "複写が手で直されている" 1 "sha256 が pin と違う"
  mk "main" "variables.css" "$h" "web/variables.css"
  expect "main への浮動した参照" 1 "浮動した参照は書けない"
  mk "${c:0:39}" "variables.css" "$h" "web/variables.css"
  expect "commit が 40 桁でない" 1 "40 桁の小文字 16 進でない"
  mk "$c" "../README.md" "$h" "web/variables.css"
  expect "config/web/ の外を指す name" 1 "1 つのファイル名でない"
  mk "$c" ".hidden" "$h" "web/variables.css"
  expect "隠しファイルの name" 1 "1 つのファイル名でない"
  mk "$c" "variables.css" "ABC" "web/variables.css"
  expect "sha256 の形が違う" 1 "64 桁の小文字 16 進でない"
  mk "$c" "variables.css" "$h" "web/missing.css"
  expect "複写がない" 1 "複写 web/missing.css がない"
  printf 'title = "x"\n' > case.toml
  expect "[[file]] が 1 つもない" 1 "[[file]] が 1 つもない"
  printf '[[file]]\ncommit = "%s"\nname = "variables.css"\n' "$c" > case.toml
  expect "欄が揃っていない" 1 "commit・name・sha256・path が揃っていない"
  # 1 枚だけの HTML の中の区間(begin・end)
  mkr() { printf '[[file]]\ncommit = "%s"\nname   = "variables.css"\nsha256 = "%s"\npath   = "%s"\nbegin  = "%s"\nend    = "%s"\n' "$c" "$h" "$1" "$2" "$3" > case.toml; }
  b='design-pin:begin variables.css'
  e='design-pin:end variables.css'
  printf '<style>\n/* %s */\n:root { --bg: #ffffff; }\n/* %s */\n</style>\n' "$b" "$e" > web/page.html
  mkr "web/page.html" "$b" "$e"
  expect "区間が正本と一致する" 0 ""
  printf '<style>\n/* %s */\n:root { --bg: #fffffe; }\n/* %s */\n</style>\n' "$b" "$e" > web/page-tampered.html
  mkr "web/page-tampered.html" "$b" "$e"
  expect "区間が手で直されている" 1 "sha256 が pin と違う"
  printf '<style>\n/* %s */\n:root { --bg: #ffffff; }\n/* %s */\n:root { --x: 1; }\n</style>\n' "$b" "$e" > web/page-outside.html
  mkr "web/page-outside.html" "$b" "$e"
  expect "区間の外の変更は、区間の固定に触れない" 0 ""
  printf '[[file]]\ncommit = "%s"\nname = "variables.css"\nsha256 = "%s"\npath = "web/page.html"\nbegin = "%s"\n' "$c" "$h" "$b" > case.toml
  expect "begin だけで end がない" 1 "両方書くか、どちらも書かない"
  printf '<style>\n:root { --bg: #ffffff; }\n/* %s */\n</style>\n' "$e" > web/page-nobegin.html
  mkr "web/page-nobegin.html" "$b" "$e"
  expect "begin の印がない" 1 "begin($b)を含む行が 0 行"
  printf '<style>\n/* %s */\n:root { --bg: #ffffff; }\n/* %s */\n/* %s */\n</style>\n' "$b" "$e" "$e" > web/page-dup.html
  mkr "web/page-dup.html" "$b" "$e"
  expect "end の印が 2 行ある" 1 "end($e)を含む行が 2 行"
  printf '<style>\n/* %s */\n/* %s */\n:root { --bg: #ffffff; }\n</style>\n' "$e" "$b" > web/page-order.html
  mkr "web/page-order.html" "$b" "$e"
  expect "end が begin より前にある" 1 "end($e)が begin($b)より前にある"
  rm -f case.toml
  expect "pin のファイルがない" 1 "pin のファイル"
  total=$((total + 1))
  set +e
  out="$(check 'bad repo' case.toml false 2>&1)"
  code=$?
  set -e
  if [[ "$code" == 2 && "$out" == *"<owner>/<repo> の形でない"* ]]; then
    echo "ok   SOURCE_REPO の形が違う"
  else
    echo "FAIL SOURCE_REPO の形が違う (exit=$code): $out" >&2
    bad=$((bad + 1))
  fi
  popd > /dev/null
  echo "design-pin --selftest: $((total - bad))/$total"
  [[ "$bad" -eq 0 ]]
}

if [[ "${1:-}" == "--selftest" ]]; then
  selftest
else
  check "${SOURCE_REPO:?SOURCE_REPO(<owner>/<repo>)を渡す}" "${PIN_FILE:-design-pin.toml}" "${CHECK_UPSTREAM:-true}"
fi
