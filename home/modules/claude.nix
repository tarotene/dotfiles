# Claude Code のフック群 — plan-review ゲート・wrap-up inbox・plan-view・
# sign-prewarm・pr-gate・issue-index を home-manager で配備する。
#
# 1) plan-review ゲート(PreToolUse / ExitPlanMode):
#    Codex CLI によるプランの自動レビュー。gate は acceptance-convergent であり、
#    critic は verdict を出さず、judge(jq)が決定論的に gate 適格性を判定する。
#    最終ラウンドは closer。詳細は docs/claude/codex-plan-review.md。
#
# 2) wrap-up inbox(SessionStart + Stop):
#    スコープ外の気づきの「収集」と「起票」を分離する。SessionStart hook が
#    additionalContext で「気づきは state 領域の inbox(JSONL)に --add で追記せよ」
#    と注入し、Stop hook が inbox 非空かつ起票可能(gh あり・git repo・GitHub
#    remote あり)なら exit 2 + stderr 指示でフルコンテキストの本体 Claude に
#    gh issue create させる。それ以外は黙って exit 0(ADR-0005 の binary-existence
#    gating に倣う)。詳細は docs/claude/wrapup-inbox.md。
#
# 3) plan-view(PreToolUse / ExitPlanMode + CLI):
#    プランを pandoc で HTML にして Chrome の専用窓(--app)に飛ばす。LLM は呼ばず、
#    Markdown を 1:1 で写すだけの表示専用の道具である。plan-review gate と同じ
#    matcher に別エントリとして並び、並列に走る(= review の結果を待たない)。承認
#    フローに干渉しないため、成否に関わらず stdout に何も出さず exit 0 する。
#    詳細は docs/claude/plan-view.md。
#
# 4) sign-prewarm(SessionStart, matcher: startup|resume):
#    git commit の署名パスフレーズ入力を、ログイン後最初に Claude を開いた安全な
#    瞬間に前倒しする。home/modules/gpg.nix が gpg-agent の cache TTL を実質無限に
#    したことで再入力は「ログインに 1 回」まで落ちるが、その 1 回を放置すると
#    Claude の Bash 呼び出し中に GUI pinentry が grab 付きで出現し、キー入力を
#    奪ったままコミットが固まる。判定は SessionStart 時の cwd に依存せず、常に
#    グローバルな git 設定だけを見る。additionalContext は出さない(副作用だけの
#    hook)。詳細は docs/claude/sign-prewarm.md、脅威モデルの変化は ADR-0003 Amendment 2。
#
# 5) pr-gate(SessionStart + Stop):
#    「CI 待ちのまま完了を宣言する」「push し忘れたまま完了する」の 2 事故を、Stop の
#    1 点だけで hard gate する(base 鮮度・未コミット変更は advisory)。判定対象は
#    ~/.claude/pr-gate-repos に列挙した nwo だけ(既定は本リポジトリのみ)で、
#    それ以外では完全沈黙する。中心不変条件は「揃っていない集合を緑と読まないこと」
#    — 期待される required check をサーバの ruleset から取り、`gh pr checks --watch`
#    の exit code ではなく取り直した --json を jq で判定する。詳細は docs/claude/pr-gate.md。
#
# 6) issue-index(SessionStart, matcher: startup|resume|compact):
#    自分に関係する open Issue の索引(番号・タイトル・ラベル・起票者)だけを
#    additionalContext で注入する。本文は渡さない — 深掘りは Claude 自身に
#    `gh issue view` を叩かせる。データ源は `gh issue list` ではなく GitHub
#    Search API(`gh api search/issues`): --limit は取得上限であり総数ではない
#    ので、それで総数を数えると嘘になる。assignee:@me が 0 件なら repo 全体の
#    open にフォールバックし、他人起票の行にだけ起票者を明記する(タイトルは
#    untrusted なので制御文字除去 + 120 文字切り詰めもするが、これは防御ではなく
#    payload 制御に過ぎない)。詳細は docs/claude/issue-index.md。
#
# 7) git-worktree-allow(PreToolUse, matcher: Bash, if: "Bash(git -C *)"):
#    herdr worktree を外から駆動する `git -C <worktree> <サブコマンド>` を検証して
#    プログラム的に許可する。`Bash(git -C * add *)` のような中間ワイルドカードの
#    permission rule は、`-C` の位置への任意オプション挿入(--exec-path 等 = 任意
#    コード実行)を素通しするため Claude Code が毎セッション警告し、しかも中間 `*`
#    は実際にはマッチしない。hook なら「実在する ~/.herdr/worktrees/ 配下のパス +
#    許可サブコマンド + 単一 git 呼び出し」を検証してから allow を返せる。
#    詳細は docs/claude/git-worktree-allow.md。
#
# 8) git-stash-guard(PreToolUse, matcher: Bash, if: "Bash(git *)"):
#    素の `git stash` / `git stash pop` / SHA 無しの apply・drop・裸の push・
#    clear を deny する。stash スタックはリポジトリ単位で herdr の worktree 間
#    (=セッション間)で共有されているため、素の stash は他セッションの WIP を
#    取り違えて pop/apply する事故につながる。git-worktree-allow(allow 側)とは
#    非対称: allow 側は if 不一致でも安全(単に許可を出さないだけ)だが、この
#    hook は deny 側なので if 不一致は検査されない素通り = 事故そのものになる。
#    かつ registerHooks は command 文字列の完全一致でしか存在判定しないため、
#    同一スクリプトを 2 つの if で二重登録することもできない。よって if は
#    "Bash(git *)" まで広げ、絞り込みは hook 内部の早期 exit(grep → jq)に移した。
#    詳細は docs/claude/git-stash-guard.md。
#
# 9) herdr-sidebar-metadata(5 イベント + statusLine):
#    各エージェントの permission mode・モデル・context%・コスト・effort を Herdr
#    サイドバーに常時表示する。mode は hook input からしか取れず、モデル等は
#    statusline JSON からしか取れないため 2 チャネル構成になっている。トークン色は
#    静的指定のみなので、mode の色分けは「モード毎に別トークン + 非アクティブは
#    null クリア」で表現する。詳細は docs/claude/herdr-sidebar-metadata.md。
#
# 10) Opus Plan Mode のモデル実体(settings.json の env + fallbackModel):
#    `model: "opusplan"` は「Plan 中は opus エイリアス、実行中は sonnet エイリアス」
#    という *エイリアスのペア* であり、各エイリアスがどの具体モデルに解決されるかは
#    別に宣言できる。ここで opus エイリアスだけを Fable 5 に差し替えることで
#    「Plan 中は Fable 5(1M)、実行中は Sonnet 5」を得る。model キー自体は
#    /model で日常的に切り替える対象なので home-manager は触らない。
#    詳細は docs/claude/opusplan-model-aliases.md。
#
# 11) 個人スキル(diagramming, skill-gardening):
#    hook ではなく ~/.claude/skills/ 配下に置く判断知識。diagramming は作図時に
#    「内容の型に合うジャンル・技術を選ぶ」処方と、手書き SVG に落ちた場合の
#    技術非依存の不変条件(矢印端点をボックス定義から導出する・完成の定義に視認を
#    含める等)を持つ。skill-gardening はこの知見をどう dotfiles(公開)に固定化
#    するかのメタスキルで、公開リポジトリ向けサニタイズ規則の正本を持つ。
#    hook のような settings.json 登録は不要(スキルは ~/.claude/skills/ を
#    スキャンするだけで発動する)なので home.file だけで足りる。詳細は
#    docs/claude/diagramming.md、docs/claude/skill-gardening.md。
#
# 12) claude-usage(herdr の tab_bar_right command、hook ではない):
#    `/usage` を打たずに Rate Limit(5h セッション窓)と Fable の週間上限を Herdr
#    のタブバー右端に常時表示する。データ源は statusline / hooks の入力 JSON には
#    無い唯一の経路(`/usage` が内部で使う非公開 API)であり、settings.json への
#    hook 登録はしない — herdr が interval 実行して標準出力の最終行を描画する。
#    壊れたときの症状は「タブバーからこのセグメントが消えるだけ」に収束させる。
#    詳細は docs/claude/claude-usage-tabbar.md。
#
# Hybrid translation (ADR-0002): hook スクリプト・スキーマ・スラッシュコマンド・
# スキルは config/claude/ 配下に literal で置き、home.file で配備する。どの hook も
# 必要なバイナリが無いホストでは黙って no-op するため全ホストへ無条件配備でよい。
#
# ~/.claude/settings.json は Claude-Code-owned(CLI が実行時に書き換える)なので、
# hook の登録だけは store symlink にできない — desktop.nix の fcitx5 プロファイルと
# 同じ制約。代わりに activation 時に冪等マージする: 同一 command を持つエントリが
# 該当イベント配下に無いときだけ注入し、それ以外は一切触らない。
{
  config,
  lib,
  pkgs,
  ...
}:
let
  repoConfig = ../../config;
  hooksDir = "${config.home.homeDirectory}/.claude/hooks";
  planReviewCmd = "bash '${hooksDir}/codex-plan-review.sh'";
  wrapupStopCmd = "bash '${hooksDir}/wrapup-stop-gate.sh'";
  wrapupSessionStartCmd = "bash '${hooksDir}/wrapup-session-start.sh'";
  planViewCmd = "bash '${hooksDir}/plan-view.sh'";
  issueIndexCmd = "bash '${hooksDir}/issue-index.sh'";
  signPrewarmCmd = "bash '${hooksDir}/sign-prewarm.sh'";
  prGateSessionStartCmd = "bash '${hooksDir}/pr-gate.sh' session-start";
  prGateStopCmd = "bash '${hooksDir}/pr-gate.sh' stop";
  gitWorktreeAllowCmd = "bash '${hooksDir}/git-worktree-allow.sh'";
  gitStashGuardCmd = "bash '${hooksDir}/git-stash-guard.sh'";
  herdrMetadataCmd = "bash '${hooksDir}/herdr-claude-metadata.sh'";
  statusLineCmd = "bash '${hooksDir}/claude-statusline.sh'";

  # かつて登録したが撤回した hook。activation が全ホストの settings.json から
  # (event, command) の組で完全一致削除する。撤回が宣言的にできる前は、hook
  # スクリプトを home.file から外すと settings.json のエントリだけが残り、消えた
  # パスを指したまま毎イベントで ENOENT を吐き続けた(#44 の実体 — herdr のリネーム
  # ではなく自リポジトリ由来。herdr が settings.json に書くのは
  # integration::claude_settings::rewrite による .hooks.SessionStart の
  # herdr-agent-state.sh だけで、statusLine にも他イベントにも触れない)。
  retiredHookEntries = [
    # 旧イテレーションは PostToolUse に herdr-claude-metadata.sh を登録していた。
    # 現行は PreToolUse で同じ情報を取る(遅延が小さい)ので、こちらは外す。
    {
      event = "PostToolUse";
      command = herdrMetadataCmd;
    }
  ];

  # かつて配って撤回した statusLine。syncStatusLine が .statusLine.command との
  # 完全一致でキーを削除する対象。今回は herdr-sidebar-metadata を新規導入するので
  # 空 — 将来この機能自体を取り下げるときに statusLineCmd をここへ移す。
  retiredStatusLineCommands = [ ];

  # Opus Plan Mode のエイリアス実体。
  #
  # `model: "opusplan"` は Plan 中に opus エイリアス、実行中に sonnet エイリアスを
  # 解決する(claude 2.1.252 の実測: Plan 側が Hl() を、実行側が dp() を呼ぶ)。
  # その Hl() は ANTHROPIC_DEFAULT_OPUS_MODEL を最優先で返し、未設定ならカタログ
  # 既定の Opus に落ちる — dp() も ANTHROPIC_DEFAULT_SONNET_MODEL で同型。つまり
  # 「Plan 中だけ別のモデルを使う」はこの env 1 本で表現できる。
  #
  # ここでは opus エイリアスだけを Fable 5 に差し替える。sonnet エイリアスは
  # カタログ既定(= Sonnet 5)がそのまま望みの値なので宣言しない — 宣言すると
  # モデル世代が上がったときに古い ID に固定してしまう。
  #
  # 副作用: opus エイリアスは *全体* が Fable 5 になる。`/model opus` を選んでも
  # 実体は Fable 5 なので、素の Opus を使いたいときはエイリアスではなくモデル名を
  # 直接指定する(`/model claude-opus-5`)。
  claudeModelEnv = {
    ANTHROPIC_DEFAULT_OPUS_MODEL = "claude-fable-5";
  };

  # primary が overload / 利用不可のときに順に試すモデル(settings スキーマの
  # fallbackModel)。Plan 中の Fable が詰まったら Opus 5 に落ちる。エイリアスでは
  # なく具体 ID を書く — "opus" と書くと上の env で Fable 5 に解決され、
  # fallback が自分自身を指してしまう。
  claudeFallbackModels = [ "claude-opus-5" ];

  # かつて宣言したが撤回した env キー。activation が settings.json の .env から
  # 削除する。retiredPermissionRules と同じく「かつて自分が書いたキー」だけを
  # 消す — 無条件 del にしないのは、他の経路で入れた env を奪わないため。
  retiredModelEnvKeys = [ ];

  registerHooks = pkgs.writeShellScript "register-claude-hooks" ''
    set -eu
    settings="$1"
    shift
    jq=${pkgs.jq}/bin/jq

    if [ ! -f "$settings" ]; then
      mkdir -p "$(dirname "$settings")"
      printf '{}\n' > "$settings"
    fi

    # tmp は settings.json と同じディレクトリに作る。mktemp の既定($TMPDIR か
    # /tmp)は $HOME と別 fs になり得て、その場合 mv は rename(2) ではなく
    # copy+unlink に落ちる(非原子的で、途中で落ちれば settings.json が壊れる)。
    # mode も mktemp の 0600 決め打ちではなく元ファイルに合わせる。
    write_back() {
      chmod --reference="$settings" "$1" 2>/dev/null || chmod 600 "$1"
      mv "$1" "$settings"
    }

    # retire <event> <cmd>: そのイベント配下から command 完全一致のハンドラだけを
    # 外す。空になった matcher グループとイベントキーも畳む。該当ゼロなら読むだけで
    # 書かない(定常状態では settings.json に触らない)。他ツールのエントリ(herdr の
    # herdr-agent-state.sh、ローカルの public-publish-guard.sh / pr-body-guard.sh)は
    # command が一致しない限り触れない。
    retire() {
      event="$1" cmd="$2"
      if ! "$jq" -e --arg event "$event" --arg cmd "$cmd" \
          '[.hooks[$event][]? | .hooks[]? | select(.command == $cmd)] | length > 0' \
          "$settings" >/dev/null; then
        return 0
      fi
      tmp="$(mktemp "$settings.hm.XXXXXX")"
      "$jq" --arg event "$event" --arg cmd "$cmd" '
        .hooks[$event] = ( (.hooks[$event] // [])
          | map( if any(.hooks[]?; .command == $cmd)
                 then (.hooks |= map(select(.command != $cmd)))
                 else . end )
          | map(select((.hooks? == null) or ((.hooks | length) > 0))) )
        | (if ((.hooks[$event] // []) | length) == 0 then del(.hooks[$event]) else . end)
      ' "$settings" > "$tmp"
      write_back "$tmp"
    }

    # register <event> <matcher> <cmd> <timeout> [<if>]
    #   matcher / timeout / if は空文字(または省略)ならフィールド自体を出力しない。
    #   if はハンドラレベルの絞り込み(permission rule 構文、例: "Bash(git -C *)")で、
    #   マッチしない呼び出しでは hook プロセス自体が spawn されない。
    #   存在判定は command の一致だけで足りる(hook のパスがエントリを一意に定める)。
    register() {
      event="$1" matcher="$2" cmd="$3" timeout="$4" if_rule="''${5-}"
      if "$jq" -e --arg event "$event" --arg cmd "$cmd" \
          '[.hooks[$event][]? | .hooks[]? | select(.command == $cmd)] | length > 0' \
          "$settings" >/dev/null; then
        return 0
      fi
      tmp="$(mktemp "$settings.hm.XXXXXX")"
      "$jq" --arg event "$event" --arg matcher "$matcher" \
            --arg cmd "$cmd" --arg timeout "$timeout" --arg ifrule "$if_rule" '
        .hooks[$event] = ((.hooks[$event] // []) + [
          (if $matcher == "" then {} else { matcher: $matcher } end)
          + { hooks: [
              { type: "command", command: $cmd }
              + (if $ifrule == "" then {} else { "if": $ifrule } end)
              + (if $timeout == "" then {} else { timeout: ($timeout | tonumber) } end)
            ] }
        ])' "$settings" > "$tmp"
      write_back "$tmp"
    }

    if [ "''${1-}" = "--retire" ]; then
      shift
      while [ "$#" -gt 0 ] && [ "$1" != "--register" ]; do
        retire "$1" "$2"
        shift 2
      done
    fi
    if [ "''${1-}" != "--register" ]; then
      echo "register-claude-hooks: --register が必要" >&2
      exit 1
    fi
    shift

    plan_review="$1";           shift
    wrapup_stop="$1";           shift
    wrapup_session_start="$1";  shift
    plan_view="$1";             shift
    issue_index="$1";           shift
    sign_prewarm="$1";          shift
    pr_gate_session_start="$1"; shift
    pr_gate_stop="$1";          shift
    git_worktree_allow="$1";    shift
    git_stash_guard="$1";       shift
    herdr_metadata="$1";        shift

    register PreToolUse ExitPlanMode "$plan_review" 300
    register Stop "" "$wrapup_stop" ""
    register SessionStart "" "$wrapup_session_start" ""
    # plan-view は plan-review gate と同じ matcher に、別エントリとして並ぶ。
    # Claude Code は同一 matcher の hook を並列に走らせるので、review の結果を
    # 待たずに窓が開く（= 表示は gate から独立している）。timeout は短く: この
    # hook は pandoc とプロセス fork しかせず、ブラウザの終了は待たない。
    register PreToolUse ExitPlanMode "$plan_view" 15
    # issue-index は startup/resume/compact でだけ発火する。clear は「文脈を捨てたい」
    # という利用者の意思表示なので外す。compact は逆に文脈を続けたい表示であり、
    # 要約で索引が落ちている可能性が高く再注入の価値が最も高い(autoCompactEnabled
    # は off なので発火は手動 /compact 時のみ)。fork は元セッションの文脈を
    # 引き継ぐので不要。
    register SessionStart "startup|resume|compact" "$issue_index" 10
    # sign-prewarm は compact を含めない: 同一プロセス内の事象なので agent の
    # キャッシュはすでに温まっているか、そもそもまだ温まっていないかのどちらか
    # であり、compact での再発火は温度判定で黙って no-op になるだけで発火の価値が
    # ない。resume は別ログインからの再開があり得るので含める。
    register SessionStart "startup|resume" "$sign_prewarm" 120
    # pr-gate: SessionStart は状態の一覧取得のみ(短時間)。Stop は CI の
    # --watch --fail-fast を timeout 300s 付きで自前で回すので、hook の timeout は
    # それより長く確保する(既知の罠: registerHooks は command 一致だけで存在判定
    # するので、matcher/timeout を後から変えても既存エントリは更新されない —
    # docs/claude/issue-index.md。だから timeout は最初から余裕を持たせておく)。
    register SessionStart "" "$pr_gate_session_start" 10
    register Stop "" "$pr_gate_stop" 600
    # git-worktree-allow: if で "Bash(git -C *)" に絞る — それ以外の Bash 呼び出しでは
    # hook プロセス自体が起動しない。判定はすべて hook 側(パス実在・許可サブコマンド・
    # 単一 git 呼び出し)で行い、非該当は無出力 exit 0 で通常の permission フローに
    # フォールスルーする。
    register PreToolUse Bash "$git_worktree_allow" 10 "Bash(git -C *)"
    # git-stash-guard: if を worktree-allow よりずっと広い "Bash(git *)" にする
    # 理由は docs/claude/git-stash-guard.md(deny 側は if 不一致 = 素通りが
    # 事故そのものになるため、絞り込みは hook 内部の早期 exit に移した)。
    register PreToolUse Bash "$git_stash_guard" 10 "Bash(git *)"
    # herdr-claude-metadata は permission mode の遷移を Herdr サイドバーに流す。
    # 同一 command を 5 イベントに登録する(スクリプト側が hook_event_name で分岐):
    # SessionStart=初期値+残留上書き / UserPromptSubmit=アイドル中の Shift+Tab を
    # 1 プロンプト 1 回で拾う / PreToolUse=plan 承認→acceptEdits を最小遅延で拾う
    # (スクリプト側の前回値キャッシュで、モードが同じなら jq 1 回で即抜ける)/
    # Stop=ttl リフレッシュ / SessionEnd=トークン全クリア。PostToolUse は
    # PreToolUse と同情報で遅延だけ悪いので登録しない(旧イテレーションは
    # PostToolUse に登録していたので retiredHookEntries で外す)。Herdr 外では
    # 即 no-op。
    register SessionStart "" "$herdr_metadata" 10
    register UserPromptSubmit "" "$herdr_metadata" 10
    register PreToolUse "" "$herdr_metadata" 10
    register Stop "" "$herdr_metadata" 10
    register SessionEnd "" "$herdr_metadata" 10
  '';

  # settings.json の statusLine を宣言に合わせる。
  # 使い方: sync-claude-statusline <settings> <desired-or-empty> [<retired>…]
  #   retired を先に処理し、.statusLine.command が retired のいずれかに完全一致
  #   したときだけキーを削除する。無条件 del にしないのは、/statusline で本人が
  #   設定した値を「宣言なし」の switch で奪わないため — retiredPermissionRules が
  #   「かつて自分が配った文字列」だけを消すのと同型。desired が非空なら最後に
  #   set-if-different するので、retire→set の順で set が勝つ。
  syncStatusLine = pkgs.writeShellScript "sync-claude-statusline" ''
    set -eu
    settings="$1"
    desired="$2"
    shift 2
    jq=${pkgs.jq}/bin/jq

    if [ ! -f "$settings" ]; then
      mkdir -p "$(dirname "$settings")"
      printf '{}\n' > "$settings"
    fi

    write_back() {
      chmod --reference="$settings" "$1" 2>/dev/null || chmod 600 "$1"
      mv "$1" "$settings"
    }

    current="$("$jq" -r '.statusLine.command // ""' "$settings")"

    for retired in "$@"; do
      [ "$current" = "$retired" ] || continue
      tmp="$(mktemp "$settings.hm.XXXXXX")"
      # null 代入ではなくキーの削除。「この機能を入れる前の形に戻す」が撤回の
      # 意味であり、これで /statusline による後からの設定も素直に効く。
      "$jq" 'del(.statusLine)' "$settings" > "$tmp"
      write_back "$tmp"
      current=""
      break
    done

    if [ -n "$desired" ] && [ "$current" != "$desired" ]; then
      tmp="$(mktemp "$settings.hm.XXXXXX")"
      "$jq" --arg cmd "$desired" \
        '.statusLine = { type: "command", command: $cmd }' "$settings" > "$tmp"
      write_back "$tmp"
    fi
  '';

  # settings.json の .env と .fallbackModel を宣言に合わせる。
  # 使い方: sync-claude-model-config <settings> <desired-json> <retired-env-keys-json>
  #   desired = { env: {…}, fallbackModel: […] }
  #   retire を先に処理してから宣言値を書くので、同じキーを両方に置いたら宣言が勝つ。
  #   .env の他のキー・.model・他のトップレベルキーには一切触らない — .model は
  #   /model で本人が切り替える対象で、宣言的に固定すると UI の選択を毎 switch で
  #   奪ってしまう。
  syncModelConfig = pkgs.writeShellScript "sync-claude-model-config" ''
    set -eu
    settings="$1"
    desired="$2"
    retired="$3"
    jq=${pkgs.jq}/bin/jq

    if [ ! -f "$settings" ]; then
      mkdir -p "$(dirname "$settings")"
      printf '{}\n' > "$settings"
    fi

    write_back() {
      chmod --reference="$settings" "$1" 2>/dev/null || chmod 600 "$1"
      mv "$1" "$settings"
    }

    tmp="$(mktemp "$settings.hm.XXXXXX")"
    "$jq" --argjson desired "$desired" --argjson retired "$retired" '
      # 撤回: 宣言から外したキーだけを .env から外す(.env が無ければ何もしない)。
      reduce $retired[] as $k (
        .;
        if (.env | type) == "object" then del(.env[$k]) else . end
      )
      # 宣言: .env の該当キーだけを宣言値にする。
      | reduce (($desired.env // {}) | to_entries[]) as $e (.; .env[$e.key] = $e.value)
      # 撤回で空になった .env はキーごと畳む(「入れる前の形」に戻す)。
      | if (.env | type) == "object" and (.env | length) == 0 then del(.env) else . end
      | if (($desired.fallbackModel // []) | length) > 0
        then .fallbackModel = $desired.fallbackModel
        else . end
    ' "$settings" > "$tmp"

    # 定常状態では settings.json に触らない(mtime も動かさない)。
    if "$jq" -e -s '.[0] == .[1]' "$settings" "$tmp" >/dev/null; then
      rm -f "$tmp"
    else
      write_back "$tmp"
    fi
  '';

  # settings.json の permissions.allow を要素単位で冪等に同期する。
  # 使い方: register-claude-permissions <settings> --retire <r>… --allow <a>…
  #   --retire 以降のルールは allow から削除(無ければ何もしない)、--allow 以降は
  #   追加(既に同一文字列があれば何もしない)。ルール文字列を後から書き換えるときは
  #   旧文字列を retiredPermissionRules に移す — これで全ホストが次回の switch で旧
  #   ルールを掃除する(かつては「旧ルールが残り続ける」が既知の制約だった —
  #   docs/claude/claude-permissions.md)。permissions.defaultMode や allow 以外の
  #   キーには一切触らない。
  registerPermissions = pkgs.writeShellScript "register-claude-permissions" ''
    set -eu
    settings="$1"
    shift
    jq=${pkgs.jq}/bin/jq

    if [ ! -f "$settings" ]; then
      mkdir -p "$(dirname "$settings")"
      printf '{}\n' > "$settings"
    fi

    mode=""
    for arg in "$@"; do
      case "$arg" in
        --retire|--allow) mode="$arg"; continue ;;
      esac
      tmp="$(mktemp)"
      case "$mode" in
        --retire)
          "$jq" --arg r "$arg" '
            .permissions.allow = ((.permissions.allow // []) | map(select(. != $r)))
          ' "$settings" > "$tmp"
          ;;
        --allow)
          "$jq" --arg r "$arg" '
            .permissions.allow = ((.permissions.allow // []) | if index($r) then . else . + [$r] end)
          ' "$settings" > "$tmp"
          ;;
        *)
          echo "register-claude-permissions: --retire/--allow より前にルールが来た: $arg" >&2
          exit 1
          ;;
      esac
      mv "$tmp" "$settings"
    done
  '';

  # Claude Code の permission rule 構文は `Tool(specifier)`(裸のコマンド文字列では
  # 認識されない)。Add / Commit / Create PR で毎回止まる直接原因はこの 4 件。
  # 破壊的操作は増やさない — 読み取り・検査系のみ追加する。gh の書き込み系
  # (pr edit / issue create / issue edit)は aocs-draft スキルが明示的な人の確認を
  # 要求する操作なので入れない。
  permissionRules = [
    "Bash(git add *)"
    "Bash(git commit *)"
    "Bash(git push *)"
    "Bash(gh pr create *)"

    "Bash(git status *)"
    "Bash(git diff *)"
    "Bash(git log *)"
    "Bash(git show *)"
    "Bash(git grep *)"
    "Bash(git rev-parse *)"
    "Bash(git branch *)"
    "Bash(git fetch *)"
    "Bash(git ls-remote *)"
    "Bash(git worktree list *)"
    "Bash(git switch *)"
    "Bash(git checkout -b *)"

    "Bash(uv run pytest *)"
    "Bash(uv run ruff *)"
    "Bash(uv run mypy *)"
    "Bash(uv run pre-commit run *)"
    "Bash(uv run docs-check *)"
    "Bash(./scripts/check-*.sh *)"
    "Bash(./scripts/list-branch-inventory.sh *)"
    "Bash(./scripts/sweep-removed-vendor-symbols.sh *)"

    "Bash(gh pr view *)"
    "Bash(gh pr list *)"
    "Bash(gh pr diff *)"
    "Bash(gh pr checks *)"
    "Bash(gh issue view *)"
    "Bash(gh issue list *)"

    "Bash(nix fmt)"
    "Bash(nix flake check)"
    "Bash(nix build *)"
    "Bash(home-manager generations)"

    "Bash(npm ci)"
    "Bash(npm run *)"
    "Bash(npm test *)"
  ];

  # かつて配ったが撤回したルール。activation が全ホストの settings.json から削除する。
  # `Bash(git -C * add *)` 等の中間ワイルドカードは、`-C` の位置への任意オプション
  # 挿入(--exec-path 等)を素通しするとして Claude Code が毎セッション警告し、
  # しかも中間 `*` は実際にはマッチしない。代替は git-worktree-allow hook(検証つき
  # のプログラム的許可 — docs/claude/git-worktree-allow.md)。
  retiredPermissionRules = [
    "Bash(git -C * add *)"
    "Bash(git -C * commit *)"
    "Bash(git -C * status *)"
    "Bash(git -C * diff *)"
  ];
in
{
  # plan-review gate の deny 対象 severity とラウンド上限をこの環境向けに再校正する。
  # 既定(BLOCKER,MAJOR / 3 ラウンド)は実測 deny 率 69%、3 ラウンド到達が中位という
  # 結果で、review の価値より摩擦が勝っていた。MAJOR は backlog へ落として報告のみに
  # し、ラウンドも 2 に絞る。gate 本体(codex-plan-review.sh)は触らない — closer
  # ラウンドが judge() に空文字を渡して「gate 適格 severity なし」を表現する不変条件
  # (`${3-$GATE_SEVERITIES}` のコロンなしデフォルト)に影響しないよう、値は env 経由
  # でのみ渡す。sessionVariables は次回ログインから効く。詳細は
  # docs/claude/codex-plan-review.md の環境変数節。
  home.sessionVariables = {
    CODEX_PLAN_REVIEW_GATE_SEVERITIES = "BLOCKER";
    MAX_PLAN_REVIEWS = "2";
  };

  home.file.".claude/hooks/codex-plan-review.sh" = {
    source = repoConfig + "/claude/hooks/codex-plan-review.sh";
    executable = true;
  };

  # critic の強制出力スキーマ(codex exec --output-schema)。hook が自身の
  # ディレクトリ相対で解決するので、2 ファイルは ~/.claude/hooks/ に並べて置く。
  home.file.".claude/hooks/codex-plan-review.schema.json".source =
    repoConfig + "/claude/hooks/codex-plan-review.schema.json";

  # wrap-up inbox の 2 hook。session-start は stop-gate と同じパス計算を使い、
  # 同じディレクトリに並んでいることを前提に stop-gate のパスを指示文に埋める。
  home.file.".claude/hooks/wrapup-stop-gate.sh" = {
    source = repoConfig + "/claude/hooks/wrapup-stop-gate.sh";
    executable = true;
  };
  home.file.".claude/hooks/wrapup-session-start.sh" = {
    source = repoConfig + "/claude/hooks/wrapup-session-start.sh";
    executable = true;
  };

  # plan-view: プランを HTML にして Chrome の専用窓に飛ばす hook + CLI。
  # スクリプトは CSS を自身のディレクトリ相対で解決するので、schema と同じく
  # 2 ファイルを ~/.claude/hooks/ に並べて置く（リポジトリ上の隣接関係も同じ)。
  home.file.".claude/hooks/plan-view.sh" = {
    source = repoConfig + "/claude/hooks/plan-view.sh";
    executable = true;
  };
  home.file.".claude/hooks/plan-view.css".source = repoConfig + "/claude/hooks/plan-view.css";

  # issue-index: 自分に関係する open Issue の索引だけを SessionStart で注入する。
  home.file.".claude/hooks/issue-index.sh" = {
    source = repoConfig + "/claude/hooks/issue-index.sh";
    executable = true;
  };

  # sign-prewarm: git commit の署名パスフレーズをログイン直後に温める。
  home.file.".claude/hooks/sign-prewarm.sh" = {
    source = repoConfig + "/claude/hooks/sign-prewarm.sh";
    executable = true;
  };

  # herdr-sidebar-metadata: permission mode(hook)とモデル・メトリクス(statusline)
  # を Herdr サイドバーのカスタムトークンに流す 2 チャネル構成。表示側の行定義は
  # config/herdr/config.toml(home/modules/herdr.nix が配備)。herdr が自動
  # インストールする統合 hook(herdr-agent-state.sh、herdr 管理)の隣に並ぶが、
  # 互いに自分のエントリしか触らないので衝突しない。
  home.file.".claude/hooks/herdr-claude-metadata.sh" = {
    source = repoConfig + "/claude/hooks/herdr-claude-metadata.sh";
    executable = true;
  };
  home.file.".claude/hooks/claude-statusline.sh" = {
    source = repoConfig + "/claude/hooks/claude-statusline.sh";
    executable = true;
  };

  # claude-usage: herdr の tab_bar_right command が interval 実行する(Claude Code
  # hook ではない — settings.json には登録しない)。呼び出し側は
  # config/herdr/config.toml。詳細は docs/claude/claude-usage-tabbar.md。
  home.file.".claude/hooks/claude-usage.sh" = {
    source = repoConfig + "/claude/hooks/claude-usage.sh";
    executable = true;
  };

  # pr-gate: PR completion barrier。判定対象は allowlist に列挙した nwo だけ
  # (既定は本リポジトリのみ)なので、他リポジトリでは完全沈黙する。
  home.file.".claude/hooks/pr-gate.sh" = {
    source = repoConfig + "/claude/hooks/pr-gate.sh";
    executable = true;
  };
  # git-worktree-allow: herdr worktree への `git -C` を検証つきで許可する PreToolUse hook。
  home.file.".claude/hooks/git-worktree-allow.sh" = {
    source = repoConfig + "/claude/hooks/git-worktree-allow.sh";
    executable = true;
  };
  # git-stash-guard: 素の `git stash` を deny する PreToolUse hook。
  home.file.".claude/hooks/git-stash-guard.sh" = {
    source = repoConfig + "/claude/hooks/git-stash-guard.sh";
    executable = true;
  };

  home.file.".claude/pr-gate-repos".text = ''
    # pr-gate.sh が Stop / SessionStart で判定する対象リポジトリ(owner/repo, 1行1つ)。
    # ここに無い repo では完全沈黙する。# 始まりの行と空行は無視。
    tarotene/dotfiles
  '';

  # Codex / Devin など hook を持たないエージェントや素のシェルから使う入口。
  # 本体を 2 箇所に置くと ~/.local/bin 側から CSS に届かないので、exec で寄せる。
  home.file.".local/bin/plan-view" = {
    text = ''
      #!/usr/bin/env bash
      exec bash "$HOME/.claude/hooks/plan-view.sh" "$@"
    '';
    executable = true;
  };

  # コマンドファイルは /home/tarotene をハードコードしている — どの identity も
  # home.username = "tarotene" を固定している間は問題ない(identities/*.nix)。
  # username を上書きするホストが現れたら見直すこと。
  home.file.".claude/commands/codex-plan-review.md".source =
    repoConfig + "/claude/commands/codex-plan-review.md";
  home.file.".claude/commands/plan-view.md".source = repoConfig + "/claude/commands/plan-view.md";

  # diagramming: 作図するときの処方(ジャンル選択)と原則(接続不良防止・視認必須)。
  # cases.md は追記型の失敗事例集で、追記時のサニタイズ規則は skill-gardening 側を見る。
  home.file.".claude/skills/diagramming/SKILL.md".source =
    repoConfig + "/claude/skills/diagramming/SKILL.md";
  home.file.".claude/skills/diagramming/cases.md".source =
    repoConfig + "/claude/skills/diagramming/cases.md";
  # skill-gardening: 知見をこの公開リポジトリにスキル化するときのメタスキル
  # (器の判断・配線チェックリスト・公開リポジトリ向けサニタイズ規則の正本)。
  home.file.".claude/skills/skill-gardening/SKILL.md".source =
    repoConfig + "/claude/skills/skill-gardening/SKILL.md";

  # --retire は retiredHookEntries が空でも末尾に `\` が残らないよう
  # concatMapStrings(区切り文字列を要素ごとに前置)で組む — concatMapStringsSep
  # だと空リストで区切りだけが浮く。
  home.activation.registerClaudeHooks = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${registerHooks} "$HOME/.claude/settings.json" \
      --retire${
        lib.concatMapStrings (
          e: " \\\n      " + lib.escapeShellArg e.event + " " + lib.escapeShellArg e.command
        ) retiredHookEntries
      } \
      --register \
      ${lib.escapeShellArg planReviewCmd} \
      ${lib.escapeShellArg wrapupStopCmd} \
      ${lib.escapeShellArg wrapupSessionStartCmd} \
      ${lib.escapeShellArg planViewCmd} \
      ${lib.escapeShellArg issueIndexCmd} \
      ${lib.escapeShellArg signPrewarmCmd} \
      ${lib.escapeShellArg prGateSessionStartCmd} \
      ${lib.escapeShellArg prGateStopCmd} \
      ${lib.escapeShellArg gitWorktreeAllowCmd} \
      ${lib.escapeShellArg gitStashGuardCmd} \
      ${lib.escapeShellArg herdrMetadataCmd}
  '';

  home.activation.registerClaudeStatusLine = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${syncStatusLine} "$HOME/.claude/settings.json" \
      ${lib.escapeShellArg statusLineCmd}${
        lib.concatMapStrings (c: " \\\n      " + lib.escapeShellArg c) retiredStatusLineCommands
      }
  '';

  # settings.json の .env / .fallbackModel を宣言に合わせる。hooks・statusLine・
  # permissions と同じ DAG 位置で、独立した activation として走らせる(jq マージの
  # 責務を混ぜない)。
  home.activation.registerClaudeModelConfig = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${syncModelConfig} "$HOME/.claude/settings.json" \
      ${
        lib.escapeShellArg (
          builtins.toJSON {
            env = claudeModelEnv;
            fallbackModel = claudeFallbackModels;
          }
        )
      } \
      ${lib.escapeShellArg (builtins.toJSON retiredModelEnvKeys)}
  '';

  # settings.json の permissions.allow を冪等に拡充する。registerClaudeHooks と同じ
  # DAG 位置(writeBoundary の後)で、独立した activation script として走らせる —
  # 片方が既存の hooks 登録ロジックを壊さないようにするため、jq マージの責務を
  # 混ぜない。
  home.activation.registerClaudePermissions = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${registerPermissions} "$HOME/.claude/settings.json" \
      --retire \
      ${lib.concatMapStringsSep " \\\n      " lib.escapeShellArg retiredPermissionRules} \
      --allow \
      ${lib.concatMapStringsSep " \\\n      " lib.escapeShellArg permissionRules}
  '';
}
