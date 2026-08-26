# Claude Code のフック群 — plan-review ゲート・wrap-up inbox・plan-view を
# home-manager で配備する。
#
# 1) plan-review ゲート(PreToolUse / ExitPlanMode):
#    Codex CLI によるプランの自動レビュー。gate は acceptance-convergent であり、
#    critic は verdict を出さず、judge(jq)が決定論的に gate 適格性を判定する。
#    最終ラウンドは closer。詳細は docs/codex-plan-review.md。
#
# 2) wrap-up inbox(SessionStart + Stop):
#    スコープ外の気づきの「収集」と「起票」を分離する。SessionStart hook が
#    additionalContext で「気づきは state 領域の inbox(JSONL)に --add で追記せよ」
#    と注入し、Stop hook が inbox 非空かつ起票可能(gh あり・git repo・GitHub
#    remote あり)なら exit 2 + stderr 指示でフルコンテキストの本体 Claude に
#    gh issue create させる。それ以外は黙って exit 0(ADR-0005 の binary-existence
#    gating に倣う)。詳細は docs/wrapup-inbox.md。
#
# 3) plan-view(PreToolUse / ExitPlanMode + CLI):
#    プランを pandoc で HTML にして Chrome の専用窓(--app)に飛ばす。LLM は呼ばず、
#    Markdown を 1:1 で写すだけの表示専用の道具である。plan-review gate と同じ
#    matcher に別エントリとして並び、並列に走る(= review の結果を待たない)。承認
#    フローに干渉しないため、成否に関わらず stdout に何も出さず exit 0 する。
#    詳細は docs/plan-view.md。
#
# 4) issue-index(SessionStart, matcher: startup|resume|compact):
#    自分に関係する open Issue の索引(番号・タイトル・ラベル・起票者)だけを
#    additionalContext で注入する。本文は渡さない — 深掘りは Claude 自身に
#    `gh issue view` を叩かせる。データ源は `gh issue list` ではなく GitHub
#    Search API(`gh api search/issues`): --limit は取得上限であり総数ではない
#    ので、それで総数を数えると嘘になる。assignee:@me が 0 件なら repo 全体の
#    open にフォールバックし、他人起票の行にだけ起票者を明記する(タイトルは
#    untrusted なので制御文字除去 + 120 文字切り詰めもするが、これは防御ではなく
#    payload 制御に過ぎない)。詳細は docs/issue-index.md。
#
# Hybrid translation (ADR-0002): hook スクリプト・スキーマ・スラッシュコマンドは
# config/claude/ 配下に literal で置き、home.file で配備する。どの hook も必要な
# バイナリが無いホストでは黙って no-op するため全ホストへ無条件配備でよい。
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

  registerHooks = pkgs.writeShellScript "register-claude-hooks" ''
    set -eu
    settings="$1"
    jq=${pkgs.jq}/bin/jq

    if [ ! -f "$settings" ]; then
      mkdir -p "$(dirname "$settings")"
      printf '{}\n' > "$settings"
    fi

    # register <event> <matcher> <cmd> <timeout>
    #   matcher / timeout は空文字ならフィールド自体を出力しない。
    #   存在判定は command の一致だけで足りる(hook のパスがエントリを一意に定める)。
    register() {
      event="$1" matcher="$2" cmd="$3" timeout="$4"
      if "$jq" -e --arg event "$event" --arg cmd "$cmd" \
          '[.hooks[$event][]? | .hooks[]? | select(.command == $cmd)] | length > 0' \
          "$settings" >/dev/null; then
        return 0
      fi
      tmp="$(mktemp)"
      "$jq" --arg event "$event" --arg matcher "$matcher" \
            --arg cmd "$cmd" --arg timeout "$timeout" '
        .hooks[$event] = ((.hooks[$event] // []) + [
          (if $matcher == "" then {} else { matcher: $matcher } end)
          + { hooks: [
              { type: "command", command: $cmd }
              + (if $timeout == "" then {} else { timeout: ($timeout | tonumber) } end)
            ] }
        ])' "$settings" > "$tmp"
      mv "$tmp" "$settings"
    }

    register PreToolUse ExitPlanMode "$2" 300
    register Stop "" "$3" ""
    register SessionStart "" "$4" ""
    # plan-view は plan-review gate と同じ matcher に、別エントリとして並ぶ。
    # Claude Code は同一 matcher の hook を並列に走らせるので、review の結果を
    # 待たずに窓が開く（= 表示は gate から独立している）。timeout は短く: この
    # hook は pandoc とプロセス fork しかせず、ブラウザの終了は待たない。
    register PreToolUse ExitPlanMode "$5" 15
    # issue-index は startup/resume/compact でだけ発火する。clear は「文脈を捨てたい」
    # という利用者の意思表示なので外す。compact は逆に文脈を続けたい表示であり、
    # 要約で索引が落ちている可能性が高く再注入の価値が最も高い(autoCompactEnabled
    # は off なので発火は手動 /compact 時のみ)。fork は元セッションの文脈を
    # 引き継ぐので不要。
    register SessionStart "startup|resume|compact" "$6" 10
  '';
in
{
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

  home.activation.registerClaudeHooks = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${registerHooks} "$HOME/.claude/settings.json" \
      ${lib.escapeShellArg planReviewCmd} \
      ${lib.escapeShellArg wrapupStopCmd} \
      ${lib.escapeShellArg wrapupSessionStartCmd} \
      ${lib.escapeShellArg planViewCmd} \
      ${lib.escapeShellArg issueIndexCmd}
  '';
}
