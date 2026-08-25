# Claude Code のフック群 — plan-review ゲートと wrap-up inbox を home-manager で配備する。
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

  # コマンドファイルは /home/tarotene をハードコードしている — どの identity も
  # home.username = "tarotene" を固定している間は問題ない(identities/*.nix)。
  # username を上書きするホストが現れたら見直すこと。
  home.file.".claude/commands/codex-plan-review.md".source =
    repoConfig + "/claude/commands/codex-plan-review.md";

  home.activation.registerClaudeHooks = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${registerHooks} "$HOME/.claude/settings.json" \
      ${lib.escapeShellArg planReviewCmd} \
      ${lib.escapeShellArg wrapupStopCmd} \
      ${lib.escapeShellArg wrapupSessionStartCmd}
  '';
}
