#!/usr/bin/env bash
# 各面が持つ、デザイン正本(config/web/)の複写の固定(design-pin.toml)を検査する(ADR-1685)。
# 先行例: 裁定ページの保存先 Schema を、commit で固定した URL と sha256 で固定する bash 検査(個人の記録のリポジトリの型)。
#
# pin の形(TOML。複数の複写は [[file]] を並べる)。URL は pin に書かず、正本のリポジトリ(SOURCE_REPO。
# action の `github.action_repository`)と commit と name から組み立てる。所有者名が各面のファイルに入らず、
# main などの浮動した参照も、別のリポジトリへの参照も、書けない。
#
#   [[file]]
#   commit = "<40 桁の小文字 16 進>"
#   name   = "variables.css"          # 正本の config/web/ の下の名前
#   sha256 = "<64 桁の小文字 16 進>"
#   path   = "web/variables.css"      # 呼び出し側のリポジトリにある複写
#
# 検査は 3 つ。
#   1. commit が 40 桁の 16 進で、name が config/web/ の下の 1 つのファイル名であること。
#   2. path の複写の sha256 が、pin の sha256 と一致すること。
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
    echo "::error::pin のファイル $pin_file が無い"
    return 1
  fi
  local pin count
  pin="$(yq -p toml -o json "$pin_file")"
  count="$(jq -r '(.file // []) | length' <<< "$pin")"
  if [[ "$count" -lt 1 ]]; then
    echo "::error::$pin_file に [[file]] が 1 つも無い"
    return 1
  fi
  local fail=0 tmp i commit name want path url got up
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
      echo "::error::複写 $path が無い($pin_file の path)"
      fail=1
      continue
    fi
    got="$(sha256sum "$path" | cut -d' ' -f1)"
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
  expect "複写が無い" 1 "複写 web/missing.css が無い"
  printf 'title = "x"\n' > case.toml
  expect "[[file]] が 1 つも無い" 1 "[[file]] が 1 つも無い"
  printf '[[file]]\ncommit = "%s"\nname = "variables.css"\n' "$c" > case.toml
  expect "欄が揃っていない" 1 "commit・name・sha256・path が揃っていない"
  rm -f case.toml
  expect "pin のファイルが無い" 1 "pin のファイル"
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
