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
  signPrewarmCmd = "bash '${hooksDir}/sign-prewarm.sh'";
  prGateSessionStartCmd = "bash '${hooksDir}/pr-gate.sh' session-start";
  prGateStopCmd = "bash '${hooksDir}/pr-gate.sh' stop";

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
    # sign-prewarm は compact を含めない: 同一プロセス内の事象なので agent の
    # キャッシュはすでに温まっているか、そもそもまだ温まっていないかのどちらか
    # であり、compact での再発火は温度判定で黙って no-op になるだけで発火の価値が
    # ない。resume は別ログインからの再開があり得るので含める。
    register SessionStart "startup|resume" "$7" 120
    # pr-gate: SessionStart は状態の一覧取得のみ(短時間)。Stop は CI の
    # --watch --fail-fast を timeout 300s 付きで自前で回すので、hook の timeout は
    # それより長く確保する(既知の罠: registerHooks は command 一致だけで存在判定
    # するので、matcher/timeout を後から変えても既存エントリは更新されない —
    # docs/claude/issue-index.md。だから timeout は最初から余裕を持たせておく)。
    register SessionStart "" "$8" 10
    register Stop "" "$9" 600
  '';

  # settings.json の permissions.allow に、要素単位で冪等に追加する。registerHooks と
  # 同じ制約を負う: 既に同一文字列があれば何もしない。ルール文字列自体を後から書き
  # 換えても、旧ルールは消えない(registerHooks の「command 一致だけの存在判定」と
  # 同型の罠 — docs/claude/sign-prewarm.md の既知の制約を参照)。permissions.defaultMode や
  # allow 以外のキーには一切触らない。
  registerPermissions = pkgs.writeShellScript "register-claude-permissions" ''
    set -eu
    settings="$1"
    shift
    jq=${pkgs.jq}/bin/jq

    if [ ! -f "$settings" ]; then
      mkdir -p "$(dirname "$settings")"
      printf '{}\n' > "$settings"
    fi

    for rule in "$@"; do
      tmp="$(mktemp)"
      "$jq" --arg r "$rule" '
        .permissions.allow = ((.permissions.allow // []) | if index($r) then . else . + [$r] end)
      ' "$settings" > "$tmp"
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
    "Bash(git -C * add *)"
    "Bash(git -C * commit *)"
    "Bash(git -C * status *)"
    "Bash(git -C * diff *)"

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

  # pr-gate: PR completion barrier。判定対象は allowlist に列挙した nwo だけ
  # (既定は本リポジトリのみ)なので、他リポジトリでは完全沈黙する。
  home.file.".claude/hooks/pr-gate.sh" = {
    source = repoConfig + "/claude/hooks/pr-gate.sh";
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

  home.activation.registerClaudeHooks = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${registerHooks} "$HOME/.claude/settings.json" \
      ${lib.escapeShellArg planReviewCmd} \
      ${lib.escapeShellArg wrapupStopCmd} \
      ${lib.escapeShellArg wrapupSessionStartCmd} \
      ${lib.escapeShellArg planViewCmd} \
      ${lib.escapeShellArg issueIndexCmd} \
      ${lib.escapeShellArg signPrewarmCmd} \
      ${lib.escapeShellArg prGateSessionStartCmd} \
      ${lib.escapeShellArg prGateStopCmd}
  '';

  # settings.json の permissions.allow を冪等に拡充する。registerClaudeHooks と同じ
  # DAG 位置(writeBoundary の後)で、独立した activation script として走らせる —
  # 片方が既存の hooks 登録ロジックを壊さないようにするため、jq マージの責務を
  # 混ぜない。
  home.activation.registerClaudePermissions = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${registerPermissions} "$HOME/.claude/settings.json" \
      ${lib.concatMapStringsSep " \\\n      " lib.escapeShellArg permissionRules}
  '';
}
