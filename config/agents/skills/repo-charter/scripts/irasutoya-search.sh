#!/usr/bin/env bash
# irasutoya-search.sh — repo-charter §1 手順 6(README banner の選定)の候補探し。
#
# いらすとやの検索ページ(/search?q=…)は JS で描画され、WebFetch や curl では画像 URL が
# 取れない。Blogger の標準 JSON フィードなら、1 回の取得で題名・投稿 URL・画像 URL が返る
# (#940)。既存手段(curl と jq)で足り、新しい依存は無い。このスクリプトは、その curl と
# jq の呼び出しを 1 本にまとめるだけの薄いラッパ。
#
# 出力: 候補の JSON 配列(標準出力)。各要素は title / url(投稿ページ)/ s400 / s800
# (画像 URL。Blogger は URL 中の /s<数字>/ の段で大きさを切り替える)。
# 候補を選んだら、利用規約の確認と帰属注記は SKILL.md §5 の手順に従う。
set -euo pipefail

FEED_URL="${IRASUTOYA_FEED_URL:-https://www.irasutoya.com/feeds/posts/default}"

usage() {
  cat <<'EOF'
usage: irasutoya-search.sh [--max N] [--dry-run] <検索語>
       irasutoya-search.sh --selftest

  --max N     取得する件数の上限(既定 4)
  --dry-run   フィードを取得せず、走らせる curl を表示する
  --selftest  固定のフィードで、候補の抽出(jq)だけを確かめる(ネットワーク不使用)
EOF
}

die() {
  echo "irasutoya-search.sh: $*" >&2
  exit 2
}

# 標準入力: Blogger の JSON フィード / 標準出力: 候補の JSON 配列。
# 本文の HTML の最初の <img src=…> を画像とし、画像の無い投稿は落とす。
extract() {
  jq '
    def size($u; $n): $u | sub("/s[0-9]+/(?<f>[^/]+)$"; "/s\($n)/\(.f)");
    [ (.feed.entry // [])[]
      | ((.content["$t"] // "") | [scan("<img[^>]*src=\"([^\"]+)\"")][0][0]) as $src
      | select($src != null)
      | { title: .title["$t"],
          url: ([.link[] | select(.rel == "alternate")][0].href),
          s400: size($src; 400),
          s800: size($src; 800) } ]
  '
}

selftest() {
  local fixture out
  # shellcheck disable=SC2016 # jq の "$t" は展開させない
  fixture='{"feed":{"entry":[
    {"title":{"$t":"鳩のイラスト"},
     "link":[{"rel":"edit","href":"https://example.invalid/edit"},{"rel":"alternate","href":"https://example.invalid/a.html"}],
     "content":{"$t":"<a href=\"https://example.invalid/img/s912/a.png\"><img alt=\"x\" border=\"0\" src=\"https://example.invalid/img/s400/a.png\" /></a>"}},
    {"title":{"$t":"画像なしの投稿"},
     "link":[{"rel":"alternate","href":"https://example.invalid/b.html"}],
     "content":{"$t":"<div>本文だけ</div>"}}]}}'
  out="$(printf '%s' "$fixture" | extract)"
  printf '%s' "$out" | jq -e '
    length == 1
    and .[0].title == "鳩のイラスト"
    and .[0].url == "https://example.invalid/a.html"
    and .[0].s400 == "https://example.invalid/img/s400/a.png"
    and .[0].s800 == "https://example.invalid/img/s800/a.png"' > /dev/null \
    || die "selftest: 抽出結果が期待と違う: $out"
  # 空のフィード(検索結果 0 件)は空配列になる
  [ "$(printf '{"feed":{}}' | extract | jq -c .)" = "[]" ] || die "selftest: 空のフィードが空配列にならない"
  echo "irasutoya-search.sh selftest: OK"
}

main() {
  local max=4 dry=0 query=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --selftest)
        selftest
        return 0
        ;;
      --max)
        [ $# -ge 2 ] || die "--max には件数が要る"
        max="$2"
        shift 2
        ;;
      --dry-run)
        dry=1
        shift
        ;;
      -h | --help)
        usage
        return 0
        ;;
      -*)
        usage >&2
        die "未知のオプション: $1"
        ;;
      *)
        [ -z "$query" ] || die "検索語は 1 つだけ(空白を含むなら引用符で囲む)"
        query="$1"
        shift
        ;;
    esac
  done
  [ -n "$query" ] || {
    usage >&2
    exit 2
  }
  case "$max" in "" | *[!0-9]*) die "--max は正の整数: $max" ;; esac

  local curl_args=(-fsSL --max-time 30 -G "$FEED_URL"
    --data-urlencode "q=$query" --data-urlencode "alt=json" --data-urlencode "max-results=$max")
  if [ "$dry" -eq 1 ]; then
    printf 'curl'
    printf ' %q' "${curl_args[@]}"
    printf ' | jq <候補の抽出>\n'
    return 0
  fi
  curl "${curl_args[@]}" | extract
}

main "$@"
