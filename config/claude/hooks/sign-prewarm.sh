#!/usr/bin/env bash
# sign-prewarm.sh — git commit の署名パスフレーズを、ログイン後最初に Claude を
# 開いた安全な瞬間に前倒しして温める SessionStart hook。
#
# 設計と根拠: docs/claude/sign-prewarm.md(このリポジトリ内)
#
# home/modules/gpg.nix が gpg-agent の cache TTL を実質無限(400d)にしたことで、
# パスフレーズの再入力は「ログインに 1 回」まで落ちる(境界は home.activation の
# assertNoLinger が担保する — agent の寿命がログインセッションと一致することの
# 検査)。しかし残る「その 1 回」を放置すると、Claude の Bash 呼び出し(例:
# `git commit`)の最中に GUI pinentry が gpg-agent.conf の grab 付きで出現し、
# キー入力を奪ったままコミットが固まる。この hook は副作用(キャッシュを温める)
# だけを目的とし、additionalContext は一切出さない。
#
# 判定は SessionStart 時の cwd に依存しない。cwd で判定すると、非署名 repo や
# git 管理外ディレクトリで開始したセッションが後から署名 repo に移ってコミット
# する経路を取りこぼす(実際に温めるべきだったのに温めない)。代わりに常に
# scope なしの `git config --get` をリポジトリ外(mktemp -d)から読み、グローバル
# 値だけを見る。結果、ルールは「そのホストで署名が有効なら、ログイン後最初に
# Claude を開いたとき 1 回聞かれる」という cwd に依存しない予測可能な形になる。
# --global を使わないのは、移行前の残骸 ~/.gitconfig が home-manager の
# ~/.config/git/config より先に読まれて隠すケースがあるため(実際にこの環境で
# 発生した)。
#
# 対象は「オンディスクで利用可能な鍵」だけである。card-backed な鍵を温めようと
# すると PIN + タッチが要求され、これはまさに ADR-0003 が smartcard [S] を避けて
# 得た利益(毎コミットのタッチが要らないこと)への回帰になる。判定は
# `gpg --list-secret-keys --with-colons` の field 15(S/N of a token)で行う:
# `+` はオンディスクで利用可能、`#` は simple stub、token の serial number は
# card-backed —— この 3 値は GnuPG の DETAILS ドキュメントに明記されている。
#
# 冷えているかどうかは `--pinentry-mode error` の試し署名で判定できる。この
# モードは定義上パスフレーズを一切要求せず、キャッシュがあれば成功するだけ
# なので、判定そのものがプロンプトを出すことはない。
#
# 縮退(すべて exit 0。SessionStart は exit 2 でもブロックできない):
#   対象外 → 完全沈黙: gpg/git 不在、gpg.format が openpgp 以外、
#            commit.gpgsign が true でない、user.signingkey が空、
#            signingkey がオンディスクで利用可能でない(card-backed/不明を含む)、
#            すでに温まっている
#   温める → 本番の gpg 呼び出しを 1 回だけ行う。`--pinentry-mode ask` で
#            明示的に要求し、`timeout 90` で hook 自身が先に刈る。
#            Claude Code 側の timeout(120)に決して到達しないので、刈られた
#            pinentry が孤児として残って grab がプロンプト入力に割り込む事故が
#            原理的に起きない。
#   失敗   → stderr 1 行(刈られた/キャンセルされた): 「温められなかった。
#            最初の git commit でダイアログが出る」
#
# 使い方:
#   hook として: settings.json の SessionStart (matcher: startup|resume) から
#                stdin JSON で呼ばれる(stdin は読み捨てる — cwd に依存しないので
#                内容を使わない)
#   自己検査:   sign-prewarm.sh --selftest
set -u

# scope なしで読む(--global は使わない)。$1=key
# repo 外(mktemp -d)から読むことで、ローカル上書きに引っ張られずグローバル値だけを
# 見る。GIT_CONFIG_NOSYSTEM は付けない — system config でグローバルに上書きされて
# いる可能性もそのまま尊重する。
global_git_config() {
  git -C "$PROBE_DIR" config --get "$1" 2>/dev/null || true
}

# 鍵 $1 がオンディスクで利用可能か(field 15 == "+")を判定する。
# card-backed(token S/N)・simple stub(#)・不明(空)はいずれも false。
key_is_on_disk() {
  local key="$1"
  "$GPG_BIN" --list-secret-keys --with-colons --with-fingerprint "$key" 2>/dev/null | awk -F: -v want="$key" '
    $1 == "sec" || $1 == "ssb" { cur_f15 = $15; have_cur = 1; next }
    $1 == "fpr" && have_cur {
      fpr = $10
      if (fpr == want || (index(want, fpr) == 1) || (index(fpr, want) == 1)) {
        print cur_f15
      }
      have_cur = 0
    }
  ' | grep -qx '+'
}

# キャッシュが温まっているか。`--pinentry-mode error` は定義上プロンプトを出さず、
# キャッシュがあれば成功、なければ失敗するだけ。$1=key
is_warm() {
  local key="$1"
  printf '' | "$GPG_BIN" --batch --no-tty --pinentry-mode error \
    --local-user "$key" --detach-sign -o /dev/null 2>/dev/null
}

# 本番の温め呼び出し。--batch は付けない(agent に pinentry を呼ばせるため)。
# hook 自身が timeout で先に刈るので、Claude Code 側の強制終了(timeout=120)には
# 決して到達せず、pinentry が孤児として残らない。$1=key
warm_up() {
  local key="$1"
  timeout 90 "$GPG_BIN" --pinentry-mode ask --no-tty \
    --local-user "$key" --detach-sign -o /dev/null </dev/null 2>/dev/null
}

# hook 本体。$1(省略可)=PROBE_DIR を差し替えるためのオーバーライド(selftest 用)。
run_prewarm() {
  command -v "$GPG_BIN" >/dev/null 2>&1 || return 0
  command -v git >/dev/null 2>&1 || return 0

  PROBE_DIR="$(mktemp -d)"
  trap 'rm -rf "$PROBE_DIR"' RETURN

  local fmt sign key
  fmt="$(global_git_config gpg.format)"
  [[ -z "$fmt" || "$fmt" == "openpgp" ]] || return 0

  sign="$(global_git_config commit.gpgsign)"
  [[ "$sign" == "true" ]] || return 0

  key="$(global_git_config user.signingkey)"
  [[ -n "$key" ]] || return 0

  key_is_on_disk "$key" || return 0

  is_warm "$key" && return 0

  if ! warm_up "$key"; then
    printf '[sign-prewarm] 署名鍵 %s を温められませんでした(キャンセルまたはタイムアウト)。最初の git commit で pinentry が出ます。\n' \
      "$key" >&2
  fi
  return 0
}

# --- サブコマンド: --selftest --------------------------------------------------
if [[ "${1:-}" == "--selftest" ]]; then
  self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
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

  # gpg スタブ: SIGN_PREWARM_STUB_* で挙動を制御する。
  #   SIGN_PREWARM_STUB_LIST_FILE  : --list-secret-keys --with-colons の出力
  #   SIGN_PREWARM_STUB_WARM       : 1 なら --pinentry-mode error(温度判定)成功
  #   SIGN_PREWARM_STUB_WARMUP_RC  : 本番呼び出し(--pinentry-mode ask)の exit code
  #   SIGN_PREWARM_STUB_CALLS_FILE : 本番呼び出し(--pinentry-mode ask)が起きた回数を記録
  mkdir -p "$dir/bin"
  cat >"$dir/bin/gpg-stub" <<'STUB'
#!/usr/bin/env bash
args="$*"
case "$args" in
  *--list-secret-keys*)
    if [[ -n "${SIGN_PREWARM_STUB_LIST_FILE:-}" && -f "$SIGN_PREWARM_STUB_LIST_FILE" ]]; then
      cat "$SIGN_PREWARM_STUB_LIST_FILE"
    fi
    exit 0
    ;;
  *--pinentry-mode\ error*)
    [[ "${SIGN_PREWARM_STUB_WARM:-0}" == "1" ]] && exit 0
    exit 1
    ;;
  *--pinentry-mode\ ask*)
    if [[ -n "${SIGN_PREWARM_STUB_CALLS_FILE:-}" ]]; then
      echo 1 >>"$SIGN_PREWARM_STUB_CALLS_FILE"
    fi
    exit "${SIGN_PREWARM_STUB_WARMUP_RC:-0}"
    ;;
  *)
    exit 0
    ;;
esac
STUB
  chmod +x "$dir/bin/gpg-stub"

  mkrepo() { # mkrepo <path> [local_gpgsign]
    mkdir -p "$1"
    git -C "$1" init -q
    if [[ -n "${2:-}" ]]; then
      git -C "$1" config commit.gpgsign "$2"
    fi
  }

  run_case() { # run_case <cwd> <global_gpgsign> <global_signingkey> <global_format>
    local cwd="$1" g_sign="$2" g_key="$3" g_fmt="$4"
    local home_dir="$dir/home_$RANDOM"
    mkdir -p "$home_dir"
    git config -f "$home_dir/gitconfig" commit.gpgsign "$g_sign" 2>/dev/null || true
    [[ -n "$g_key" ]] && git config -f "$home_dir/gitconfig" user.signingkey "$g_key"
    [[ -n "$g_fmt" ]] && git config -f "$home_dir/gitconfig" gpg.format "$g_fmt"
    GIT_CONFIG_GLOBAL="$home_dir/gitconfig" GIT_CONFIG_SYSTEM=/dev/null \
      GPG_BIN="$dir/bin/gpg-stub" \
      PATH="$dir/bin:$PATH" \
      bash -c "cd '$cwd' && source '$self'; run_prewarm" \
      >"$dir/out" 2>"$dir/err"
    echo $?
  }

  echo "対象外(完全沈黙):"

  norepo="$dir/norepo"
  mkdir -p "$norepo"
  # 実機(gpg --list-secret-keys --with-colons --with-fingerprint)から取った実際の
  # 行を雛形にする(手書きのコロン区切りはフィールド数を誤りやすい —
  # field 15 の位置は実測で固定済み: sec/ssb 行 19 フィールド中の 15 番目)。
  cat >"$dir/list-ondisk.txt" <<'EOF'
sec:u:255:22:6CFC837175BE257E:1753115795:::u:::scESCA:::D2760001240100000006246379980000::ed25519:::0:
fpr:::::::::92E7B05978F0FE4E5500F6F76CFC837175BE257E:
ssb:u:255:22:8608A3F925E329CC:1783930808:1815466808:::::s:::+::ed25519::
fpr:::::::::57B25182FB450B06570860488608A3F925E329CC:
EOF

  rc="$(SIGN_PREWARM_STUB_LIST_FILE="$dir/list-ondisk.txt" run_case "$norepo" "true" \
    "57B25182FB450B06570860488608A3F925E329CC" "openpgp")"
  check "commit.gpgsign 未設定(グローバルのみ true): exit 0" 0 "$rc"
  check "commit.gpgsign 未設定: stdout 空" "" "$(cat "$dir/out")"

  rc="$(SIGN_PREWARM_STUB_LIST_FILE="$dir/list-ondisk.txt" run_case "$norepo" "false" \
    "57B25182FB450B06570860488608A3F925E329CC" "openpgp")"
  check "commit.gpgsign=false: exit 0" 0 "$rc"
  check "commit.gpgsign=false: stdout 空" "" "$(cat "$dir/out")"
  check "commit.gpgsign=false: stderr 空" "" "$(cat "$dir/err")"

  rc="$(SIGN_PREWARM_STUB_LIST_FILE="$dir/list-ondisk.txt" run_case "$norepo" "true" \
    "" "openpgp")"
  check "user.signingkey 空: exit 0" 0 "$rc"
  check "user.signingkey 空: stdout 空" "" "$(cat "$dir/out")"

  rc="$(SIGN_PREWARM_STUB_LIST_FILE="$dir/list-ondisk.txt" run_case "$norepo" "true" \
    "57B25182FB450B06570860488608A3F925E329CC" "ssh")"
  check "gpg.format=ssh: exit 0" 0 "$rc"
  check "gpg.format=ssh: stdout 空" "" "$(cat "$dir/out")"

  echo "field 15 の 3 値:"

  cat >"$dir/list-card.txt" <<'EOF'
sec:u:255:22:6CFC837175BE257E:1753115795:::u:::scESCA:::D2760001240100000006246379980000::ed25519:::0:
fpr:::::::::92E7B05978F0FE4E5500F6F76CFC837175BE257E:
ssb:u:255:22:4DB3C00BA34556B0:1753115911::::::a:::D2760001240100000006246379980000::ed25519::
fpr:::::::::CARDCARDCARDCARDCARDCARDCARDCARDCARDCARD:
EOF
  rc="$(SIGN_PREWARM_STUB_LIST_FILE="$dir/list-card.txt" SIGN_PREWARM_STUB_WARM=0 \
    SIGN_PREWARM_STUB_CALLS_FILE="$dir/calls-card.txt" \
    run_case "$norepo" "true" "CARDCARDCARDCARDCARDCARDCARDCARDCARDCARD" "openpgp")"
  check "card-backed(token S/N): exit 0" 0 "$rc"
  check "card-backed: 本番 gpg を呼ばない" "" "$(cat "$dir/calls-card.txt" 2>/dev/null || true)"

  cat >"$dir/list-stub.txt" <<'EOF'
sec:u:255:22:6CFC837175BE257E:1753115795:::u:::scESCA:::D2760001240100000006246379980000::ed25519:::0:
fpr:::::::::92E7B05978F0FE4E5500F6F76CFC837175BE257E:
ssb:r:255:22:282AE4C1E0CAC57A:1753371700:1784907700:::::s:::#::ed25519::
fpr:::::::::STUBSTUBSTUBSTUBSTUBSTUBSTUBSTUBSTUBSTUB:
EOF
  rc="$(SIGN_PREWARM_STUB_LIST_FILE="$dir/list-stub.txt" SIGN_PREWARM_STUB_WARM=0 \
    SIGN_PREWARM_STUB_CALLS_FILE="$dir/calls-stub.txt" \
    run_case "$norepo" "true" "STUBSTUBSTUBSTUBSTUBSTUBSTUBSTUBSTUBSTUB" "openpgp")"
  check "simple stub(#): exit 0" 0 "$rc"
  check "simple stub: 本番 gpg を呼ばない" "" "$(cat "$dir/calls-stub.txt" 2>/dev/null || true)"

  rc="$(SIGN_PREWARM_STUB_LIST_FILE="$dir/list-ondisk.txt" SIGN_PREWARM_STUB_WARM=0 \
    SIGN_PREWARM_STUB_CALLS_FILE="$dir/calls-ondisk.txt" \
    run_case "$norepo" "true" "57B25182FB450B06570860488608A3F925E329CC" "openpgp")"
  check "オンディスク(+) かつ cold: exit 0" 0 "$rc"
  check "オンディスク かつ cold: 本番 gpg を 1 回呼ぶ" "1" "$(wc -l <"$dir/calls-ondisk.txt" | tr -d ' ')"

  echo "温度判定:"

  rc="$(SIGN_PREWARM_STUB_LIST_FILE="$dir/list-ondisk.txt" SIGN_PREWARM_STUB_WARM=1 \
    SIGN_PREWARM_STUB_CALLS_FILE="$dir/calls-warm.txt" \
    run_case "$norepo" "true" "57B25182FB450B06570860488608A3F925E329CC" "openpgp")"
  check "既に warm: exit 0" 0 "$rc"
  check "既に warm: 本番 gpg を呼ばない" "" "$(cat "$dir/calls-warm.txt" 2>/dev/null || true)"
  check "既に warm: stdout 空" "" "$(cat "$dir/out")"
  check "既に warm: stderr 空" "" "$(cat "$dir/err")"

  rc="$(SIGN_PREWARM_STUB_LIST_FILE="$dir/list-ondisk.txt" SIGN_PREWARM_STUB_WARM=0 \
    SIGN_PREWARM_STUB_WARMUP_RC=1 \
    run_case "$norepo" "true" "57B25182FB450B06570860488608A3F925E329CC" "openpgp")"
  check "cold かつ本番失敗: exit 0" 0 "$rc"
  check "cold かつ本番失敗: stdout 空" "" "$(cat "$dir/out")"
  check "cold かつ本番失敗: stderr 非空" 1 "$([[ -s "$dir/err" ]] && echo 1 || echo 0)"

  echo "cwd 非依存の回帰テスト(R1-A-2):"

  nonsignrepo="$dir/nonsignrepo"
  mkrepo "$nonsignrepo" "false"
  rc="$(SIGN_PREWARM_STUB_LIST_FILE="$dir/list-ondisk.txt" SIGN_PREWARM_STUB_WARM=0 \
    SIGN_PREWARM_STUB_CALLS_FILE="$dir/calls-cwd.txt" \
    run_case "$nonsignrepo" "true" "57B25182FB450B06570860488608A3F925E329CC" "openpgp")"
  check "local commit.gpgsign=false な repo から起動: exit 0" 0 "$rc"
  check "local commit.gpgsign=false でもグローバル有効なら本番 gpg を 1 回呼ぶ" "1" \
    "$(wc -l <"$dir/calls-cwd.txt" | tr -d ' ')"

  [[ "$fail" == 0 ]] && echo "selftest: all passed"
  exit "$fail"
fi

# --- hook 本体(SessionStart) --------------------------------------------------
GPG_BIN="${GPG_BIN:-gpg}"
# selftest がこのファイルを source して run_prewarm を個別に呼ぶため、直接実行
# (bash sign-prewarm.sh)されたときだけ以下を走らせる。source 時は BASH_SOURCE[0]
# と $0 が一致しない。
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  cat >/dev/null || true # stdin を読み捨てる(cwd に依存しないので内容を使わない)
  run_prewarm
  exit 0
fi
