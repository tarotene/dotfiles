#!/usr/bin/env bash
# venue-urls.sh — private な person-state リポジトリ側の state/places.toml
# (自宅・職場・公共施設の登録状況・常用スタジオ)から、会場探しの候補
# サービス一覧を決定的に生成する。ネットワークアクセスは行わない — 実際の
# 空き確認・ページ取得は呼び出し側(Claude、Playwright/WebFetch)が担う。
#
# 「決定的」の範囲: どのサービス・URL を見に行くべきかの列挙だけ。個別の
# 部屋・時間帯の空き状況は各サービスがログイン不要で公開しているページに
# よってしか分からず、ここでは扱わない(SKILL.md の手順を参照)。
#
# 使い方:
#   bash venue-urls.sh --hub <person-state-repo> [--needs grand-piano,...]
#   自己検査:   venue-urls.sh --selftest
#
# 出力: JSON 配列。各要素は {service, branch?, url, login_required,
# note} — login_required は true/false/文字列(未確認・条件付きなど)。
set -euo pipefail

self_path() {
  printf '%s/%s' "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" "$(basename "${BASH_SOURCE[0]}")"
}

for cmd in yq jq; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "error: $cmd が見つかりません(PATH を確認してください)" >&2
    exit 2
  fi
done

run_main() {
  local hub="$1" needs="$2"

  local places_file="$hub/state/places.toml"
  if [[ ! -r "$places_file" ]]; then
    echo "error: $places_file が読めません(ハブのレイアウトが想定と違う可能性)" >&2
    return 1
  fi
  local places_json
  places_json="$(yq -p toml -o json "$places_file")"

  # ピアノが要件に含まれるか(grand-piano/upright-piano のいずれか)。
  local wants_piano=0
  if [[ -n "$needs" ]] && [[ ",$needs," == *",grand-piano,"* || ",$needs," == *",upright-piano,"* ]]; then
    wants_piano=1
  fi

  local candidates="[]"
  add() {
    candidates="$(jq -c --argjson item "$1" '. + [$item]' <<<"$candidates")"
  }

  # --- 常用スタジオ(ノア等、state/places.toml の preferred_venues) ---
  local n_pref i pv service branch
  n_pref="$(jq -r '(.preferred_venues // []) | length' <<<"$places_json")"
  for ((i = 0; i < n_pref; i++)); do
    pv="$(jq -c ".preferred_venues[$i]" <<<"$places_json")"
    service="$(jq -r '.service' <<<"$pv")"
    branch="$(jq -r '.branch' <<<"$pv")"
    if [[ "$service" == "ピアノスタジオノア" ]]; then
      add "$(jq -n --arg service "$service" --arg branch "$branch" \
        '{service: $service, branch: $branch,
          url: "https://www.grandpiano.jp/noahweb/webs/chart/",
          login_required: false,
          note: "週表示はログイン不要(実測 2026-09-27)。Web予約は毎月末17:00に4か月先の月末まで開く。2名の個人練習料金枠は前日21:00から。店舗を選んで空きを確認する。"}')"
    else
      add "$(jq -n --arg service "$service" --arg branch "$branch" \
        '{service: $service, branch: $branch, url: null, login_required: null,
          note: "この service 向けの既知 URL が無い。手動で確認する。"}')"
    fi
  done

  # --- ピアノが要件のときだけ出す集約サイト ---
  if [[ "$wants_piano" -eq 1 ]]; then
    add '{"service": "SpaceMarket", "url": "https://www.spacemarket.com/lists/a9m2zhee0p3ff9nbvmaaxcte/", "login_required": "予約には要登録(閲覧がログイン不要かは未確認)", "note": "東京都のグランドピアノ物件一覧。robots.txt は Claude-User を許可(実測 2026-09-27)。"}'
    add '{"service": "Instabase", "url": "https://www.instabase.jp/tokyo/list/piano", "login_required": "予約には要登録(一覧の閲覧はログイン不要)", "note": "東京都のピアノ可物件一覧。robots.txt が検索・クエリ付きURLをDisallowし60秒間隔を求める(実測 2026-09-27)— 個別物件ページを間隔を空けて見る。"}'
  fi

  # --- 公共施設(登録済みのものだけ) ---
  local n_fac fac status system
  n_fac="$(jq -r '(.facility_registrations // []) | length' <<<"$places_json")"
  for ((i = 0; i < n_fac; i++)); do
    fac="$(jq -c ".facility_registrations[$i]" <<<"$places_json")"
    status="$(jq -r '.status' <<<"$fac")"
    [[ "$status" != "registered" ]] && continue
    system="$(jq -r '.system' <<<"$fac")"
    case "$system" in
      えどねっと)
        add '{"service": "えどねっと", "url": "https://www.shisetsuyoyaku.city.edogawa.tokyo.jp/user/Home", "login_required": "予約には要利用者登録(登録済み)。空き照会がログイン不要かは未確認。", "note": "江戸川区の施設予約システム。ピアノの有無は施設ごとに確認する。"}'
        ;;
      *)
        add "$(jq -n --arg system "$system" '{service: $system, url: null, login_required: null, note: "この system 向けの既知 URL が無い。手動で確認する。"}')"
        ;;
    esac
  done

  jq . <<<"$candidates"
}

# --- サブコマンド: --selftest ---------------------------------------------------
if [[ "${1:-}" == "--selftest" ]]; then
  self="$(self_path)"
  fail=0
  dir="$(mktemp -d)"
  trap 'rm -rf "$dir"' EXIT

  check() { # check <名前> <期待> <実際>
    if [[ "$2" == "$3" ]]; then
      echo "ok   $1"
    else
      echo "FAIL $1 (expected [$2], got [$3])" >&2
      fail=1
    fi
  }

  # --hub 省略・存在しない places.toml は非0で終わること。
  if bash "$self" --hub "$dir/no-such-hub" >/dev/null 2>&1; then
    echo "FAIL 存在しない hub で成功してしまった" >&2
    fail=1
  else
    echo "ok   存在しない hub は非0で終わる"
  fi

  mkdir -p "$dir/hub/state"
  cat >"$dir/hub/state/places.toml" <<'TOML'
updated = "2026-09-27"

[home]
municipality = "東京都テスト区"
station = "テスト駅"

[[facility_registrations]]
system = "えどねっと"
municipality = "江戸川区"
status = "registered"

[[facility_registrations]]
system = "未登録システム"
municipality = "どこか区"
status = "not-registered"

[[preferred_venues]]
service = "ピアノスタジオノア"
branch = "秋葉原"

[[preferred_venues]]
service = "未知のサービス"
branch = "どこか支店"
TOML

  out_no_needs="$(bash "$self" --hub "$dir/hub")"
  n_no_needs="$(jq 'length' <<<"$out_no_needs")"
  check "needs 無指定は SpaceMarket/Instabase を含まない" 0 \
    "$(jq '[.[] | select(.service == "SpaceMarket" or .service == "Instabase")] | length' <<<"$out_no_needs")"
  check "needs 無指定でもノア・えどねっとは含む" 2 \
    "$(jq '[.[] | select(.service == "ピアノスタジオノア" or .service == "えどねっと")] | length' <<<"$out_no_needs")"
  check "未登録の施設は含まない" 0 \
    "$(jq '[.[] | select(.service == "未登録システム")] | length' <<<"$out_no_needs")"
  check "未知の service は url:null で列挙される" "null" \
    "$(jq -r '.[] | select(.service == "未知のサービス") | .url' <<<"$out_no_needs")"

  out_piano="$(bash "$self" --hub "$dir/hub" --needs grand-piano)"
  check "needs=grand-piano で SpaceMarket/Instabase を含む" 2 \
    "$(jq '[.[] | select(.service == "SpaceMarket" or .service == "Instabase")] | length' <<<"$out_piano")"

  n_piano="$(jq 'length' <<<"$out_piano")"
  check "needs 指定時の件数は無指定時+2件(SpaceMarket/Instabase)" $((n_no_needs + 2)) "$n_piano"

  [[ "$fail" == 0 ]] && echo "selftest: all passed"
  exit "$fail"
fi

# --- 通常実行 ---------------------------------------------------------------
hub=""
needs=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --hub) hub="$2"; shift 2 ;;
    --needs) needs="$2"; shift 2 ;;
    *) echo "error: 不明な引数: $1" >&2; exit 2 ;;
  esac
done
: "${hub:?--hub でハブのパスを指定してください}"

run_main "$hub" "$needs"
