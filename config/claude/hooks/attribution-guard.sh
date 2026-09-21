#!/usr/bin/env bash
# attribution-guard.sh — Claude が GitHub に書く外向きテキストに attribution
# フッターが載っていることを保証する PreToolUse hook。
#
# 設計と根拠: docs/claude/attribution-guard.md(このリポジトリ内)
#
# PR / Issue 本文に付く「🤖 Generated with [Claude Code](...)」フッターは、この
# リポジトリのルールではなく harness 側の attribution 指示(セッションごとに
# 注入される system-reminder)由来である。そのため 2 つの穴があった:
#
#   1. コメント投稿には一切付かない。gh pr comment / gh issue comment /
#      gh pr review --body で Claude が投稿したテキストは、GitHub 上では人間の
#      発言と区別が付かない。mention を含むコメントは相手に直接通知が飛ぶため、
#      人間の発言と誤読されるコストが最も高い。
#   2. PR / Issue 本文側も保証されていない。pr-gate.sh は G_link / G_visual を
#      検査するが attribution は見ていない。harness の指示が変わる・欠ける・
#      セッションによって注入されないと、repo 側は何も気付かずに静かに落ちる。
#
# 判定は 1 つだけ:
#   G_attr : 投稿本文に attribution フッター または No-Attribution: → deny
#
# なぜ Stop hook ではなく PreToolUse か: コメント投稿は通知が飛ぶ不可逆操作で、
# 事後に怒っても取り返せない。pr-gate.sh の Stop 判定群とは構造が違う。
#
# なぜ mention の有無で分岐しないか: mention が無くても watcher / assignee /
# subscriber には通知が飛ぶので「人間に届くか」は mention の有無で決まらない。
# 加えて `@` の出現をパースする条件分岐は偽陰性を生む(コードブロック内の @、
# メールアドレス、@ 無しで名前を書くケース)。AI 生成物である事実は読者が誰かに
# 依存しない。
#
# 抜け道は本文マーカー `No-Attribution: <理由>`(pr-gate.sh の No-Issue: /
# No-Visual: と同型)。理由を伴って初めて成立する。deny の理由文にはこの抜け道を
# 明示的に書く — publish-guard は verdict_reason() に「bypass 手段はここに
# 書かない」と逆方針を採っているが、あちらは漏洩防止で抜け道を教えると自分で
# 抜けてしまう。No-Attribution: は正当な判断なので、使えないと意味がない。
#
# 検査範囲の切り出し(ここが一番の設計上の勘所):
#   コマンド文字列**全体**でマーカーを探してはいけない。
#     gh pr create --body "…🤖 Generated…" && gh pr comment 1 --body "短い"
#   という Claude が自然に書く形で、comment 側が create 側のフッターによって
#   通ってしまう。よって「対象コマンドの出現位置から、次の対象コマンドの出現
#   位置まで」を 1 投稿ぶんの範囲として切り出し、範囲ごとに独立に判定する。
#
#   区切りをシェル metachar(`;` `&&` `||` `|`)にしない理由: 長文コメントの
#   典型形 --body "$(cat <<'EOF' … EOF)" は本文中に metachar を含みうる。
#   metachar で切ると本文が後段セグメントに落ちてマーカーを見失い、false deny
#   が頻発する。「次の対象コマンドまで」で切れば heredoc 内の metachar は
#   打ち切り要因にならない。
#
# 対象コマンドの検出は「コマンド位置」に限る(ここを外すと実用にならない):
#   コマンド文字列全体を正規表現で見る実装は、コミットメッセージや docs に
#   投稿コマンドの綴りを書いただけで発火する。この hook 自身を commit しよう
#   としてまさにそれで止まり、続けて修正用スクリプトのコメント文でも止まった。
#   git-stash-guard.sh:139-148 が同じ罠(自分のファイルパスに含まれる "stash")
#   を記録しているが、あちらと違い投稿コマンドの綴りはこのリポジトリの docs と
#   コミットメッセージに頻出するため、粗い判定では運用が回らない。
#
#   よって 2 段で絞る:
#     1. heredoc 本体を分離する(split_heredoc)。本体の各行は改行の直後に
#        来るので、分離しないと本体に書いた例文がコマンド位置に見える。
#        分離した本体は捨てずに保持し、--body "$(cat <<'TAG' … TAG)" の
#        本文候補として使う。
#     2. 残りをトークン化し、先頭または区切りトークン(CMD_SEPS)の直後に
#        ある gh 呼び出しだけを対象とする。クォートされた文字列は 1 トークンに
#        なるのでコマンド位置には来ない。
#
# 本文の抽出:
#   範囲内から --body / --body-file の**値だけ**を取り出す。これをしないと
#   `--title "🤖 Generated with [Claude Code]" --body "x"` が通ってしまう。
#
#   トークン分割に xargs を使わない理由(実測): GNU xargs はクォート内の改行を
#   扱えず "unmatched single quote" で失敗する。改行を含む本文は長文コメントの
#   典型なので、xargs では主要ケースが常にフォールバックに落ちる。
#
#   read -r -a の素朴な空白分割も使えない(git-stash-guard.sh はこれを採って
#   いるが、あちらが扱うのは値に空白が入らない短いフラグ列)。Markdown 本文には
#   `- 箇条書き` が必ず出るため、「次の `-` 始まりトークンまで」で切ると本文が
#   毎回途中で切れ、末尾のフッターを常に見失って全 deny になる。
#
#   よってクォートを解釈する自前トークナイザを持つ。LC_ALL=C のバイト単位走査で
#   UTF-8 は安全(継続バイトは 0x80-0xBF で ASCII のクォート・空白と衝突しない)。
#
# 抽出不能時の倒し方(経路ごとに変える):
#   heredoc(`<<`)を含む            → 本体が command 文字列内に実在するので、
#                                     範囲文字列全体をフォールバックで検査する
#   コマンド置換のみ($( / `)       → 中身が不明なので判定不能 → 通す。ここを
#                                     deny に倒すと --body "$(cat body.md)" が
#                                     常に弾かれる
#   本文フラグが無い                → 判定不能 → 通す(gh pr edit --add-label)
#   トークナイザが unmatched quote  → heredoc と同じフォールバック
#
# 縮退(ADR-0005 の binary-existence gating に倣う): jq 不在・stdin 不正は黙って
# exit 0。判定できない場合は断定に変えず素通す(pr-gate.sh の縮退表と同じ)。
#
# 既知の限界(意図的な選択、docs/claude/attribution-guard.md に詳しい):
#   - gh api は外向き投稿の URL パス(issues/pulls の作成・編集・コメント、
#     pulls のレビュー作成)のみ判定する(#195)。pulls/N/comments のような
#     インラインレビューコメントの経路は判定対象外のまま(#194 の決定を維持)。
#     GET/HEAD/DELETE は明示 -X/--method があるときだけ除外し、それ以外は
#     本文フラグの有無で自然に判定不能(通す)に落ちる。
#   - コード行へのインラインレビューコメントは対象外(1〜2 行が典型でフッターが
#     本文を圧迫する)
#   - フォールバック経路では --title 等の他フラグにマーカーがあると通る
#
# Codex CLI / Copilot CLI への展開(#192、裁定: エージェント名入りの文言):
#   この判定エンジン(decide/decide_tokens/decide_api_tokens/decide_mcp/
#   has_marker/emit_deny 等)はエージェント非依存で、config/codex/hooks/
#   attribution-guard.sh・config/copilot/hooks/attribution-guard.sh から
#   `source` される(publish-guard の「1つの判定エンジン + 薄い per-agent
#   adapter」という既存の型を踏襲、tarotene/publish-guard の
#   adapters/{codex,copilot}-adapter.sh が同型)。ATTRIBUTION_AGENT_NAME /
#   ATTRIBUTION_AGENT_URL を adapter 側が source 前に上書きすることで
#   フッター文言だけがエージェントごとに変わる。ファイル末尾の実行時
#   ディスパッチは `source` 時に暴発しないよう `[[ "${BASH_SOURCE[0]}" ==
#   "$0" ]]` で直接実行時のみに限定してある。
#   MCP tool 名の命名規則は Codex/Copilot とも未確認(#161)のため、
#   `decide_mcp`(mcp__github* 前提)は Codex/Copilot の adapter からは
#   呼ばない — Bash 経由の `gh` コマンドのみが対象。
#
# 使い方:
#   hook として: settings.json の PreToolUse(matcher: "Bash|mcp__.*")から
#                stdin JSON で呼ばれる
#   手動 e2e:   attribution-guard.sh --check '<コマンド文字列>'
#   自己検査:   attribution-guard.sh --selftest(ネットワーク不使用)
set -euo pipefail

# バイト単位で扱う(トークナイザの ${s:i:1} と grep -bo のオフセットを揃える)。
export LC_ALL=C

# ---------------------------------------------------------------------------
# 定数
# ---------------------------------------------------------------------------

# エージェント名/リンク先(#192): adapter が source 前に上書きすることで
# Codex CLI / Copilot CLI 向けのフッター文言に切り替えられる。既定値は
# Claude Code 自身の直接実行時の値(後方互換 — このリポジトリの唯一の
# 文言箇所という D4 の原則は agent ごとに 1 箇所ずつという形で維持する)。
ATTRIBUTION_AGENT_NAME="${ATTRIBUTION_AGENT_NAME:-Claude Code}"
ATTRIBUTION_AGENT_URL="${ATTRIBUTION_AGENT_URL:-https://claude.com/claude-code}"

# 要求するフッター(repo 内でこの 1 箇所だけが文言を持つ、D4)。
ATTRIBUTION_FOOTER="🤖 Generated with [${ATTRIBUTION_AGENT_NAME}](${ATTRIBUTION_AGENT_URL})"

# 検出は緩め — 文言の軽微なズレで false deny しない。CLAUDE.md の指示は厳密。
# alternation の "Filed from" は wrap-up inbox の出自フッター(統合形
# 「🤖 Filed from [Claude Code](...) wrap-up inbox」)が生成元表示を兼ねる
# ケースを受理するためのもの(D1)。旧 2 行形式(Generated with 行を別途
# 持つ)も alternation の前段でそのまま通る。wrap-up inbox は Claude Code
# 専用機能だが、エージェント名を差し替えても害はないため汎用の形にしてある。
ATTRIBUTION_RE="(Generated with|Filed from)[[:space:]]*\\[?${ATTRIBUTION_AGENT_NAME}"

# 理由を伴って初めて成立する。空の No-Attribution: は通さない。
# 除外集合はフォールバック経路で効く: 範囲文字列全体を見る場合、
# --body '確認しました No-Attribution:' の閉じクォートが理由として通ってしまう。
NO_ATTRIBUTION_RE="No-Attribution:[[:space:]]*[^[:space:]'\"\`)]"

# コマンドを区切るトークン(この直後が「コマンド位置」になる)。対象コマンドの
# 検出をコマンド位置に限るための材料。コマンド文字列全体を正規表現で見る実装は、
# コミットメッセージや docs に投稿コマンドの例を書いただけで発火する — 実際に
# この hook 自身を commit しようとして踏んだ。git-stash-guard.sh:139-148 が同じ罠を
# 自分のファイルパスで踏んだ記録を残しているが、あちらの "stash" と違い投稿
# コマンドの綴りはこのリポジトリの docs / コミットメッセージに頻出するため、
# 粗い判定では実用に耐えない。
CMD_SEPS=$';|&()\n'

# MCP GitHub の書き込み系 tool(現在未接続、#161 と同じく命名は未確認なので
# 保守的に名前で絞る)。read 系を deny すると壊れるので、書き込みを示す語を
# 含むものだけを対象にする。
MCP_WRITE_RE='(comment|create_issue|create_pull|update_issue|update_pull|create_review|submit_review|add_issue_comment|add_comment)'

# ---------------------------------------------------------------------------
# 判定ロジック(selftest がネットワーク無しに検査できる純関数群)
# ---------------------------------------------------------------------------

# $1=テキスト; attribution フッターか No-Attribution: <理由> があれば 0。
has_marker() {
  grep -qE "$ATTRIBUTION_RE" <<< "$1" && return 0
  grep -qE "$NO_ATTRIBUTION_RE" <<< "$1" && return 0
  return 1
}

# deny の理由文。抜け道を明示的に書く(publish-guard とは逆方針、冒頭参照)。
deny_reason() {
  printf '%s' "GitHub に投稿する本文に attribution がありません(deny)。本文の末尾に「${ATTRIBUTION_FOOTER}」を追記してください。本文が Claude 生成でない場合(ユーザーの逐語をそのまま代理投稿する等)は、本文に「No-Attribution: <理由>」と書いて明示的に抜けてください。"
}

# $1=cmd; heredoc 本体を分離する。
#   CMD_NOHD  = 本体を除いたコマンド文字列(コマンド位置の判定に使う)
#   HD_BODIES = 本体の連結(本文候補として使う)
#
# 本体を除かずにコマンド位置を判定すると、heredoc で流し込む文章の中に書いた
# 「PR へコメントを投稿する呼び出しの書き方」がコマンド位置に見えてしまう
# (本体の各行は改行の直後に来るため)。実際にこの hook 自身のコミットメッセージ
# (投稿コマンドの例を本文に含む)で踏んだ。
#
# `<<<`(herestring)は heredoc ではない。`<<` の次が `<` なのでタグの正規表現に
# マッチせず、自動的に除外される。
split_heredoc() {
  local line tag="" in_body=0
  CMD_NOHD=""
  HD_BODIES=""
  while IFS= read -r line || [[ -n $line ]]; do
    if ((in_body)); then
      if [[ $line =~ ^[[:space:]]*"$tag"[[:space:]]*$ ]]; then
        in_body=0
        tag=""
      else
        HD_BODIES+="$line"$'\n'
      fi
      continue
    fi
    CMD_NOHD+="$line"$'\n'
    if [[ $line =~ \<\<-?[[:space:]]*[\'\"]?([A-Za-z_][A-Za-z0-9_]*)[\'\"]? ]]; then
      tag="${BASH_REMATCH[1]}"
      in_body=1
    fi
  done <<< "$1"
}

# $1=文字列; クォートを解釈してトークンを NUL 区切りで stdout に出す。
# コマンドの区切り(CMD_SEPS)は独立トークンとして残す。
# unmatched quote / トークン 0 件なら非 0。
#
# xargs も read -r -a も使えない理由は冒頭のコメントブロック参照。
tokenize() {
  local s="$1" n=${#1} i=0 c st=none cur="" started=0
  local -a out=()
  while ((i < n)); do
    c="${s:i:1}"
    case "$st" in
      none)
        case "$c" in
          ' ' | $'\t' | $'\r')
            if ((started)); then
              out+=("$cur")
              cur=""
              started=0
            fi
            ;;
          ';' | '|' | '&' | '(' | ')' | $'\n')
            if ((started)); then
              out+=("$cur")
              cur=""
              started=0
            fi
            out+=("$c")
            ;;
          "'") st=sq; started=1 ;;
          '"') st=dq; started=1 ;;
          '\')
            i=$((i + 1))
            cur+="${s:i:1}"
            started=1
            ;;
          *)
            cur+="$c"
            started=1
            ;;
        esac
        ;;
      sq)
        if [[ $c == "'" ]]; then st=none; else cur+="$c"; fi
        ;;
      dq)
        case "$c" in
          '"') st=none ;;
          '\')
            i=$((i + 1))
            cur+="${s:i:1}"
            ;;
          *) cur+="$c" ;;
        esac
        ;;
    esac
    i=$((i + 1))
  done
  [[ $st == none ]] || return 1
  ((started)) && out+=("$cur")
  ((${#out[@]} > 0)) || return 1
  printf '%s\0' "${out[@]}"
}

# $1=トークン; コマンドの区切りなら 0。
is_sep() {
  [[ ${#1} -eq 1 && $CMD_SEPS == *"$1"* ]]
}

# is_target_at() が対象位置を見つけたときにセットする判定範囲の種別。
# cli: 従来の gh pr/issue サブコマンド / api: gh api の URL パス判定(#195)。
TARGET_KIND=""

# グローバル TOK の $1 番目が対象コマンドの先頭なら 0。
#   gh (pr|issue) (create|edit|comment)  /  gh pr review  /  gh api ...
# gh は素の `gh` でもフルパス(/usr/bin/gh)でもよい。gh api はサブコマンドの
# 有無だけで判定範囲を開始し、対象パスかどうかは decide_api_tokens() 側で
# 判定する(-X/--method がパスより先に来る形もあるため、ここではパス位置を
# 固定しない)。
is_target_at() {
  local i=$1 n=${#TOK[@]} base
  ((i + 1 < n)) || return 1
  base="${TOK[i]##*/}"
  [[ $base == gh ]] || return 1
  if [[ ${TOK[i + 1]} == api ]]; then
    TARGET_KIND=api
    return 0
  fi
  ((i + 2 < n)) || return 1
  TARGET_KIND=cli
  case "${TOK[i + 1]}" in
    pr)
      case "${TOK[i + 2]}" in
        create | edit | comment | review) return 0 ;;
      esac
      ;;
    issue)
      case "${TOK[i + 2]}" in
        create | edit | comment) return 0 ;;
      esac
      ;;
  esac
  return 1
}

# 引数 = 1 投稿ぶんのトークン列。deny なら理由文を stdout に出して 0、
# 通すなら非 0。
decide_tokens() {
  local -a tok=("$@")
  local n=${#tok[@]} i=0 have_flag=0 has_hd=0 p
  local -a texts=()

  # heredoc リダイレクトは `--body "$(cat <<'TAG' … TAG)"` のように値トークンの
  # 内側に来ることがあるので、独立トークンとしてではなく部分文字列で先に見る。
  for ((i = 0; i < n; i++)); do
    if [[ ${tok[i]} == *'<<'* ]]; then
      has_hd=1
      break
    fi
  done

  i=0
  while ((i < n)); do
    case "${tok[i]}" in
      --body | -b)
        have_flag=1
        if ((i + 1 < n)); then
          texts+=("${tok[i + 1]}")
          i=$((i + 2))
          continue
        fi
        ;;
      --body=*)
        have_flag=1
        texts+=("${tok[i]#--body=}")
        ;;
      --body-file | -F)
        have_flag=1
        if ((i + 1 < n)); then
          p="${tok[i + 1]}"
          [[ -f $p && -r $p ]] && texts+=("$(cat -- "$p")")
          i=$((i + 2))
          continue
        fi
        ;;
      --body-file=*)
        have_flag=1
        p="${tok[i]#--body-file=}"
        [[ -f $p && -r $p ]] && texts+=("$(cat -- "$p")")
        ;;
    esac
    i=$((i + 1))
  done

  ((have_flag)) || return 1 # 本文フラグ無し → 判定不能で通す

  local text=""
  ((${#texts[@]} > 0)) && text="$(printf '%s\n' "${texts[@]}")"

  if ((has_hd)) && [[ -n ${HD_BODIES:-} ]]; then
    # この範囲が heredoc を使っているなら本体を本文候補に加える
    # (--body "$(cat <<'TAG' … TAG)" の形)。1 コマンド中に複数の heredoc が
    # ある場合の対応付けはしない(既知の限界)。
    text+=$'\n'"$HD_BODIES"
  elif [[ $text == *'$('* || $text == *'`'* ]]; then
    return 1 # 本文がコマンド置換 → 中身が不明 → 判定不能で通す
  fi

  [[ -n ${text//[[:space:]]/} ]] || return 1 # 値が取れない → 判定不能で通す

  has_marker "$text" && return 1
  deny_reason
  return 0
}

# gh api の判定対象エンドポイント(#195)。issues/pulls への外向き投稿のみ。
# pulls への `/comments` 系(インラインレビューコメント)はこの正規表現の
# alternation に無い(pulls は `(/[0-9]+)?(/reviews)?` までしか許さない)ため
# 自然に非対象になる — #194 の「インラインは対象外」の決定を追加の除外
# regex 無しでそのまま維持できる。
API_JUDGED_RE='^/repos/[^/]+/[^/]+/(issues(/[0-9]+)?(/comments)?|issues/comments/[0-9]+|pulls(/[0-9]+)?(/reviews)?)$'

# $@=1 投稿ぶんの `gh api ...` トークン列; deny なら理由文を stdout に出して
# 0、通すなら非 0。decide_tokens() と対になる gh api 版(#195)。
#
# 対象化の手順:
#   1. -X/--method の値が GET/HEAD/DELETE なら判定不能で通す(実測ベース。
#      既定メソッドの推測はしない — 値が明示されていない場合は下の本文フラグ
#      チェックで自然に判定不能に落ちる)。
#   2. 範囲内トークンから API_JUDGED_RE にマッチする最初のパスを探す。無ければ
#      非対象パス(rulesets 等)として通す — apply-rulesets.sh の
#      `gh api -X POST repos/.../rulesets --input -` を誤判定しない回帰要件。
#   3. 本文は -f/--field/-F/--raw-field の `body=<値>`、または --input <file>
#      (jq で .body を読む)。--input - は判定不能(有効な texts を積まない)。
decide_api_tokens() {
  local -a tok=("$@")
  local n=${#tok[@]} i=0 have_flag=0 has_hd=0 p method='' path='' body t
  local -a texts=()

  # 各 for ループに `|| true` を付ける: ループ最後の反復で内側の `[[ ]]`/
  # `case` が非マッチ(非 0)のまま終わると for 文自体の終了ステータスも
  # 非 0 になり、set -e がこの関数ごと即死させる(if/while の条件式と
  # 違い for 文自体はこの除外規則の対象外)。decide_tokens 側の
  # `while ((i < n))` ループが毎周を `i=$((i + 1))` という必ず成功する文で
  # 終えているのと同じ理由の別解。
  for ((i = 0; i < n; i++)); do
    if [[ ${tok[i]} == *'<<'* ]]; then
      has_hd=1
      break
    fi
  done || true

  for ((i = 0; i < n; i++)); do
    case "${tok[i]}" in
      -X | --method)
        ((i + 1 < n)) && method="${tok[i + 1]}"
        ;;
      --method=*)
        method="${tok[i]#--method=}"
        ;;
    esac
  done || true
  case "${method^^}" in
    GET | HEAD | DELETE) return 1 ;;
  esac

  for ((i = 0; i < n; i++)); do
    t="/${tok[i]#/}"
    if [[ $t =~ $API_JUDGED_RE ]]; then
      path="$t"
      break
    fi
  done || true
  [[ -n $path ]] || return 1 # 対象エンドポイントでない → 通す

  i=0
  while ((i < n)); do
    case "${tok[i]}" in
      -f | --field | -F | --raw-field)
        if ((i + 1 < n)) && [[ ${tok[i + 1]} == body=* ]]; then
          have_flag=1
          texts+=("${tok[i + 1]#body=}")
          i=$((i + 2))
          continue
        fi
        ;;
      --field=body=* | --raw-field=body=*)
        have_flag=1
        texts+=("${tok[i]#*=body=}")
        ;;
      --input)
        if ((i + 1 < n)); then
          p="${tok[i + 1]}"
          have_flag=1
          if [[ $p != - && -f $p && -r $p ]]; then
            body="$(jq -r '.body // empty' -- "$p" 2> /dev/null)" || body=""
            [[ -n $body ]] && texts+=("$body")
          fi
          i=$((i + 2))
          continue
        fi
        ;;
      --input=*)
        p="${tok[i]#--input=}"
        have_flag=1
        if [[ $p != - && -f $p && -r $p ]]; then
          body="$(jq -r '.body // empty' -- "$p" 2> /dev/null)" || body=""
          [[ -n $body ]] && texts+=("$body")
        fi
        ;;
    esac
    i=$((i + 1))
  done

  ((have_flag)) || return 1 # 本文フラグ無し(GET 相当含む)→ 判定不能で通す

  local text=""
  ((${#texts[@]} > 0)) && text="$(printf '%s\n' "${texts[@]}")"

  if ((has_hd)) && [[ -n ${HD_BODIES:-} ]]; then
    text+=$'\n'"$HD_BODIES"
  elif [[ $text == *'$('* || $text == *'`'* ]]; then
    return 1 # 本文がコマンド置換 → 判定不能で通す
  fi

  [[ -n ${text//[[:space:]]/} ]] || return 1 # --input - 等、値が取れない → 通す

  has_marker "$text" && return 1
  deny_reason
  return 0
}

# $1=Bash ツールのコマンド文字列全体; deny なら理由文を stdout に出して 0。
#
# 対象コマンドは「コマンド位置」にあるものだけを拾う。コマンド文字列全体を
# 正規表現で見る実装にすると、コミットメッセージや docs に投稿コマンドの例を
# 書いただけで発火する(冒頭の CMD_SEPS のコメント参照)。
#
# 拾った対象コマンドごとに、次の対象コマンドの直前までを 1 投稿ぶんの範囲と
# して切り、範囲ごとに独立に判定する(1 件でも deny なら deny)。
decide() {
  split_heredoc "$1"

  TOK=()
  local t
  while IFS= read -r -d '' t; do TOK+=("$t"); done < <(tokenize "$CMD_NOHD") || true
  ((${#TOK[@]} > 0)) || return 1

  local n=${#TOK[@]} i at_cmd_pos=1
  local -a starts=() kinds=()
  for ((i = 0; i < n; i++)); do
    if ((at_cmd_pos)) && is_target_at "$i"; then
      starts+=("$i")
      kinds+=("$TARGET_KIND")
    fi
    if is_sep "${TOK[i]}"; then at_cmd_pos=1; else at_cmd_pos=0; fi
  done
  ((${#starts[@]} > 0)) || return 1

  # 判定関数の呼び出しは必ず `&&` の非最終項にする(set -e の除外規則に乗せる
  # ため)。$(...) 単独の代入文は、中身が非 0 を返すと set -e でスクリプト
  # ごと落ちる — decide_tokens/decide_api_tokens が pass(非 0)を返すのは
  # 通常経路なので、ここを裸の代入文にすると大半のコマンドで即死する。
  local m=${#starts[@]} s e reason
  for ((i = 0; i < m; i++)); do
    s=${starts[i]}
    if ((i + 1 < m)); then e=${starts[i + 1]}; else e=$n; fi
    if [[ ${kinds[i]} == api ]]; then
      reason="$(decide_api_tokens "${TOK[@]:s:e - s}")" && {
        printf '%s' "$reason"
        return 0
      }
    else
      reason="$(decide_tokens "${TOK[@]:s:e - s}")" && {
        printf '%s' "$reason"
        return 0
      }
    fi
  done
  return 1
}


# $1=tool 名 $2=stdin JSON 全体; deny なら理由文を stdout に出して 0。
#
# MCP GitHub は現在未接続。matcher を Bash 単体にすると接続した瞬間に無検査に
# なる(home/modules/claude.nix が publish-guard の旧実装で同じ欠陥を持っていたと
# 明記している)ので、名前で書き込み系に絞ってから body を見る。
decide_mcp() {
  local tool="$1" input="$2" body
  grep -qE "$MCP_WRITE_RE" <<< "$tool" || return 1
  body="$(jq -r '.tool_input.body // .tool_input.comment // empty' <<< "$input" 2> /dev/null)" || return 1
  [[ -n $body ]] || return 1
  has_marker "$body" && return 1
  deny_reason
  return 0
}

# ---------------------------------------------------------------------------
# hook 入出力(git-stash-guard.sh と同じ契約)
# ---------------------------------------------------------------------------

emit_deny() {
  jq -n --arg reason "$1" '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: $reason
    }
  }'
}

main() {
  command -v jq > /dev/null 2>&1 || exit 0

  local input tool cmd reason
  input="$(cat)" || exit 0

  tool="$(jq -r '.tool_name // empty' <<< "$input" 2> /dev/null)" || exit 0
  case "$tool" in
    Bash)
      cmd="$(jq -r '.tool_input.command // empty' <<< "$input" 2> /dev/null)" || exit 0
      [[ -n $cmd ]] || exit 0
      reason="$(decide "$cmd")" || exit 0
      ;;
    mcp__github*)
      reason="$(decide_mcp "$tool" "$input")" || exit 0
      ;;
    *) exit 0 ;;
  esac

  emit_deny "$reason"
  exit 0
}

# ---------------------------------------------------------------------------
# selftest
# ---------------------------------------------------------------------------

# mktemp のディレクトリは trap から参照するのでグローバルに置く(関数 local だと
# EXIT 時にスコープ外で set -u に触れる)。
SELFTEST_TMP=""
cleanup_selftest() { [[ -n ${SELFTEST_TMP:-} ]] && rm -rf "$SELFTEST_TMP"; }

selftest() {
  local fails=0 tmp
  SELFTEST_TMP="$(mktemp -d)"
  trap cleanup_selftest EXIT
  tmp="$SELFTEST_TMP"

  expect_deny() { # $1=ラベル $2=コマンド
    local out
    if ! out="$(decide "$2")"; then
      echo "FAIL($1: deny 期待、通した): $2" >&2
      fails=$((fails + 1))
    elif [[ -z $out ]]; then
      echo "FAIL($1: deny 期待、理由が空): $2" >&2
      fails=$((fails + 1))
    fi
  }
  expect_pass() { # $1=ラベル $2=コマンド
    if decide "$2" > /dev/null; then
      echo "FAIL($1: pass 期待、deny した): $2" >&2
      fails=$((fails + 1))
    fi
  }

  local footer='🤖 Generated with [Claude Code](https://claude.com/claude-code)'

  printf '本文\n%s\n' "$footer" > "$tmp/with.md"
  printf '本文だけ\n' > "$tmp/without.md"

  # 1: インライン body にフッター
  expect_pass "1 インライン+footer" "gh pr comment 1 --body '確認しました。$footer'"
  # 2: インライン body にフッター無し
  expect_deny "2 インライン-footer" "gh pr comment 1 --body '確認しました。'"
  # 3: 本文フラグ無し(判定不能 → 通す)
  expect_pass "3 body フラグ無し" "gh pr edit 1 --add-label bug"
  # 4: --body-file の中身にフッター
  expect_pass "4 body-file+footer" "gh issue comment 1 --body-file $tmp/with.md"
  # 5: --body-file の中身にフッター無し
  expect_deny "5 body-file-footer" "gh issue comment 1 --body-file $tmp/without.md"
  # 6: --body-file が存在しない(判定不能 → 通す)
  expect_pass "6 body-file 不在" "gh issue comment 1 --body-file $tmp/nope.md"
  # 7: No-Attribution: に理由あり
  expect_pass "7 No-Attribution+理由" "gh pr comment 1 --body 'ユーザーの原文です。No-Attribution: 本文はユーザーの逐語'"
  # 8: 空の No-Attribution:
  expect_deny "8 No-Attribution 理由無し" "gh pr comment 1 --body 'x No-Attribution: '"
  # 9: 閉じクォートを理由と誤認しないか(R1-B-1 後半)
  expect_deny "9 No-Attribution 閉じクォート" "gh pr comment 1 --body '確認しました No-Attribution:'"
  # 10: && 連結で前段のフッターを取り違えないか(R1-B-1 前半)
  expect_deny "10 && 連結の取り違え" "gh pr create --title t --body '本文 $footer' && gh pr comment 1 --body 'x'"
  # 11: 手前の echo からマーカーが混入しないか(R1-B-1 前半)
  expect_deny "11 echo からの混入" "echo '$footer'; gh pr comment 1 --body 'x'"
  # 12: --title のマーカーを本文と誤認しないか(本文抽出の厳密化で塞がる)
  expect_deny "12 --title からの混入" "gh pr create --title '$footer' --body 'x'"
  # 13: Markdown 箇条書きで本文が切れないか(false deny の回帰。これが落ちると実用不可)
  expect_pass "13 Markdown 箇条書き" "$(printf "gh pr comment 1 --body '## 見出し\n\n- item1\n- item2\n\n%s'" "$footer")"
  # 14: heredoc 本体のフッター(`;` を含む長文)
  expect_pass "14 heredoc" "$(printf "gh pr comment 1 --body \"\$(cat <<'EOF'\n本文; セミコロン入り\n%s\nEOF\n)\"" "$footer")"
  # 15: コマンド置換で本文を渡した形(判定不能 → 通す)
  expect_pass "15 コマンド置換" 'gh pr comment 1 --body "$(cat body.md)"'
  # 16: 非対象コマンド
  expect_pass "16 非対象" "git status"
  expect_pass "16 非対象(gh 参照系)" "gh pr view 1 --json body"
  # 17: gh pr review のレビュー本体
  expect_deny "17 pr review" "gh pr review 1 --approve --body 'LGTM'"
  expect_pass "17 pr review+footer" "gh pr review 1 --approve --body 'LGTM $footer'"

  # 追加の境界: --body=<値> 形、issue create、両範囲にフッターがある複合
  expect_deny "18 --body= 形" "gh issue create --title t --body=短い本文"
  expect_pass "18 --body= 形+footer" "gh issue create --title t --body=\"本文 $footer\""
  expect_pass "19 複合で両方に footer" "gh pr create --title t --body '本文 $footer' && gh pr comment 1 --body 'x $footer'"

  # 20: コマンド位置にない「投稿コマンドの綴り」で発火しないこと(false deny の
  #     回帰)。この hook 自身のコミットメッセージ(本文に投稿コマンドの例を含む)
  #     で実際に踏んだ。docs や commit メッセージに綴りが出るのは日常なので、
  #     ここが落ちると gate は実用上使えない。
  expect_pass "20 commit msg 内の例文" \
    "$(printf 'git commit -q -F - <<%sMSG%s\nfeat: x\n\n  gh pr create --body "..." && gh pr comment 1 --body "y"\nMSG' "'" "'")"
  expect_pass "20 docs の grep" "grep -n 'gh pr comment 1 --body' docs/claude/attribution-guard.md"
  expect_pass "20 echo で綴りを出す" "echo 'gh issue comment 1 --body x'"

  # 21: heredoc 本体にフッターが無ければ deny。has_hd の検出は値トークンの
  #     内側にある `<<TAG` も拾う必要がある(独立トークンだけを見る実装では
  #     pass に倒れていた、実測)。
  expect_deny "21 heredoc 本体に footer 無し" \
    "$(printf "gh pr comment 1 --body \"\$(cat <<%sEOF%s\n本文だけ\nEOF\n)\"" "'" "'")"

  # 22-24: wrap-up inbox の統合フッター(D1)。出自フッターが生成元表示を兼ねる
  # 1 行形式を受理し、Claude Code を含まない出自フッター単独は引き続き deny、
  # 旧 2 行形式も回帰として通ることを確認する。
  local wrapup_footer='🤖 Filed from [Claude Code](https://claude.com/claude-code) wrap-up inbox'
  expect_pass "22 wrapup 統合フッター" "gh issue create --title t --body '本文 $wrapup_footer'"
  expect_deny "23 wrapup フッターだが Claude Code 無し" "gh issue create --title t --body '本文 🤖 Filed from wrap-up inbox'"
  expect_pass "24 旧 2 行形式(回帰)" "$(printf "gh issue create --title t --body '本文\n%s\n🤖 Filed from Claude Code wrap-up inbox'" "$footer")"

  # 25-33: gh api の外向き投稿経路(#195)。
  printf '{"body": "本文だけ"}\n' > "$tmp/api-without.json"
  printf '{"body": "本文 %s"}\n' "$footer" > "$tmp/api-with.json"

  # 25: issue コメント作成(-f body=)
  expect_deny "25 api issue comment -footer" "gh api repos/o/r/issues/1/comments -f body='確認しました'"
  expect_pass "25 api issue comment +footer" "gh api repos/o/r/issues/1/comments -f body=\"確認しました $footer\""

  # 26: pr 作成(pulls への POST)
  expect_deny "26 api pr create -footer" "gh api repos/o/r/pulls -f title=t -f body='本文'"
  expect_pass "26 api pr create +footer" "gh api repos/o/r/pulls -f title=t -f body=\"本文 $footer\""

  # 27: インラインレビューコメント(pulls/N/comments)は #194 の決定どおり
  #     対象外のまま。API_JUDGED_RE の alternation に無いパスなので、
  #     除外用の追加正規表現なしで自然に通る(除外の正本ケース)。
  expect_pass "27 api inline pulls/N/comments" "gh api repos/o/r/pulls/1/comments -f body='inline コメント'"
  expect_pass "27 api inline pulls/comments/N(編集)" "gh api -X PATCH repos/o/r/pulls/comments/9 -f body='inline 編集'"

  # 28: --input <file>(judged path + --input)
  expect_deny "28 api --input ファイル -footer" "gh api repos/o/r/issues --input $tmp/api-without.json"
  expect_pass "28 api --input ファイル +footer" "gh api repos/o/r/issues --input $tmp/api-with.json"
  expect_pass "28 api --input ファイル不在(判定不能)" "gh api repos/o/r/issues --input $tmp/nope.json"

  # 29: --input -(stdin、判定不能 → 通す)。heredoc 併用時は本体を検査する。
  expect_pass "29 api --input -(stdin)" "gh api repos/o/r/issues --input -"
  expect_deny "29 api --input - + heredoc footer 無し" \
    "$(printf "gh api repos/o/r/issues --input - <<%sEOF%s\n{\"body\": \"本文だけ\"}\nEOF" "'" "'")"

  # 30: PATCH 編集(issues/comments/N)
  expect_deny "30 api PATCH 編集 -footer" "gh api -X PATCH repos/o/r/issues/comments/9 -f body='編集'"
  expect_pass "30 api PATCH 編集 +footer" "gh api -X PATCH repos/o/r/issues/comments/9 -f body=\"編集 $footer\""

  # 31: 明示 GET(判定不能 → 通す)。本文フラグも無いので二重に安全。
  expect_pass "31 api GET" "gh api -X GET repos/o/r/issues --jq '.[].title'"

  # 32: 非対象パス(rulesets 等)は素通り —
  #     scripts/github-rulesets-apply が呼ぶ実際の形の回帰。
  expect_pass "32 api 非対象パス(rulesets)" "gh api -X POST repos/o/r/rulesets --input $tmp/api-without.json"

  # 33: コマンド位置外の綴り(echo の引数内)で発火しないこと
  expect_pass "33 api コマンド位置外" "echo 'gh api repos/o/r/issues -f body=x'"

  if [[ $fails -gt 0 ]]; then
    echo "selftest: ${fails} 件失敗" >&2
    exit 1
  fi
  echo "selftest: OK"
}

# config/codex/hooks/attribution-guard.sh・config/copilot/hooks/
# attribution-guard.sh がこのファイルを `source` して判定エンジンだけを
# 再利用する(#192)。source 時にこのディスパッチが暴発しないよう、直接
# 実行時のみに限定する(標準的な bash の「source か実行か」判定イディオム)。
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  case "${1-}" in
    --selftest) selftest ;;
    --check)
      if reason="$(decide "${2-}")"; then
        printf 'deny: %s\n' "$reason"
        exit 1
      fi
      echo "pass"
      ;;
    *) main ;;
  esac
fi
