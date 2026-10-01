# Claude Code のフック群 — plan-review ゲート・wrap-up inbox・plan-view・
# sign-prewarm・pr-gate・issue-index を home-manager で配備する。
#
# 1) plan-review ゲート(PreToolUse / ExitPlanMode):
#    GitHub Copilot CLI(read-only custom agent)によるプランの自動レビュー。
#    gate は acceptance-convergent であり、critic は verdict を出さず、
#    judge(jq)が決定論的に gate 適格性を判定する。最終ラウンドは closer。
#    詳細は docs/claude/copilot-plan-review.md(旧 Codex 版の設計経緯も同ファイルに
#    履歴として残る)。
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
#    「CI 待ちのまま完了を宣言する」「push し忘れたまま完了する」「PR を Issue に
#    繋がないまま終わる」「見た目の変更なのに視覚証跡が無いまま終わる」の 4 事故を、
#    Stop の 1 点だけで hard gate する(base 鮮度・未コミット変更は advisory)。
#    判定対象は ~/.claude/pr-gate-repos に列挙した nwo だけ(既定は本リポジトリの
#    み)で、それ以外では完全沈黙する。中心不変条件は「揃っていない集合を緑と読ま
#    ないこと」— 期待される required check をサーバの ruleset から取り、
#    `gh pr checks --watch` の exit code ではなく取り直した --json を jq で判定
#    する。視覚証跡(G_visual)は 12) の pr-description スキルが定める本文スケルト
#    ンの `## Before / After` 節を検査し、証跡の有無だけを機械強制する(対比の完全
#    性はスキル側の責務)。詳細は docs/claude/pr-gate.md。
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
# 10) worktree-fresh-base(SessionStart, matcher: startup|resume):
#    herdr の Workspace Fork は親チェックアウトの HEAD を fetch なしで使うため、
#    新しい worktree が古い base から生まれることがある。まだ何も積んでいない
#    pristine な worktree(作業ツリークリーン かつ ahead==0 かつ behind>0)に限り、
#    fetch 後に `git merge --ff-only` で origin/<base> へ黙って揃える。履行歴を
#    持つブランチは動かさない — 既存 pr-gate.sh の base 追従 advisory とは非対称に
#    「本当に何もない」ケースだけを能動的に解消する。詳細は
#    docs/claude/worktree-fresh-base.md。
#
# 11) Opus Plan Mode のモデル実体(scripts/claude-plan-model + settings.json):
#    `model: "opusplan"` は「Plan 中は opus エイリアス、実行中は sonnet エイリアス」
#    という *エイリアスのペア* であり、各エイリアスがどの具体モデルに解決されるかは
#    settings.json の env で別に宣言できる。したがってモードは常に
#    (Plan 側, 実行側) のペアで、3 つある: fable/sonnet(既定)・opus/sonnet・
#    fable/opus。opus エイリアスを Fable に差し替えるとその瞬間 `/model opus` も
#    Fable になるため、Fable 固有のリミットが枯れたときに戻る道が塞がる
#    (fallbackModel は overload 系にしか効かず、Usage limit では発火しない)。
#    そこで `claude-plan-model` を 1 コマンドの巡回路として置く。
#    「今どのモードか」は本人が倒す実行時状態としてスクリプトが所有し、
#    home-manager は上書きしない(model キーを触らないのと同じ理由)。逆に
#    「そのモードの具体モデル ID」は宣言側の責務で、activation の `sync` が
#    claude バイナリの latest_per_family から毎回引き直す — 具体 ID を Nix に
#    書くと必ず腐るため。詳細は docs/claude/opusplan-model-aliases.md。
#    引き直しの契機は `hms`・トグル実行時に加え、`claude update` 直後
#    (config/zsh/modules/53-tools-claude.zsh、ADR-457)。claude 本体は
#    native installer が正で自動更新は宣言で OFF にしてあるため、更新は
#    この手動経路 1 本に絞られる。
#
# 12) 個人スキル(diagramming, skill-gardening, living-description,
#     issue-ref-freshness, pr-description,
#     wrapup-chores, copilot-model-bump, issue-hygiene, tracking-issue, stacked-pr):
#    hook ではなく ~/.claude/skills/ 配下に置く判断知識。diagramming は作図時に
#    「内容の型に合うジャンル・技術を選ぶ」処方と、手書き SVG に落ちた場合の
#    技術非依存の不変条件(矢印端点をボックス定義から導出する・完成の定義に視認を
#    含める等)を持つ。skill-gardening はこの知見をどう dotfiles(公開)に固定化
#    するかのメタスキルで、公開リポジトリ向けサニタイズ規則の正本を持つ。
#    living-description は Issue/PR の本文(Description)を「起票時点のスナップ
#    ショット」ではなく「現在の合意状態を表す正本」として扱い、コメントで裁定が
#    確定した時点で本文を編集し続ける習慣(複数の関連Issueに仕様が重複している
#    場合は横断的に同期する)。issue-ref-freshness はその外部参照版で、他リポジトリ
#    の issue/PR を Open 前提で参照している記述を、参照先の裁定を読んで書き直す
#    (常時監視の timer は置かず、機会発動 + 手動の一括点検)。pr-description は PR 本文の標準スケルトン(課題・
#    解決策・Before/After・検証・要確認)と、見た目に影響する変更には Before/After
#    証跡を必ず添える習慣を持つ — 証跡の有無は 5) の pr-gate(G_visual)が機械強制
#    し、対比の完全性(ペア性)はこのスキルの責務として二層に分ける。wrapup-chores
#    は 2) の wrap-up inbox のうち判断を要さない軽微な項目(未起票の inbox 行 +
#    起票済みの wrapup 由来 open Issue)をまとめて triage し、1 回の確認後に 1 つの
#    chores PR で一括対処する習慣 — hook 自体には手を入れず、削除は既存の
#    `--mark-filed` 経由のみを使う。copilot-model-bump は外部 AI CLI(Copilot CLI 等)
#    に固定 pin した具体モデル ID を、ベンダー側の GA・廃止サイクルに追従して更新する
#    定型手順(pin 箇所の棚卸し・上流確認・スラッグ実機確認・完了条件)。issue-hygiene
#    は open Issue が出自(同一 ADR / PR / 構想)ごとに束ねられずに積み上がったとき、
#    GitHub ネイティブ sub-issues 機能で親子構造を明示し、子が全決着済みなのに本文が
#    未更新のまま open で残る「腐った tracking Issue」を清算する定期衛生管理の手順
#    (旧来のチェックボックス方式は手動更新が要り腐敗するため使わない)。tracking-issue
#    は issue-hygiene から「書く/更新する」側を分離したスキルで、親 Issue を起票・
#    更新するときの本文スケルトン(目的/スコープ・完了定義・傘の外への依存・
#    スコープ外 + 維持義務の一文)・禁止事項(子 Issue へのチェックボックス参照・
#    期日の本文への記載・親を議論の場にすること)・`tracking` ラベルによる
#    findability を持つ。issue-hygiene は事後の衛生管理(clean up)、tracking-issue
#    は起票・更新時の書式(write)に責務を分ける。stacked-pr は
#    PR 同士に依存関係がある(先行 PR の成果物を後続が参照する、または同一ファイルの
#    同じ節を逐次編集する)ときに main 起点で並行させず base を親ブランチにした
#    stacked PR として積む手順 — 判定条件・分割の設計原則・rebase.updateRefs による
#    追従・Issue リンクの書き分け・GitHub ネイティブ stack 機能(`gh stack link`)の
#    使い方を持つ。PR 同士の依存関係は Issue 同士の依存関係とは別問題であることに
#    注意。hook のような settings.json 登録は不要(スキルは ~/.claude/skills/ を
#    スキャンするだけで発動する)なので home.file だけで足りる。詳細は
#    docs/claude/diagramming.md、docs/claude/skill-gardening.md、
#    docs/claude/living-description.md、docs/claude/issue-ref-freshness.md、
#    docs/claude/pr-description.md、
#    docs/claude/wrapup-chores.md、docs/claude/copilot-model-bump.md、
#    docs/claude/issue-hygiene.md、docs/claude/tracking-issue.md、
#    docs/claude/stacked-pr.md
#    (腐る事実は docs/stacked-pr-github-native.md に切り出す、ADR-0008)。
#
# 13) claude-usage(herdr の tab_bar_right command、hook ではない):
#    `/usage` を打たずに Rate Limit(5h セッション窓)と Fable の週間上限を Herdr
#    のタブバー右端に常時表示する。データ源は statusline / hooks の入力 JSON には
#    無い唯一の経路(`/usage` が内部で使う非公開 API)であり、settings.json への
#    hook 登録はしない — herdr が interval 実行して標準出力の最終行を描画する。
#    壊れたときの症状は「タブバーからこのセグメントが消えるだけ」に収束させる。
#    詳細は docs/claude/claude-usage.md。
#
# 14) グローバル ~/.agents/AGENTS.md(正本)+ ~/.claude/CLAUDE.md(router、
#     全セッション常時コンテキスト):
#    「検証可能な不確実性が現れたら情報源(Slack/Drive/GitHub/公式ドキュメント/
#    文献)を参照するか明示的に判断せよ」「発明する前に先行例を確認せよ」という
#    agent 非依存の調査規律・PR 運用・生成元明示の方針は共有 AGENTS.md が正本を
#    持ち、Copilot CLI(~/.copilot/copilot-instructions.md)には同一ソースを
#    そのままマウントする(リポジトリ単位の ADR-0016「AGENTS.md = canon、
#    CLAUDE.md = router」と同型構造をグローバル階層に適用)。~/.claude/CLAUDE.md
#    は `@~/.agents/AGENTS.md` を import する 1 行 + Claude Code 固有の施行配線
#    (gate スクリプト・ExitPlanMode・AskUserQuestion まわり)だけを持つ。
#    ~/.codex/AGENTS.md は同一ソースの単純マウントではなく、共有 AGENTS.md +
#    config/codex/AGENTS.codex.md(Codex 固有配線)を nix 評価時に結合した
#    派生物(ADR-0032 Amendment #531、Codex が `@import` 構文を持たないため)。
#    hook 注入(issue-index 方式)は動的生成が要らない静的方針には過剰、スキルは
#    呼び出し起点が要るため常時適用の方針には不向きなので、いずれも home.file で
#    配備する。store symlink による read-only 配布なので、セッション中の `#`
#    メモ追記ショートカットは書き込み失敗する — 知見の永続化は skill-gardening
#    の PR フローに乗せる想定であり、意図的な設計。詳細は
#    docs/claude/global-claude-md.md と docs/claude/global-agents-md.md、
#    docs/adr/0032-global-agent-instructions-canon.md の Amendment。
#
# 15) scope-inventory(個人スキル、global CLAUDE.md の 1 節)+ plan-scope-gate
#     (PreToolUse / ExitPlanMode):
#    Tracking Issue のような複数項目を含む依頼を Plan Mode に投げると、作業スコープ
#    の増大を気にして依頼された範囲を黙って縮小した計画を返してくることが多い。
#    global CLAUDE.md に「複数項目の依頼は計画冒頭に要求インベントリ(逐語列挙 +
#    Rn の ID)を置く」という短い規律を追加し、scope-inventory スキルがその作り方
#    (gh graphql での sub-issues 列挙・閉じた棄却タグ Blocked-Upstream/Obsolete/
#    User-Excluded・参照 Issue を Reference-Only: で書き分ける手順)を持つ。
#    plan-scope-gate.sh は指示文だけでは足りない部分(BAITBENCH: 明示的に禁止しても
#    ショートカット使用率は平均50%超)を機械検査で塞ぐ — LLM を呼ばず、jq/grep/gh
#    だけで判定する純粋な judge。plan-review / plan-view と同じ matcher に 3 つ目の
#    エントリとして並ぶ。経路A(ユーザー発言から参照 Issue を抽出し、子 sub-issues
#    のカバレッジを検査)と経路B(`## 要求インベントリ` 節内の処分の整合性を検査)の
#    2本立て。当初検討した「プラン中の縮小マーカーを起点に検査する」設計は、過去
#    プラン327本の実測で誤検知率が高すぎて棄却した。詳細は docs/claude/scope-inventory.md。
#
# 16) precedent-grounding(個人スキル、global CLAUDE.md の 1 節)+ 拡張した
#     copilot-plan-review lens A(ADR-0012):
#    プロンプトへの「敵対的レビュー」「文献調査」の都度指示を、著者(プランを
#    書く Claude)が設計判断ごとに先行例へ接地して成果物に残す形に機構化する。
#    文献調査(Huang et al. 2023 / Kamoi et al. 2024 等)が「同一コンテキストの
#    自己批評は推論・設計タスクで改善しない」と示す一方、Anthropic 公式ドキュメント
#    も「fresh context の批評者 + 明示的基準 + 報告範囲の限定」を推奨するため、
#    著者が接地し既存の文脈を切った critic(lens A)が監査する形にした。新規の
#    独立 lens は立てず lens A に統合し、premium request は増やさない。形式検査
#    (節または免除行の存在、各 Dn の出典・取得日・差分の要素)は LLM を呼ばない
#    機械 gate(plan-precedent-gate.sh)が担う。plan-review / plan-view /
#    plan-scope-gate と同じ ExitPlanMode matcher に 4 つ目のエントリとして並ぶ。
#    詳細は docs/adr/0012-precedent-grounding-over-prompted-adversarial-review.md
#    と docs/claude/precedent-grounding.md。
#
# 17) attribution-guard(PreToolUse, matcher: "Bash|mcp__.*"):
#    Claude が GitHub に書く外向きテキスト(PR/Issue の create・edit、Issue/PR
#    コメント、gh pr review のレビュー本体)に attribution フッターが載って
#    いることを保証する。PR 本文のフッターは harness 側の attribution 指示
#    由来なので、(a) コメント投稿には一切付かず、(b) 本文側も repo に強制が
#    無い(pr-gate.sh は G_link / G_visual しか見ない)という 2 つの穴があった。
#    投稿は通知が飛ぶ不可逆操作なので Stop hook では取り返せず、PreToolUse で
#    deny する。抜け道は本文マーカー No-Attribution: <理由>(pr-gate.sh の
#    No-Issue: と同型、理由必須で grep 可能)。
#
#    matcher は bleep と同じ複合 1 本 "Bash|mcp__.*" — MCP GitHub は
#    現在未接続だが、matcher を Bash 単体にすると接続した瞬間に無検査になる
#    (下の bleep の登録コメントが、旧実装の同じ欠陥を「最大の機能
#    欠陥」と記録している)。絞り込みは hook 内部の早期 exit に置く。
#    詳細は docs/claude/attribution-guard.md。
#
#    Codex CLI / Copilot CLI にも同じ判定エンジンを展開する(#192)。
#    bleep の「1つの判定エンジン + 薄い per-agent adapter」の型を
#    踏襲し、config/codex/hooks/・config/copilot/hooks/ の adapter が
#    config/claude/hooks/attribution-guard.sh を `source` してフッターの
#    エージェント名だけを差し替える。登録は bleep の Codex/Copilot
#    登録ブロック(下方)と同じ lost-update 対策の順序付けに続ける。
#
# 18) plan-fresh-gate(PreToolUse / ExitPlanMode):
#    herdr worktree を並行 Plan モードでパイプライン駆動する運用では、先発の
#    PR が merge された後も後発のエージェントがセッション開始時点の古い
#    コードベースを見たままプランを承認してしまう。worktree-fresh-base.sh は
#    SessionStart 限定の pristine ff-only 追従しか持たず、長い Plan セッション
#    中の drift はノーガードだった。この hook が ExitPlanMode 承認点そのもので
#    その隙間を塞ぐ: 常に fetch し、pristine(worktree-fresh-base.sh と同じ
#    5 条件)なら ff-only で追従、origin/<base> の進行分がプラン参照ファイルと
#    交差するときだけ deny する。pr-gate.sh の G_base は同種の drift を
#    advisory に留めるが、その根拠(block が rebase → force-push ループを
#    誘発する)はここでは成立しない — 要求するのは履歴改変ではなく「再読 +
#    再 ExitPlanMode」だけで、deny 済み SHA のセッション state 記録により
#    有限回に収束する。plan-review / plan-view / plan-scope-gate /
#    plan-precedent-gate と同じ matcher に 5 つ目のエントリとして並ぶ。
#    詳細は docs/claude/plan-fresh-gate.md。
#
# 19) stack-base-guard(PreToolUse, matcher: "Bash|mcp__.*", ADR-0027):
#    セッション内の複数 PR は依存関係を予測せず常に作成順の単一チェーンに
#    積む(uncertainty-first stacking)ことを、`gh pr create` / `gh pr edit
#    --base`(および相当する MCP GitHub 呼び出し)の作成時に機械強制する。
#    層(i) 状態レスの祖先一致検査(HEAD が他の open PR のコミットを祖先に
#    含むなら base はその PR の head でなければならない — タグでも抜けられ
#    ない)と、層(ii) セッション ID 単位の状態(2 本目以降のチェーン外
#    ブランチは本文 `Independent-PR: <理由>` を要求する)の 2 層で判定する。
#    判定不能はすべて fail-open。attribution-guard.sh の判定エンジン
#    (split_heredoc/tokenize/is_sep)を source して再利用する。完了時の
#    対になる強制(`gh stack link` の要求)は pr-gate.sh の G_stack が担う。
#    詳細は docs/adr/0027-uncertainty-first-stacking.md と
#    docs/claude/stack-base-guard.md。
#
# 20) dotfiles.claude.mcpServers(home/modules/claude-mcp-servers.nix、値は既定で空):
#    ~/.claude.json の .mcpServers を加法的に merge する extensible option。この
#    ファイルではなく独立モジュールに切り出してある(quarantine.nix と同じ
#    「1 option = 1 ファイル」の粒度)。詳細は docs/claude/claude-mcp-servers.md。
#
# 21) pr-title-guard(PreToolUse, matcher: "Bash|mcp__.*", ADR-0031):
#    PR タイトルを commit-message 契約として作成時に機械強制する。squash-only
#    運用では PR タイトルが main の commit subject になる唯一のテキストであり、
#    `gh pr create --title` / `gh pr edit --title` が Conventional Commits +
#    Angular 慣行の 11 type 閉集合(scripts/pr-title-check が判定エンジンの
#    単一ソース)に非適合なら deny する。発火は owner が tarotene のリポジトリ
#    限定(会社ホストにも common 層として配備されるため)。判定不能・checker
#    不在・非 tarotene owner はすべて fail-open。一時解除は環境変数
#    PR_TITLE_GUARD_ALLOW=1。attribution-guard.sh/stack-base-guard.sh と同じ
#    「判定エンジンを source して is_target_at を上書きする」型。Codex/Copilot
#    にも同じ判定エンジンを展開する(#192 の型を踏襲)。詳細は
#    docs/adr/0031-pr-title-as-commit-message-contract.md と
#    docs/claude/pr-title-contract.md。
#
# 22) external-send-guard(PreToolUse, matcher: "mcp__.*"):
#    Claude が Gmail MCP tool 経由で外部(自分以外)宛にメールを直接送信するのを
#    deny し、create_draft(返信は replyToMessageId 付き)へ誘導する。motivating
#    case: 公式サイトに実在するメールアドレスを一般問い合わせに使ったところ、
#    実際は求人問い合わせ専用の窓口で、相手から不審がられた(2026-09-22)。
#    アドレスの存在確認だけでは窓口の文脈までは保証されず、その文脈判定は
#    機械では行えないため、外部宛送信そのものを一律止めてユーザーの Gmail
#    上での編集・送信を承認点にする
#    設計にした。send_message/reply/forward の 3 tool だけを対象にし、
#    create_draft・読み取り系は対象外。自分のアドレス集合は
#    ~/.config/external-send-guard/self.txt(このリポジトリにはコミットしない、
#    bleep と同じ理由)。
#    Slack への拡張(#463): 別セッションでの作業中に「外部発信は常に下書きに
#    留めたい」というフィードバックがあり、同じ理由で Slack の
#    send_message/reply/schedule_message/post_message 系 tool にも宛先を
#    問わない無条件 deny を適用した(draft 系 tool は対象外)。GitHub は
#    対象外 — AGENTS.md の完了定義(`gh pr create` 等を確認なしで実行)と
#    矛盾するため。詳細は docs/claude/external-send-guard.md。
#
# 23) external-call-scheduling(個人スキル):
#    電話・来店・窓口対応など Claude が代行できないハンドオフ作業を、トーク
#    スクリプトを文書化して渡すだけで終わらせない。相手の営業時間・定休日と
#    ユーザーの予定表上の空き時間を突き合わせ、衝突しない枠にリマインダー
#    予定を作成するところまでを手順化する。Claude が提案する予定専用の
#    カレンダーが既にあればそれを使い、無ければ書き込み先をユーザーに確認
#    する。カレンダー・メール等の外部サービスに送る文章は skill-gardening の
#    サニタイズ規則(固有名詞・URL・内部識別子を書かない)を適用する対象だと
#    明記している — リポジトリ内の成果物とは別の基準がかかる。詳細は
#    docs/claude/external-call-scheduling.md。
#
# 24) decision-colocation-guard(PreToolUse, matcher: "Bash|mcp__.*", ADR-396):
#    決定成果物(ADR/設計文書/skill)の新規追加、または既存 ADR への
#    `## Amendment` 追加を、その決定を執行する実ファイルの同梱なしに `gh pr
#    create` させない。判定は scripts/decision-colocation-check(判定エンジン
#    単一ソース、CI required check と共有)に委譲する。執行点として認める
#    パスは「非 .md かつ docs/ 配下でない」の 2 述語のみ。実在するが無変更の
#    パスの併記だけでは合格しない(ADR-387 を意図的に不合格側に倒して検算
#    — docs/claude/decision-colocation.md 参照)。attribution-guard.sh/
#    stack-base-guard.sh/pr-title-guard.sh と同じ「判定エンジンを source
#    して is_target_at を上書きする」型。Codex/Copilot adapter は作らない
#    — CI required check が全エージェント共通の backstop になるため。一時
#    解除は環境変数 SKIP_DECISION_COLOCATION_GUARD=1。詳細は
#    docs/adr/396-decision-colocation.md と docs/claude/decision-colocation.md。
#
# 25) slot-availability(個人スキル):
#    調整さん等の候補日程一覧を Google Calendar の空き状況と突き合わせ、
#    朝・昼・夜の3コマ単位で当たり判定(○/△/×)を決定的なスクリプト
#    (scripts/slot-hit.sh、bash + yq + jq。判定コアは judge.jq/finalize.jq、
#    共通関数は lib.jq。別の private な person-state リポジトリ側の ADR-0009
#    D13 により旧 slot-hit.py〔Python〕から移植)に委譲する。終日イベント
#    (試験本番等)を見落として
#    実際は拘束される日を ○ と誤判定しないよう、`soft_day_prefixes` に一致
#    する終日イベントの日に時間指定の確定予定が無ければ実行を止める。調べて
#    判明した拘束時間は設定ファイルではなく Google Calendar 側に時間指定の
#    予定として書き戻す(判定ルールと個々の予定の事実を二重管理しない)。
#    external-call-scheduling とは発火条件が異なる(こちらは Claude 自身が
#    実行できる回答作業)ため独立したスキルにした。個人のカレンダー allowlist
#    等は private な person-state リポジトリに置き、既定パス
#    `~/.config/slot-availability/config.toml` へ手動シンボリックリンクで
#    接続する(dotfiles には置かない)。詳細は docs/claude/slot-availability.md。
#
# 26) performance-planning(個人スキル):
#    演奏本番(アマオケ・室内楽・ソロ)の練習計画提案(需要 vs 供給・
#    マイルストーン到来の確認)・合わせ調整の統合(slot-availability の
#    judge/finalize に委譲し、確定後を person-state リポジトリ側の TOML へ
#    反映)・当日タイムテーブル/遠征の Calendar dispatch を行う。データの
#    正本(`state/performances/`、ADR-0009)は別の private な person-state
#    リポジトリに置き、`scripts/performance-hub`(writing-style-hub と
#    同型のマーカーファイル/環境変数間接参照)でパスを解決する。yq の
#    TOML エンコーダが配列・テーブルの書き込みをサポートしないため、TOML
#    への実際の書き込みは Claude が Edit ツールで行う分担にしている。
#    詳細は docs/claude/performance-planning.md。
#
# 27) handoff(個人スキル)+ pr-gate.sh の中断ハンドオフ節 + issue-index.sh の
#    着手可能な handoff:ai 節:
#    ユーザーの指示で作業を途中で打ち切るとき、残タスクを Human/AI 双方に
#    振り分けて GitHub Issue に起票し、後で再開できる状態にする。WIP は
#    commit → push → Draft PR(本文に `Handoff: #N`)、担当は閉語彙ラベル
#    `handoff:human`/`handoff:ai`、順序は GitHub ネイティブの Issue
#    dependencies(blocked_by)で表す。誤りやすい API 操作(ラベル確認・
#    新設、blocked_by の設定)は `scripts/handoff.sh` の決定論的サブコマンドに
#    寄せる(番号→id の解決を含む)。詳細は docs/claude/handoff.md。
#
# 28) claude-routines(個人スキル、ADR-519-routines-declaration-in-repo):
#    Claude Code routine(claude.ai の scheduled cloud agent)の設定・
#    prompt を対象リポジトリ自身の `.claude/routines/<name>.{json,md}` に
#    正本化する。差分計算(7分類 + unmanaged 検出)は
#    `scripts/routines-plan.sh`(bash + jq、決定的)に固定し、ローカルの
#    セッションはその出力(create/update body)を `RemoteTrigger` に
#    1:1 で中継するだけにする(SKILL.md)。クラウドの自己監査 routine
#    向けの汎用手順は `auditor.md`(dotfiles の clone から
#    `scripts/routines-plan.sh` を直接呼ぶ、`gh` CLI ではなく
#    `mcp__github__*` を使う)。詳細は docs/claude/claude-routines.md。
#
# 29) pkexec-guard(PreToolUse, matcher: Bash、PR #326 のグリルセッション、
#    2026-09-28):
#    agent セッションが root 権限のコマンドを実行するとき、polkit
#    (`pkexec`)の認証ダイアログに全文が表示される範囲だけを通す。
#    PR #326 は当初 `SUDO_ASKPASS` helper が pinentry に生の Assuan で
#    `GETPIN` を発行し、得たパスワードを stdout に平文で返す方式だった
#    が、#348(pinentry への直接呼び出しが人間の操作なしに実パスワードを
#    返した事故)→ #421(ADR-0003 Amendment 5: 同一 uid からの pinentry
#    直叩きは gpg-agent 側では塞げない残余リスクと確定)を踏まえ、その
#    方式は不採用と裁定し直した。代わりに polkit を使う——パスワードは
#    root 側の polkit-agent-helper-1 が検証するだけで agent の出力には
#    一切出ない。判定は Rust(crates/pkexec-guard、ADR-0024)の閉じた
#    許可リスト: 単純な 1 コマンド `/usr/bin/pkexec <絶対パス> <引数…>`
#    のみを通し、複合コマンド・展開・80 文字超(pkexec 自身のダイアログ
#    `cmdline_short` の省略なし表示上限)・許可リスト外・root 非所有また
#    は group/other 書き込み可能なパスはすべて deny する。裸の `pkexec`
#    も deny する(`~/.local/bin`・`~/bin` は PATH 先頭にあり、ユーザーが
#    書き込める場所に同名の偽 pkexec を置けてしまうため)。`sudo -A`/
#    `-S`/`--askpass`/`--stdin`/`SUDO_ASKPASS` の再導入も全 Agent 共通で
#    deny する。Codex CLI / Copilot CLI からは pkexec を一切許さない
#    (明示 ask ルールが auto 系モードでも確認を強制することを確認できて
#    いるのは Claude Code のみ)。`permissions.ask` の
#    `Bash(/usr/bin/pkexec *)` が、guard を通過したコマンドに改めて人間の
#    確認を求める(auto mode でも分類器より先に評価される)。詳細は
#    docs/claude/pkexec-guard.md。
#
# 30) new-tool-guard(PreToolUse, matcher: Write、ADR-543「既存手段の前倒し
#    接地と、決定論への昇格導線」、2026-09-28、段2/4):
#    「同じことをフルスクラッチではなく既存の枯れた技術で実現できないか」
#    (Q1)を、気付いたときではなく新しい道具・単位を Write する着手の瞬間
#    に前倒しで問う。対象は shebang(`#!`)で始まる新規ファイル、
#    `bin`/`scripts`/`hooks`/`cmd` を構成要素に含む新規パスの新規ファイル、
#    パッケージマニフェスト(`Cargo.toml`/`package.json`/`pyproject.toml`/
#    `go.mod`/`flake.nix`)の新設のいずれか(判定は crates/new-tool-guard
#    の `classify` サブコマンドが単一正本、pr-gate.sh の `judge_prior` も
#    同じ判定を呼ぶ)。テスト・fixture・scratchpad 配下と、ディスク上に
#    既に存在するファイル(上書き)は対象外。該当かつ、現在の worktree の
#    session ledger(git toplevel キー、`~/.claude/new-tool-guard/state/`)
#    にそのパスの `既存手段:` 行が未登録なら deny する——deny メッセージが
#    `register` コマンドの完全形を同梱するので、登録して再試行すれば通る。
#    段1(ADR-543)の `## 先行例との対比` 節側の `既存手段:` 必須化との
#    使い分けは「着手前に Plan を書いたか」で決まらない: Plan を書かない
#    小さな作業でもこの hook が発火する。Claude Code 専用(#531、Codex/
#    Copilot 展開は別 Issue)。
#
# 31) feedback-target-guard(PreToolUse, matcher: Bash(gh *)、ADR-543、
#    2026-09-28、段3/4):
#    Q2(LLM/散文 → 決定論への昇格)の兆候の1つ「同じ規範・skill を指す
#    feedback Issue の再発」を機械的に集計できるようにする。`gh issue
#    create --label feedback`(カンマ区切りの複数ラベルも可)を検出し、
#    本文に `Target: skill/<name>` / `Target: agents-md/<節見出し>` /
#    `Target: hook/<name>` のいずれかが無い、またはその実体が存在しない
#    場合に deny する。コマンド解析(heredoc 分離・トークナイザ)は
#    attribution-guard.sh を source して再利用する(ADR-0024 の Rust 既定
#    に対する例外 — stack-base-guard.sh / adr-number.sh / pr-title-
#    guard.sh と同じ「既存の bash 資産を source する」理由、
#    rust-migration.toml で追跡)。
#
# 32) cmd-hash-log(PostToolUse, matcher: Bash、ADR-543、2026-09-28、段3/4):
#    Q2 の兆候の2つ目「同じコードブロックが改変なしに繰り返し実行されて
#    いる」(逐語反復)を検出するための入力を記録する。実行された Bash
#    コマンドの正規化(空白圧縮・行末継続結合)+ハッシュ(FNV-1a、依存
#    ゼロ)だけを `~/.local/state/claude/cmd-hashes.jsonl` に1行1レコード
#    で追記する——コマンド本文そのものはどこにも書かない(ADR-0011 の
#    プライバシー規約 `agent-turn-log.sh` をさらに強めた形)。判定は返さ
#    ず、記録が失敗しても常に exit 0(fail-open)。正規化+ハッシュ関数は
#    `hook_io::cmd_hash` が単一正本で、段4 の promotion-detect が SKILL.md
#    のコードブロックを同じ関数でハッシュして照合する。
#
# 33) pr-confirm-guard(PreToolUse, matcher: "Bash|mcp__.*"、
#    docs/claude/pr-confirm-guard.md、2026-09-30 改定):
#    PR 本文に (i) 未チェックの task list(`- [ ]`)、または (ii) `## 要確認`
#    に Issue 参照(`#N` または issues URL)を持たない項目があれば
#    `gh pr create/edit` の呼び出し時に deny する。人の確認が要る残作業は
#    後続 Issue へ払い出し、「要確認」はその Issue へのポインタだけを書く
#    節に転換した(社内の別リポジトリで観測された、未チェック task list を
#    残したままマージされた実例が動機)。全リポジトリで発火する(owner スコープ
#    を持たない — attribution-guard.sh と同じ「書き手側の規律」の類型)。
#    escape hatch は PR_CONFIRM_GUARD_ALLOW=1。Codex 版 adapter あり(下記
#    registerCodexPrConfirmGuardHooks)。
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
  bleep,
  ...
}:
let
  repoConfig = ../../config;
  hooksDir = "${config.home.homeDirectory}/.claude/hooks";
  planReviewCmd = "bash '${hooksDir}/copilot-plan-review.sh'";
  wrapupStopCmd = "bash '${hooksDir}/wrapup-stop-gate.sh'";
  # Codex 向けは同じファイルを、フッターの agent 名/URL だけ環境変数で
  # 差し替えて直接登録する(adapter ファイルを新設しない — ADR-0032
  # Amendment #531、D8)。attribution-guard の Codex adapter と同じ
  # 定数値。
  codexWrapupStopCmd = "ATTRIBUTION_AGENT_NAME='Codex CLI' ATTRIBUTION_AGENT_URL='https://learn.chatgpt.com/docs/codex/cli' bash '${hooksDir}/wrapup-stop-gate.sh'";
  wrapupSessionStartCmd = "bash '${hooksDir}/wrapup-session-start.sh'";
  planViewCmd = "bash '${hooksDir}/plan-view.sh'";
  issueIndexCmd = "bash '${hooksDir}/issue-index.sh'";
  signPrewarmCmd = "bash '${hooksDir}/sign-prewarm.sh'";
  prGateSessionStartCmd = "bash '${hooksDir}/pr-gate.sh' session-start";
  prGateStopCmd = "bash '${hooksDir}/pr-gate.sh' stop";
  # pr-gate.sh の判定(G_pr/G_link/G_visual/G_stack)は agent 名を一切
  # 参照しないため、Codex にも同じファイルを adapter 無しで直接登録する
  # (ADR-0032 Amendment #531)。
  codexPrGateStopCmd = "bash '${hooksDir}/pr-gate.sh' stop";
  # #416 (ADR-0024 Stage 4c): git/worktree 系の hook は Rust バイナリ
  # (crates/git-worktree-allow など、pkgs.dotfiles-tools)になり、下の
  # home.file が ~/.claude/hooks/<name>(拡張子無し)として配備する。bash を
  # 介さず直接起動する。旧 `bash '….sh'` の command は retiredHookEntries /
  # --retire で完全一致削除する(legacy*Cmd)。
  gitWorktreeAllowCmd = "'${hooksDir}/git-worktree-allow'";
  gitStashGuardCmd = "'${hooksDir}/git-stash-guard'";
  legacyGitWorktreeAllowCmd = "bash '${hooksDir}/git-worktree-allow.sh'";
  legacyGitStashGuardCmd = "bash '${hooksDir}/git-stash-guard.sh'";
  legacyCodexGitStashGuardCmd = "bash '${config.home.homeDirectory}/.codex/hooks/git-stash-guard.sh'";
  legacyWorktreeFreshBaseCmd = "bash '${hooksDir}/worktree-fresh-base.sh'";
  legacyWorktreeCreateGuardCmd = "bash '${config.home.homeDirectory}/.local/libexec/git-worktree-create-guard'";
  legacyWorktreeAuditContextCmd = "bash '${config.home.homeDirectory}/.local/bin/git-audit-worktrees' --context";
  # 上流(tarotene/bleep、旧 tarotene/publish-guard、ADR-0009)は #25-28
  # (Rust hook cutover + rename)で旧 3 adapter(claude-adapter.sh/
  # adapters/{codex,copilot}-adapter.sh)を廃止し、単一 shim
  # hooks/bleep.sh --host=<name> に統合した。bleep.sh は `realpath "$0"`
  # で自己解決するため、旧 claude-adapter.sh と違い CLAUDE_PLUGIN_ROOT の
  # 明示注入が不要になった(home.file 配備でも plugin 配布でも同じ
  # 呼び出し形で動く)。
  # #561: bleep.sh の main() は BLEEP_HOOK_BIN を見つけて exec するだけで
  # BLEEP_LEX_BIN は自分では伝播しない(bleep 本体の cmd_scan_bash_command
  # が使う継ぎ目は別で、見つからなければ素の `bleep-hook`(PATH 解決、
  # ~/.cargo/bin にフォールバック)に縮退する) — 継ぎ目は2箇所あるので
  # 両方をこの呼び出し元で明示的に pin する。どちらも同じ flake input
  # `bleep`(pin 済み rev)から crane でビルドした
  # ${hooksDir}/bleep-hook(下の home.file)を指すので、bash 本体
  # (hooks/bleep.sh 経由)と Rust バイナリが常に同一 rev から導出され、
  # `~/.cargo/bin` の手動 install 経由での版ずれが構造的に起きなくなる。
  bleepHookBinPath = "${hooksDir}/bleep-hook";
  bleepEnvPrefix = "BLEEP_HOOK_BIN='${bleepHookBinPath}' BLEEP_LEX_BIN='${bleepHookBinPath}'";
  bleepClaudeCmd = "${bleepEnvPrefix} bash '${hooksDir}/bleep/hooks/bleep.sh' --host=claude";
  bleepCodexCmd = "${bleepEnvPrefix} bash '${hooksDir}/bleep/hooks/bleep.sh' --host=codex";
  bleepCopilotCmd = "${bleepEnvPrefix} bash '${hooksDir}/bleep/hooks/bleep.sh' --host=copilot";
  # #561 で env prefix を足す前の command 文字列。register() は完全一致でしか
  # 既存登録を見つけられないため(claude.nix 内の register() 定義のコメント
  # 参照)、これを先に --retire/completely-remove してからでないと、新旧
  # 2本が settings.json に同時に残ってしまう。
  legacyBleepClaudeCmdUnprefixed = "bash '${hooksDir}/bleep/hooks/bleep.sh' --host=claude";
  legacyBleepCodexCmdUnprefixed = "bash '${hooksDir}/bleep/hooks/bleep.sh' --host=codex";
  legacyBleepCopilotCmdUnprefixed = "bash '${hooksDir}/bleep/hooks/bleep.sh' --host=copilot";
  # 旧(別リポジトリ切り出し前)の command 文字列。settings.json から完全一致
  # 削除するためだけに残す(下の retiredHookEntries)。
  legacyPublicPublishGuardCmd = "bash '${hooksDir}/public-publish-guard.sh'";
  # #25-28 で置き換えられた旧 3 adapter の command 文字列。settings.json /
  # ~/.codex/hooks.json / ~/.copilot/settings.json から完全一致削除する
  # ためだけに残す(下の retiredHookEntries、および register-{codex,copilot}
  # -hooks への --retire 呼び出し)。ファイル自体は upstream から削除済みで、
  # 削除せず放置すると存在しないパスを指したまま毎 tool call で失敗する。
  legacyPublishGuardClaudeAdapterCmd = "CLAUDE_PLUGIN_ROOT='${hooksDir}/publish-guard' bash '${hooksDir}/publish-guard/hooks/claude-adapter.sh'";
  legacyPublishGuardCodexAdapterCmd = "bash '${hooksDir}/publish-guard/adapters/codex-adapter.sh'";
  legacyPublishGuardCopilotAdapterCmd = "bash '${hooksDir}/publish-guard/adapters/copilot-adapter.sh'";
  registerCodexHooks = pkgs.writeShellScript "register-codex-hooks" (
    builtins.readFile ../../scripts/register-codex-hooks
  );
  registerCopilotHooks = pkgs.writeShellScript "register-copilot-hooks" (
    builtins.readFile ../../scripts/register-copilot-hooks
  );
  # attribution-guard の Codex/Copilot adapter(#192)。adapter は
  # CLAUDE_HOOKS_DIR → ソースツリー相対 → $HOME/.claude/hooks の順で
  # attribution-guard.sh を探す(#602: 配備先に ../../claude/hooks は無い)
  # ので、この2つは ~/.codex/hooks/・~/.copilot/hooks/ 直下に置く
  # (herdr-{codex,copilot}-metadata.sh と同じ配置)。
  codexAttributionGuardCmd = "bash '${config.home.homeDirectory}/.codex/hooks/attribution-guard.sh'";
  copilotAttributionGuardCmd = "bash '${config.home.homeDirectory}/.copilot/hooks/attribution-guard.sh'";
  herdrMetadataCmd = "bash '${hooksDir}/herdr-claude-metadata.sh'";
  statusLineCmd = "bash '${hooksDir}/claude-statusline.sh'";
  worktreeFreshBaseCmd = "'${hooksDir}/worktree-fresh-base'";
  worktreeCreateGuardCmd = "'${config.home.homeDirectory}/.local/libexec/git-worktree-create-guard'";
  worktreeAuditContextCmd = "'${config.home.homeDirectory}/.local/bin/git-audit-worktrees' --context";
  planScopeGateCmd = "bash '${hooksDir}/plan-scope-gate.sh'";
  planPrecedentGateCmd = "bash '${hooksDir}/plan-precedent-gate.sh'";
  planFreshGateCmd = "bash '${hooksDir}/plan-fresh-gate.sh'";
  attributionGuardCmd = "bash '${hooksDir}/attribution-guard.sh'";
  # stack-base-guard(ADR-0027)は attribution-guard.sh を source するので
  # 同じ ~/.claude/hooks/ ディレクトリに置く(相対 source パス
  # "$(dirname ...)/attribution-guard.sh" が解決できる配置)。
  stackBaseGuardCmd = "bash '${hooksDir}/stack-base-guard.sh'";
  # pr-title-guard(ADR-0031)も同じ理由で attribution-guard.sh と同階層。
  prTitleGuardCmd = "bash '${hooksDir}/pr-title-guard.sh'";
  # Codex/Copilot 版 pr-title-guard adapter(#192 の型を踏襲)。
  # attribution-guard の Codex/Copilot adapter と同じく ~/.codex/hooks/・
  # ~/.copilot/hooks/ 直下に置き、pr-title-guard.sh は $HOME/.claude/hooks
  # 経由で解決される(#602)。
  codexPrTitleGuardCmd = "bash '${config.home.homeDirectory}/.codex/hooks/pr-title-guard.sh'";
  copilotPrTitleGuardCmd = "bash '${config.home.homeDirectory}/.copilot/hooks/pr-title-guard.sh'";
  # pr-confirm-guard(docs/claude/pr-confirm-guard.md)も attribution-guard.sh
  # を source するので同階層。
  prConfirmGuardCmd = "bash '${hooksDir}/pr-confirm-guard.sh'";
  # Codex 版 pr-confirm-guard adapter(pr-title-guard.sh と同じ型)。
  # ~/.codex/hooks/ 直下に置き、pr-confirm-guard.sh は $HOME/.claude/hooks
  # 経由で解決される(#602)。Copilot adapter は作らない(依頼は Codex のみ、
  # #3962 相当)。
  codexPrConfirmGuardCmd = "bash '${config.home.homeDirectory}/.codex/hooks/pr-confirm-guard.sh'";
  # git-stash-guard/stack-base-guard/adr-number の Codex adapter(ADR-0032
  # Amendment #531)。attribution-guard/pr-title-guard の Codex adapter と
  # 同じく ~/.codex/hooks/ 直下に置く(source 先の解決は #602 を参照)。
  # #416: Codex adapter(bash)は廃止し、同じ Rust バイナリを --host codex で
  # 直接登録する。
  codexGitStashGuardCmd = "'${hooksDir}/git-stash-guard' --host codex";
  codexStackBaseGuardCmd = "bash '${config.home.homeDirectory}/.codex/hooks/stack-base-guard.sh'";
  codexAdrNumberCmd = "bash '${config.home.homeDirectory}/.codex/hooks/adr-number.sh'";
  # codex-plan-gate(ADR-0032 Amendment #531、docs/claude/codex-plan-gate.md):
  # Codex の Plan mode に ExitPlanMode 相当の機械検査を課す Stop hook。
  # 独立ファイル(他 hook を source しない)なので配置は
  # ~/.codex/hooks/ 直下のみで足りる。
  codexPlanGateCmd = "bash '${config.home.homeDirectory}/.codex/hooks/codex-plan-gate.sh'";
  # rulesets-write-guard(Rust)は Claude/Codex で入出力形が同一
  # (crates/hook-io の Agent::Claude | Agent::Codex が同じ JSON 形を emit
  # すると確認済み — crates/hook-io/src/decision.rs)なので adapter を
  # 挟まず、Claude と同じバイナリを直接 Codex にも登録する。
  codexRulesetsWriteGuardCmd = "'${config.home.homeDirectory}/.claude/hooks/rulesets-write-guard'";
  # decision-colocation-guard(ADR-396, docs/claude/decision-colocation.md)
  # も attribution-guard.sh を source するので同階層。Codex/Copilot adapter
  # は意図的に作らない — CI required check が全エージェント共通の
  # backstop として機能するため(ADR-396 の非目標に明記)。
  decisionColocationGuardCmd = "bash '${hooksDir}/decision-colocation-guard.sh'";
  # adr-number(ADR-380, docs/claude/adr-numbering.md)も attribution-guard.sh
  # を source するので同階層。段3(利便性層)のみ — deny は一切しない。
  adrNumberCmd = "bash '${hooksDir}/adr-number.sh'";
  # gh-edit-allow(#392, docs/claude/gh-edit-allow.md): Rust 製(crates/
  # gh-edit-allow、ADR-0024)。bash を挟まず実行ファイルを直接呼ぶ。配置は
  # 他の hook と同じ ~/.claude/hooks/ の安定パス — store path を command に
  # 直接書くと、ビルドのたびに command 文字列が変わり、完全一致で存在判定
  # する registerHooks が旧エントリを残し続けるため。
  ghEditAllowCmd = "'${hooksDir}/gh-edit-allow'";
  # rulesets-write-guard(ADR-0000-rulesets-declaration-in-repo D7):
  # `gh api` による ruleset(required_status_checks 等)の直接書換を deny
  # する。gh-edit-allow と同じ理由(判定に既存の巨大 bash 資産を source
  # する必要が無い)で ADR-0024 の既定どおり Rust。
  rulesetsWriteGuardCmd = "'${hooksDir}/rulesets-write-guard'";
  # routines-write-guard(28番、ADR-0000-routines-declaration-in-repo D7):
  # 宣言外の cron routine create/update(RemoteTrigger)を deny する。
  # rulesets-write-guard と同じ理由で Rust だが、判定対象が Bash コマンド
  # 文字列ではないため env var による bypass は持たない(crates/
  # routines-write-guard/src/lib.rs 参照)。
  routinesWriteGuardCmd = "'${hooksDir}/routines-write-guard'";
  # pkexec-guard(29番、docs/claude/pkexec-guard.md、PR #326 のグリルセッション
  # で SUDO_ASKPASS 方式を不採用に裁定し直した結果の設計): rulesets-write-guard
  # と同じ理由(判定に既存の bash 資産を source する必要が無い)で ADR-0024
  # の既定どおり Rust。3 エージェントから同じ 1 バイナリを呼ぶ(bash の
  # `source` に相当する仕組みが Rust に無いため、attribution-guard のような
  # per-agent adapter ファイルは作らず、`--agent` フラグで出力形式を切り替える
  # — crates/pkexec-guard/src/main.rs)。
  pkexecGuardCmd = "'${hooksDir}/pkexec-guard'";
  pkexecGuardCodexCmd = "'${hooksDir}/pkexec-guard' --agent codex";
  pkexecGuardCopilotCmd = "'${hooksDir}/pkexec-guard' --agent copilot";
  # new-tool-guard(30番、docs/adr/543-existing-means-and-deterministic-
  # promotion.md D1): 新しい道具・単位(shebang 付き新規ファイル、
  # bin/scripts/hooks/cmd 配下の新規ファイル、パッケージマニフェストの
  # 新設)を Write する前に、`既存手段:` の session ledger 登録を要求する。
  # pkexec-guard と同じ理由(判定に既存の bash 資産を source する必要が
  # 無い)で ADR-0024 の既定どおり Rust。Claude Code 専用(#531、Codex/
  # Copilot への展開は別 Issue の主題)。
  newToolGuardCmd = "'${hooksDir}/new-tool-guard'";
  # feedback-target-guard(ADR-543 段3): `gh issue create --label feedback` に
  # `Target:` 行を要求する。attribution-guard.sh の heredoc 分離・トークナイザ
  # を source して再利用する(rulesets-write-guard/pkexec-guard とは逆に、
  # ADR-0024 の Rust 既定に対する例外 — stack-base-guard.sh / adr-number.sh /
  # pr-title-guard.sh と同じ理由、rust-migration.toml 参照)。
  feedbackTargetGuardCmd = "bash '${hooksDir}/feedback-target-guard.sh'";
  # repo-create-guard(ADR-0013 Amendment 2026-09-29, docs/claude/repo-create-
  # guard.md): `gh repo create` / `gh api -X POST user/repos`・`orgs/*/repos`
  # を deny し、repo-charter スキルの手順に強制的に載せる。attribution-
  # guard.sh のコマンド解析エンジンを source して再利用する(stack-base-
  # guard.sh/feedback-target-guard.sh と同じ ADR-0024 の Rust 既定に対する
  # 例外)。
  repoCreateGuardCmd = "bash '${hooksDir}/repo-create-guard.sh'";
  # cmd-hash-log(ADR-543 段3): 実行された Bash コマンドの正規化ハッシュ
  # だけを記録する(コマンド本文は書かない、ADR-0011 のプライバシー規約を
  # 踏襲)。逐語反復検出(段4 の promotion-detect)の入力。何も判定しない
  # ため ADR-0024 の既定どおり Rust だが、他 hook を deny/allow させる
  # 種類のものではない。
  cmdHashLogCmd = "'${hooksDir}/cmd-hash-log'";
  # external-send-guard(22番、docs/claude/external-send-guard.md): 外部宛
  # メールの直接送信を deny し create_draft へ誘導する。他 hook を source
  # しない独立ファイルだが、配置ディレクトリは揃えておく。
  externalSendGuardCmd = "bash '${hooksDir}/external-send-guard.sh'";
  # agent-turn-log(UserPromptSubmit + Stop、docs/adr/0011): 1 スクリプトが
  # 2 イベントに同一 command で登録され、`.hook_event_name` で分岐する
  # (herdr-claude-metadata.sh と同じ形)。出力は
  # ${XDG_STATE_HOME:-~/.local/state}/daily-report/agent-events.jsonl —
  # 別リポジトリ(daily-report)がそのまま読む契約なので、フィールド名は
  # 変更しないこと。
  agentTurnLogCmd = "bash '${hooksDir}/agent-turn-log.sh'";
  # Codex 向けは同じファイルを AGENT_NAME=codex だけ差し替えて直接登録する
  # (adapter ファイルを新設しない — ADR-0032 Amendment #531、D8)。
  codexAgentTurnLogCmd = "AGENT_NAME=codex bash '${hooksDir}/agent-turn-log.sh'";
  # atuin hook claude-code(docs/adr/0011): atuin 自身が提供するエージェント
  # フック — Bash tool 呼び出しの command/cwd/duration/exit code を atuin の
  # history.db に記録する。`atuin hook install claude-code` は settings.json
  # を直接書き換えて宣言的管理の外に出るため使わず、この repo の既存の
  # register() 経由で配線する。コマンド文字列は atuin, "Agent Hooks",
  # <https://docs.atuin.sh/latest/guide/agent-hooks/>(2026-09-14 取得)が
  # 文書化する契約のとおり(引数なし)。
  atuinHookClaudeCodeCmd = "atuin hook claude-code";
  # 旧 Codex 版の plan-review hook command。中身(--search exec --output-schema
  # 等)ごと copilot-plan-review.sh に置き換えたので、activation が settings.json
  # から完全一致で削除してから新 command を登録する(下の retiredHookEntries)。
  legacyCodexPlanReviewCmd = "bash '${hooksDir}/codex-plan-review.sh'";

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
    # Codex → Copilot 移行(docs/claude/copilot-plan-review.md)。旧 command 文字列を
    # PreToolUse/ExitPlanMode から完全一致削除してから、新 planReviewCmd を登録する。
    {
      event = "PreToolUse";
      command = legacyCodexPlanReviewCmd;
    }
    # public-publish-guard の上流分離(ADR-0009)。旧 command は matcher が
    # "Bash" 単体だったが、新 command は "Bash|mcp__.*" で登録する — register()
    # の存在判定は command の完全一致だけで matcher を見ないため、retire を
    # 挟まないと matcher が古いまま更新されない(既知の罠、claude.nix 内の
    # register() 定義のコメント参照)。
    {
      event = "PreToolUse";
      command = legacyPublicPublishGuardCmd;
    }
    # #25-28: publish-guard → bleep への改名 + Rust hook cutover。旧
    # claude-adapter.sh(CLAUDE_PLUGIN_ROOT 注入込み)の command 文字列を
    # 完全一致削除してから、新 bleepClaudeCmd を登録する。
    {
      event = "PreToolUse";
      command = legacyPublishGuardClaudeAdapterCmd;
    }
    # #561: bleep-hook(Rust)の版ずれ対策で BLEEP_HOOK_BIN/BLEEP_LEX_BIN の
    # env prefix を追加した。旧(prefix 無し)の command 文字列を先に削除する。
    {
      event = "PreToolUse";
      command = legacyBleepClaudeCmdUnprefixed;
    }
    # #416 (ADR-0024 Stage 4c): git/worktree 系 hook の bash 版 → Rust 版。
    # register() の存在判定は command の完全一致だけなので、旧 command を
    # 先に削除しないと旧エントリが消えたパスを指したまま残る。
    {
      event = "PreToolUse";
      command = legacyGitWorktreeAllowCmd;
    }
    {
      event = "PreToolUse";
      command = legacyGitStashGuardCmd;
    }
    {
      event = "SessionStart";
      command = legacyWorktreeFreshBaseCmd;
    }
    {
      event = "PreToolUse";
      command = legacyWorktreeCreateGuardCmd;
    }
    {
      event = "SessionStart";
      command = legacyWorktreeAuditContextCmd;
    }
  ];

  # かつて配って撤回した statusLine。syncStatusLine が .statusLine.command との
  # 完全一致でキーを削除する対象。今回は herdr-sidebar-metadata を新規導入するので
  # 空 — 将来この機能自体を取り下げるときに statusLineCmd をここへ移す。
  retiredStatusLineCommands = [ ];

  # Opus Plan Mode のモデル実体 — 具体値は scripts/claude-plan-model が持つ。
  #
  # `model: "opusplan"` は Plan 中に opus エイリアス、実行中に sonnet エイリアスを
  # 解決する。各エイリアスの解決先は settings.json の
  # `.env.ANTHROPIC_DEFAULT_{OPUS,SONNET}_MODEL` で乗っ取れるので、モードは
  # (Plan 側, 実行側) のペアになる — fable/sonnet(既定)・opus/sonnet・fable/opus。
  #
  # ここに具体モデル ID を書かないのは、書くと必ず腐るから。エイリアス文字列
  # (`fable`)は env の値として使えず(API が unrecognized_model で拒否する)、
  # 具体 ID は世代が上がるたびに手で追う必要がある — 実際 `claude-fable-5` の pin は
  # claude 2.1.263 の時点で既に 1 世代遅れていた。代わりに、どちらのモードかだけを
  # settings.json に残し、その具体 ID は claude バイナリに焼かれた
  # `latest_per_family` から毎回引き直す。宣言(activation)が持つのは「引き直す
  # 規則」であって、「今どのモードか」ではない — モードは Fable のリミットが
  # 枯れたときに本人が倒す実行時状態で、`.model` を宣言で固定しないのと同じ理由で
  # home-manager は上書きしない。
  #
  # 詳細は docs/claude/opusplan-model-aliases.md。
  planModelScript = ../../scripts/claude-plan-model;

  # activation の PATH には jq も ~/.local/bin(claude 本体の置き場。自己更新する
  # ので nix 管理外)も載っていない。sync はその両方を読むので明示的に足す。
  planModelSyncPath = lib.makeBinPath [
    pkgs.jq
    pkgs.coreutils
    pkgs.gnugrep
    pkgs.gnused
  ];

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
    bleep_claude="$1";          shift
    herdr_metadata="$1";        shift
    worktree_fresh_base="$1";   shift
    worktree_create_guard="$1"; shift
    worktree_audit_context="$1"; shift
    plan_scope_gate="$1";        shift
    plan_precedent_gate="$1";    shift
    plan_fresh_gate="$1";        shift
    attribution_guard="$1";      shift
    agent_turn_log="$1";         shift
    atuin_hook_claude_code="$1"; shift
    stack_base_guard="$1";       shift
    pr_title_guard="$1";         shift
    pr_confirm_guard="$1";       shift
    decision_colocation_guard="$1"; shift
    external_send_guard="$1";    shift
    adr_number="$1";             shift
    gh_edit_allow="$1";          shift
    rulesets_write_guard="$1";   shift
    routines_write_guard="$1";   shift
    pkexec_guard="$1";           shift
    new_tool_guard="$1";         shift
    feedback_target_guard="$1";  shift
    repo_create_guard="$1";      shift
    cmd_hash_log="$1";           shift

    register PreToolUse ExitPlanMode "$plan_review" 300
    register Stop "" "$wrapup_stop" ""
    register SessionStart "" "$wrapup_session_start" ""
    # plan-view は plan-review gate と同じ matcher に、別エントリとして並ぶ。
    # Claude Code は同一 matcher の hook を並列に走らせるので、review の結果を
    # 待たずに窓が開く（= 表示は gate から独立している）。timeout は短く: この
    # hook は pandoc とプロセス fork しかせず、ブラウザの終了は待たない。
    register PreToolUse ExitPlanMode "$plan_view" 15
    # plan-scope-gate も同じ matcher に 3 つ目のエントリとして並ぶ。gh api graphql
    # 1 往復(+ フォールバック時は issue view 1 回)だけなので timeout は短め。
    register PreToolUse ExitPlanMode "$plan_scope_gate" 20
    # plan-precedent-gate(ADR-0012)も同じ matcher に 4 つ目のエントリとして並ぶ。
    # gh/ネットワークを一切呼ばず jq とテキスト処理だけなので timeout は最短。
    register PreToolUse ExitPlanMode "$plan_precedent_gate" 10
    # plan-fresh-gate も同じ matcher に 5 つ目のエントリとして並ぶ。fetch 1 回
    # (timeout 15s)+ diff/grep/awk のみで gh は呼ばない。
    register PreToolUse ExitPlanMode "$plan_fresh_gate" 30
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
    # worktree-fresh-base: pristine な worktree だけを origin/<base> へ黙って
    # fast-forward する。pr-gate の SessionStart advisory(base 追従)より先に
    # 列挙しているが、Claude Code は同一イベントの hook を並列実行するため
    # 逐次を保証しない — レースは許容し、動かした場合だけこの hook 自身が
    # additionalContext で報告する(docs/claude/worktree-fresh-base.md)。
    register SessionStart "startup|resume" "$worktree_fresh_base" 30
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
    # bleep(旧 publish-guard、上流分離、ADR-0009): 会社/private リポジトリの
    # 実名が git push・gh pr/issue の create/edit/comment・MCP tool call 経由で
    # PUBLIC な面に漏れるのを防ぐ(docs/claude/public-publish-guard.md)。
    # matcher は複合1本 "Bash|mcp__.*" — Bash と MCP を2つの hook エントリに
    # 分けない。register() の存在判定は command 文字列の完全一致だけで
    # matcher を見ないため、同一 command を2つの matcher で登録しようとすると
    # 2回目が早期 return し、MCP 経路が無検査のまま残ってしまう(旧実装が
    # matcher "Bash" 単体だったために持っていた最大の機能欠陥そのもの)。
    # if は付けない(Bash(*) のような permission rule 構文は MCP の tool 名
    # には一致しないため) — git-stash-guard と同じ理由(deny/ask 側は
    # matcher/if 不一致 = 素通りが事故になる)で、絞り込みは hook 内部の
    # 早期 exit に置く。gh api の生呼び出しの往復も想定してタイムアウトは
    # やや長め。
    register PreToolUse "Bash|mcp__.*" "$bleep_claude" 20
    # attribution-guard: bleep と同じ外向き投稿面(gh pr/issue の
    # create/edit/comment・gh pr review・MCP tool call)を見るが、判定の向きが
    # 逆 — あちらは「社名が現れる」ことの検出、こちらは「attribution が無い」
    # ことの検出。matcher も同じ複合 1 本にする(上と同じ理由: Bash 単体だと
    # MCP 経路が無検査のまま残る)。gh api の往復は無いのでタイムアウトは短い。
    register PreToolUse "Bash|mcp__.*" "$attribution_guard" 10
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
    # Direct worktree creation can outlive an agent's temporary checkout cleanup.
    # Herdr owns both lifecycle ends, so reject every Bash spelling here.
    register PreToolUse Bash "$worktree_create_guard" 10
    # The timer is the primary detector; SessionStart also exposes pending state
    # directly to the agent that is in a position to clean it up.
    register SessionStart "startup|resume" "$worktree_audit_context" 30
    # agent-turn-log(docs/adr/0011): 純粋なロガーで判定を持たないため matcher/if
    # は不要。UserPromptSubmit/Stop の両方に同一 command で登録し、スクリプト側が
    # `.hook_event_name` で分岐する(herdr-claude-metadata.sh と同じ形)。
    register UserPromptSubmit "" "$agent_turn_log" 10
    register Stop "" "$agent_turn_log" 10
    # atuin hook claude-code(docs/adr/0011): atuin 自身が提供するエージェント
    # フックを、Bash tool の PreToolUse/PostToolUse/PostToolUseFailure 3 イベント
    # すべてに同一 command で登録する(成功/失敗どちらの exit code も history.db
    # に残すため、PostToolUse だけでは足りない)。matcher は Claude Code 標準の
    # tool 名一致("Bash")であり、他 hook が使う permission-rule 構文の `if`
    # ではない。
    register PreToolUse Bash "$atuin_hook_claude_code" 10
    register PostToolUse Bash "$atuin_hook_claude_code" 10
    register PostToolUseFailure Bash "$atuin_hook_claude_code" 10
    # stack-base-guard(ADR-0027): bleep/attribution-guard と同じ
    # 複合 matcher "Bash|mcp__.*" に並ぶ(Bash 単体だと MCP 接続の瞬間に
    # 無検査になる、同じ理由の繰り返し)。gh pr list 1 往復 + ローカル git
    # 走査のみなので timeout は attribution-guard 並みでよいが、往復を含む
    # ため若干長めに確保する。
    register PreToolUse "Bash|mcp__.*" "$stack_base_guard" 20
    # pr-title-guard(ADR-0031): attribution-guard/stack-base-guard と同じ
    # 複合 matcher。判定は tarotene owner のローカル解決だけで gh API 往復を
    # 持たないため timeout は短め。
    register PreToolUse "Bash|mcp__.*" "$pr_title_guard" 10
    # pr-confirm-guard: pr-title-guard と同じ複合 matcher。owner スコープを
    # 持たず全リポジトリで発火する。本文の task list 検査・節切り出し・
    # 項目分割は jq/gh API を使わない純粋な文字列処理なので timeout は
    # 同じ短さでよい。
    register PreToolUse "Bash|mcp__.*" "$pr_confirm_guard" 10
    # decision-colocation-guard(ADR-396): attribution-guard/stack-base-guard/
    # pr-title-guard と同じ複合 matcher。scripts/decision-colocation-check
    # 1 往復(git diff + ファイル読み取りのみ、gh API 往復は持たない)なので
    # timeout は stack-base-guard 並みでよい。
    register PreToolUse "Bash|mcp__.*" "$decision_colocation_guard" 20
    # external-send-guard(docs/claude/external-send-guard.md): Gmail/Slack
    # の送信系 MCP tool だけが対象なので matcher は "mcp__.*" のみでよい
    # (bleep/attribution-guard と違い Bash 経由の送信は原理的に検出できない
    # ため、Bash|mcp__.* にする理由がない)。
    # jq/文字列処理のみで往復が無いので timeout は最短。
    register PreToolUse "mcp__.*" "$external_send_guard" 10
    # adr-number(ADR-380): `gh pr create` 直後に ADR-478 を PR 番号へ自動
    # 改番する段3(利便性層)。deny は一切しないため他の "Bash|mcp__.*" 系
    # guard と揃える必要が無く、atuin と同じ単純な Bash matcher でよい。
    # docs/adr/0000-*.md が無ければ stdin すら読まず即 exit するので常時
    # コストはほぼゼロ — timeout は atuin と同じ短さでよい。
    register PostToolUse Bash "$adr_number" 10
    # gh-edit-allow(#392): 1 バイナリで記録役と判定役を兼ね、hook_event_name で
    # 分岐する(herdr-claude-metadata と同じ「同一 command を複数イベントに」形)。
    # PostToolUse は `git push && gh pr create …` のような複合コマンドの成功も
    # 拾うため if を付けない(Rust 製で起動 ~1ms、非該当は即 exit)。PreToolUse は
    # allow しか返さない(不一致は素通し)ので、if で gh 呼び出しに絞ってよい
    # — git-worktree-allow と同じ理由付け。
    register PostToolUse Bash "$gh_edit_allow" 10
    register PreToolUse Bash "$gh_edit_allow" 10 "Bash(gh *)"
    # rulesets-write-guard(ADR-0000-rulesets-declaration-in-repo D7): deny
    # のみ返す(判定しない入力は素通し)ので、gh-edit-allow の PreToolUse と
    # 同じ if で gh 呼び出しに絞ってよい。
    register PreToolUse Bash "$rulesets_write_guard" 10 "Bash(gh *)"
    # routines-write-guard(28番、ADR-0000-routines-declaration-in-repo D7):
    # RemoteTrigger は Bash ではないため、Bash 専用の permission-rule 構文
    # (`Bash(gh *)` のような if narrowing)は使えない — matcher で
    # RemoteTrigger 呼び出し全体を hook に渡し、対象外のアクション/body は
    # バイナリ自身が None を返して素通しする(external-send-guard の
    # "mcp__.*" と同じ「広い matcher + バイナリ内部判定」形)。
    register PreToolUse RemoteTrigger "$routines_write_guard" 10
    # pkexec-guard(29番、docs/claude/pkexec-guard.md): deny のみを返す
    # (判定しない入力は素通し)が、git-stash-guard/bleep と同じ理由で if を
    # 付けない — 対象の "pkexec"/"sudo" トークンはコマンド文字列のどこにでも
    # 現れうるため、"Bash(pkexec *)" のような先頭一致の if では絞れず、
    # 絞り込みは常にバイナリ内部の早期 exit に置く。Rust 製で起動が速いため
    # timeout は atuin/gh-edit-allow 並みの短さでよい。
    register PreToolUse Bash "$pkexec_guard" 10
    # new-tool-guard(30番、docs/adr/543-existing-means-and-deterministic-
    # promotion.md D1): deny のみ返す(判定しない入力は素通し)。matcher を
    # "Write" に絞る — 対象イベントは常に PreToolUse(Write) で、バイナリ
    # 内部でさらに「新規ファイルか」「新しい道具・単位の述語に合致するか」
    # を判定する。Rust 製で起動が速いため timeout は他の Rust hook 並み。
    register PreToolUse Write "$new_tool_guard" 10
    # feedback-target-guard(ADR-543 段3): gh-edit-allow/rulesets-write-guard
    # と同じ理由で "Bash(gh *)" に絞る(コマンド文字列が "gh " で始まる場合
    # のみ発火。複合コマンドの2番目以降に gh が来るケースは対象外 — 既存の
    # gh-edit-allow/rulesets-write-guard と同じ既知のトレードオフ)。
    register PreToolUse Bash "$feedback_target_guard" 10 "Bash(gh *)"
    # repo-create-guard(ADR-0013 Amendment 2026-09-29): stack-base-guard/
    # decision-colocation-guard と同じ複合 matcher "Bash|mcp__.*" に並ぶ
    # (gh-edit-allow/rulesets-write-guard/feedback-target-guard の
    # "Bash(gh *)" if 絞り込みと違い、`git push && gh repo create …` の
    # ような複合コマンドの2番目以降に対象コマンドが来るケースも拾う必要が
    # あるため、この if 絞り込みは使わない)。判定は文字列処理のみで gh API
    # 往復を持たないため timeout は pr-title-guard 並みでよい。
    register PreToolUse "Bash|mcp__.*" "$repo_create_guard" 15
    # cmd-hash-log(ADR-543 段3): 判定を返さないので matcher を絞らず全
    # Bash 実行を対象にする(記録漏れが検出の精度を下げるため)。Rust 製で
    # 起動が速く、成功しても失敗しても即 exit するので timeout は最短。
    register PostToolUse Bash "$cmd_hash_log" 10
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

  # settings.json の permissions.allow を要素単位で冪等に同期する。
  # 使い方: register-claude-permissions <settings> --retire <r>… --allow <a>…
  #   --retire 以降のルールは allow から削除(無ければ何もしない)、--allow 以降は
  #   追加(既に同一文字列があれば何もしない)。ルール文字列を後から書き換えるときは
  #   旧文字列を retiredPermissionRules に移す — これで全ホストが次回の switch で旧
  #   ルールを掃除する(かつては「旧ルールが残り続ける」が既知の制約だった —
  #   docs/claude/claude-permissions.md)。permissions.defaultMode や allow 以外の
  #   キーには一切触らない。
  # ルール1件ごとに mktemp+jq+mv の read-modify-write サイクルを回すと(かつては
  # retiredPermissionRules + permissionRules で最大 53 回)、herdr-agent-state.sh や
  # Claude Code CLI 自体との並行書き込みに対する lost-update 窓がルール数分だけ
  # 反復される(#61)。retire/allow の全ルールを 1 回の jq 呼び出しにまとめ、
  # mktemp+mv も 1 回に減らして窓の反復回数を減らす。
  # pkexec-guard(29番、docs/claude/pkexec-guard.md)向けに `--retire-ask`/
  # `--ask` の2セクションを追加した。`.permissions.ask` は明示 ask ルールで、
  # auto mode の分類器より先に評価され常に確認を強制する(公式ドキュメント、
  # docs/claude/pkexec-guard.md 参照)。allow と違い「常に許可」を意味しない
  # ため、#461 の中間ワイルドカード strip(検出のみで表現不可能にできない
  # 理由はそちらのコメント参照)は ask には適用しない — 末尾 `*` のみの
  # 単純なルールしか今のところ書いていない。
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
    retire_args=()
    allow_args=()
    retire_ask_args=()
    ask_args=()
    for arg in "$@"; do
      case "$arg" in
        --retire|--allow|--retire-ask|--ask) mode="$arg"; continue ;;
      esac
      case "$mode" in
        --retire) retire_args+=("$arg") ;;
        --allow) allow_args+=("$arg") ;;
        --retire-ask) retire_ask_args+=("$arg") ;;
        --ask) ask_args+=("$arg") ;;
        *)
          echo "register-claude-permissions: --retire/--allow/--retire-ask/--ask より前にルールが来た: $arg" >&2
          exit 1
          ;;
      esac
    done

    retire_json="$("$jq" -n --args '$ARGS.positional' "''${retire_args[@]}")"
    allow_json="$("$jq" -n --args '$ARGS.positional' "''${allow_args[@]}")"
    retire_ask_json="$("$jq" -n --args '$ARGS.positional' "''${retire_ask_args[@]}")"
    ask_json="$("$jq" -n --args '$ARGS.positional' "''${ask_args[@]}")"

    # #461: 中間 `*` を含む allow rule を settings.json の実体からも一律 strip
    # する(宣言側は permissionRules の nix eval 時 assert で表現不可能にして
    # あるが、settings.json は Claude Code 自身の「常に許可」プロンプトや
    # /promote-permissions からも書かれる実行時の可変状態なので、宣言側の
    # 防御だけでは届かない — こちらは検出のみが上限)。正規表現は
    # `hasMidWildcard`(claude.nix)と同じ意図: `(` か空白の直後の `*` に、
    # 空白を挟んで `)` 以外の文字が続くパターン。
    mid_wildcard_re='(\(| )\*[[:space:]]+[^)]'

    tmp="$(mktemp)"
    "$jq" --argjson retire "$retire_json" --argjson allow "$allow_json" \
      --argjson retireAsk "$retire_ask_json" --argjson ask "$ask_json" \
      --arg midWildcardRe "$mid_wildcard_re" '
      .permissions.allow = (
        (((.permissions.allow // []) - $retire)
          | map(select(test($midWildcardRe) | not))) as $kept
        | $kept + ($allow | map(select(. as $r | ($kept | index($r)) | not)))
      )
      | .permissions.ask = (
        ((.permissions.ask // []) - $retireAsk) as $keptAsk
        | $keptAsk + ($ask | map(select(. as $r | ($keptAsk | index($r)) | not)))
      )
    ' "$settings" > "$tmp"
    mv "$tmp" "$settings"
  '';

  # Claude Code の permission rule 構文は `Tool(specifier)`(裸のコマンド文字列では
  # 認識されない)。Add / Commit / Create PR で毎回止まる直接原因はこの 4 件。
  # 破壊的操作は増やさない — 読み取り・検査系のみ追加する。gh の書き込み系
  # (pr edit / issue create / issue edit)はワイルドカードでは入れない —
  # `Bash(gh pr edit *)` は他人の PR の編集まで許してしまうため。代わりに
  # gh-edit-allow hook(#392、docs/claude/gh-edit-allow.md)が「このセッションが
  # 作成した PR/Issue」だけを検証付きで allow する。
  #
  # #461: `(` か空白の直後の `*` に、空白を挟んで `)` 以外の文字が続くパターン
  # ("Bash(git -C * add *)" のような中間ワイルドカード)を検出する。Claude Code
  # の Wildcard patterns(https://code.claude.com/docs/en/permissions、取得
  # 2026-09-25)は「`*` はルール中どこにでも置け、`Bash(git * main)` は
  # `git merge main` にも `git -c core.fsmonitor=<script> diff main` にも
  # マッチする」と明記している — 中間 `*` は実際に危険側でマッチする(以前この
  # ファイルに書かれていた「実際にはマッチしない」という理解は誤りだった)。
  # `-C`/`-p`/`--prefix` のような値受け取り位置への任意オプション挿入を素通しし、
  # #453/#460/#461 で3回、個別撤回を繰り返すことになった。個別撤回の代わりに、
  # 宣言する側は `permissionRules_` の下の assert で nix eval 時に拒否し
  # (表現不可能)、settings.json という実行時の可変状態は registerPermissions
  # の jq が一律 strip する(検出のみが上限、上のコメント参照)。
  hasMidWildcard = rule: builtins.match ".*(\\(| )\\*[[:space:]]+[^)]+.*" rule != null;

  permissionRules_ = [
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
    "Bash(git shelve *)"
    "Bash(git shelve)"
    "Bash(git unshelve)"

    "Bash(uv run pytest *)"
    "Bash(uv run ruff *)"
    "Bash(uv run mypy *)"
    "Bash(uv run pre-commit run *)"
    "Bash(uv run docs-check *)"
    "Bash(./scripts/check-*.sh *)"

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

  # #461: 上の `hasMidWildcard` で中間ワイルドカードを含むルールが無いことを
  # nix eval 時(`nix flake check`/`nix build`)に確認してから実際に使う値を返す。
  # `permissionRules_`(素の配列)という別バインディングを経由するのは、
  # `permissionRules = assert (中略 permissionRules …); …` のように自分自身を
  # 参照する assert を書くと Nix の遅延評価が自己再帰(infinite recursion)に
  # なるため。
  permissionRules =
    assert lib.assertMsg (lib.all (r: !hasMidWildcard r) permissionRules_)
      "home/modules/claude.nix permissionRules に中間 `*` を含むルールがあります: ${lib.concatStringsSep ", " (lib.filter hasMidWildcard permissionRules_)} — 値受け取り位置への任意オプション挿入を素通しするため書けません(#461)。個別ルールに絞るか、検証付きの allow hook を追加してください。";
    permissionRules_;

  # pkexec-guard(29番)向けの明示 ask ルール。`.permissions.allow` と違い
  # 「常に許可」ではなく「auto mode でも必ず確認を出す」ことが目的
  # (公式ドキュメント "auto mode config" — 明示 ask ルールは分類器より先に
  # 評価される、docs/claude/pkexec-guard.md 参照)。guard 自身が通すのは
  # `/usr/bin/pkexec <許可リスト> ...` の形だけなので、ここも同じ絶対パスで
  # 揃える(裸の `pkexec` はこのルールに一致せず、guard が deny する)。
  askRules = [ "Bash(/usr/bin/pkexec *)" ];
  retiredAskRules = [ ];

  # かつて配ったが撤回したルール。activation が全ホストの settings.json から削除する。
  # `Bash(git -C * add *)` 等の中間ワイルドカードを含む撤回は、もう個別に
  # ここへ列挙する必要が無い(#461) — registerPermissions の jq が
  # settings.json 上の中間ワイルドカードルールを出自(宣言/promote/実行時
  # プロンプトのどれか)を問わず一律 strip するようになった。このリストに
  # 残す/追加するのは、中間ワイルドカード**以外**の理由で退役したルールだけ。
  #
  # `list-branch-inventory.sh` / `sweep-removed-vendor-symbols.sh` は使い捨ての
  # ワンオフ作業用ルールが陳腐化して残っていたもの(#101)。~/.ghr 配下の全ローカル
  # リポジトリおよび tarotene 名義の全 GitHub リポジトリを検索したが、該当スクリプトは
  # このファイル自身の permissionRules 記述以外に実体が存在しないことを確認済み。
  #
  # `mcp__brave-search__*` / `mcp__github__*` は、対応する MCP サーバーが user scope
  # から消えたことで宛先を失ったルール。brave-search は API キーが失効したまま
  # サーバー自体が撤去され(#299)、github MCP は運用が gh CLI 側に寄った
  # (グローバル CLAUDE.md の gh 前提)結果として
  # home/modules/claude-mcp-servers.nix の reconcile 対象外になった。存在しない
  # ツール名への allow は効果を持たないが、実態と乖離した宣言が残ると次に読む人が
  # 「このサーバーは生きている」と誤読する。
  # `mcp__plugin_context7_context7__*` は plugin 由来で現役なので触らない。
  retiredPermissionRules = [
    "Bash(./scripts/list-branch-inventory.sh *)"
    "Bash(./scripts/sweep-removed-vendor-symbols.sh *)"

    "mcp__brave-search__brave_web_search"
    "mcp__github__issue_write"
    "mcp__github__issue_read"
    "mcp__github__pull_request_read"
    "mcp__github__update_pull_request"
    "mcp__github__list_issues"
    "mcp__github__create_pull_request"
    "mcp__github__search_issues"
  ];
in
{
  # plan-review gate の deny 対象 severity とラウンド上限をこの環境向けに再校正する。
  # 既定(BLOCKER,MAJOR / 3 ラウンド)は実測 deny 率 69%、3 ラウンド到達が中位という
  # 結果で、review の価値より摩擦が勝っていた。MAJOR は backlog へ落として報告のみに
  # し、ラウンドも 2 に絞る。gate 本体(copilot-plan-review.sh)は触らない — closer
  # ラウンドが judge() に空文字を渡して「gate 適格 severity なし」を表現する不変条件
  # (`${3-$GATE_SEVERITIES}` のコロンなしデフォルト)に影響しないよう、値は env 経由
  # でのみ渡す。sessionVariables は次回ログインから効く。詳細は
  # docs/claude/copilot-plan-review.md の環境変数節。
  #
  # `DISABLE_AUTOUPDATER`(claude 本体 = native installer が正、ADR-457)は
  # background check だけを止め、`claude update` 自体は動く(公式 docs 確認済み)。
  # settings.json の `env`(Claude Code 所有ファイルへの jq merge、
  # docs/claude/opusplan-model-aliases.md)ではなくここに置くのは、即時性が
  # 要らない(次回ログインから効けば足りる)値のために merge/撤回の手続きを
  # 増やさないため。更新は config/zsh/modules/53-tools-claude.zsh 経由の
  # `claude update` 1 本に絞る。
  home.sessionVariables = {
    COPILOT_PLAN_REVIEW_GATE_SEVERITIES = "BLOCKER";
    MAX_PLAN_REVIEWS = "2";
    DISABLE_AUTOUPDATER = "1";
  };

  home.file.".claude/hooks/copilot-plan-review.sh" = {
    source = repoConfig + "/claude/hooks/copilot-plan-review.sh";
    executable = true;
  };

  # critic の出力契約(文書兼 jq validator の参照用)。GitHub Copilot CLI には
  # Codex の `exec --output-schema` に相当する強制出力スキーマ機構が無いため、
  # 実際の検証は hook 内の CRITIC_SCHEMA_JQ が行う — この JSON はその契約を
  # 人間 / プロンプト向けに文書化したものである。配備先は hook と同じ
  # ~/.claude/hooks/ に並べて置く(hook が自身のディレクトリ相対で解決するため)
  # が、ソースツリー上は非 hook アセットとして config/claude/assets/ に分離
  # している(ADR-0007)。
  home.file.".claude/hooks/copilot-plan-review.schema.json".source =
    repoConfig + "/claude/assets/copilot-plan-review.schema.json";

  # plan-reviewer: copilot-plan-review.sh が `--agent plan-reviewer` で呼ぶ
  # read-only custom agent。tools は view/grep/glob だけで、
  # write/execute/web/GitHub MCP は与えない(docs/claude/copilot-plan-review.md)。
  home.file.".copilot/agents/plan-reviewer.agent.md".source =
    repoConfig + "/copilot/agents/plan-reviewer.agent.md";

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
  # スクリプトは CSS を自身のディレクトリ相対で解決するので、配備先では schema
  # と同じく 2 ファイルを ~/.claude/hooks/ に並べて置く。ソースツリー上は
  # config/claude/assets/ に分離しているが、plan-view.sh の find_css() が
  # SCRIPT_DIR/../assets/ も探すので、リポジトリ直接実行でも解決する(ADR-0007)。
  home.file.".claude/hooks/plan-view.sh" = {
    source = repoConfig + "/claude/hooks/plan-view.sh";
    executable = true;
  };
  home.file.".claude/hooks/plan-view.css".source = repoConfig + "/claude/assets/plan-view.css";

  # agent-turn-log: UserPromptSubmit / Stop の1ターン境界を JSONL 追記する
  # 純粋なロガー(ゲートではない)。出力契約は docs/adr/0011。
  home.file.".claude/hooks/agent-turn-log.sh" = {
    source = repoConfig + "/claude/hooks/agent-turn-log.sh";
    executable = true;
  };

  # plan-scope-gate: 要求インベントリ(scope-inventory、15番)の脱落を機械検査する。
  # plan-review / plan-view と同じ matcher に 3 つ目のエントリとして並ぶ。
  home.file.".claude/hooks/plan-scope-gate.sh" = {
    source = repoConfig + "/claude/hooks/plan-scope-gate.sh";
    executable = true;
  };

  # plan-precedent-gate: 先行例との対比(precedent-grounding、16番、ADR-0012)の
  # 脱落を機械検査する。同じ matcher に 4 つ目のエントリとして並ぶ。
  home.file.".claude/hooks/plan-precedent-gate.sh" = {
    source = repoConfig + "/claude/hooks/plan-precedent-gate.sh";
    executable = true;
  };

  # plan-fresh-gate: ExitPlanMode 直前に origin/<base> の進行を検出し、プラン
  # 参照ファイルと交差するときだけ deny する(docs/claude/plan-fresh-gate.md)。
  # 同じ matcher に 5 つ目のエントリとして並ぶ。
  home.file.".claude/hooks/plan-fresh-gate.sh" = {
    source = repoConfig + "/claude/hooks/plan-fresh-gate.sh";
    executable = true;
  };

  # attribution-guard: Claude が GitHub に書く外向きテキストに attribution
  # フッターを強制する(docs/claude/attribution-guard.md)。判定は純関数群に
  # 切り出してあり --selftest がネットワーク無しに 26 ケースを検査する。
  home.file.".claude/hooks/attribution-guard.sh" = {
    source = repoConfig + "/claude/hooks/attribution-guard.sh";
    executable = true;
  };
  # stack-base-guard(ADR-0027): セッション内の複数 PR を常時単一チェーンに
  # 積むことを作成時に機械強制する(docs/claude/stack-base-guard.md)。
  # attribution-guard.sh を同ディレクトリから source するので、配置は
  # 必ず ~/.claude/hooks/ 直下(上の attribution-guard.sh と同じ階層)。
  home.file.".claude/hooks/stack-base-guard.sh" = {
    source = repoConfig + "/claude/hooks/stack-base-guard.sh";
    executable = true;
  };
  # feedback-target-guard(ADR-543 段3): 同じ理由で attribution-guard.sh と
  # 同じ階層(~/.claude/hooks/ 直下)に配置する。
  home.file.".claude/hooks/feedback-target-guard.sh" = {
    source = repoConfig + "/claude/hooks/feedback-target-guard.sh";
    executable = true;
  };
  # repo-create-guard(ADR-0013 Amendment 2026-09-29): 同じ理由で
  # attribution-guard.sh と同じ階層(~/.claude/hooks/ 直下)に配置する。
  home.file.".claude/hooks/repo-create-guard.sh" = {
    source = repoConfig + "/claude/hooks/repo-create-guard.sh";
    executable = true;
  };
  # external-send-guard(docs/claude/external-send-guard.md): 外部宛メールの
  # 直接送信を deny し create_draft へ誘導する。独立ファイルで他 hook を
  # source しない。
  home.file.".claude/hooks/external-send-guard.sh" = {
    source = repoConfig + "/claude/hooks/external-send-guard.sh";
    executable = true;
  };
  # adr-number(ADR-380, docs/claude/adr-numbering.md): PostToolUse で
  # ADR-478 を PR 番号へ自動改番する段3(利便性層)。attribution-guard.sh
  # を同ディレクトリから source するので、配置は必ず ~/.claude/hooks/ 直下。
  home.file.".claude/hooks/adr-number.sh" = {
    source = repoConfig + "/claude/hooks/adr-number.sh";
    executable = true;
  };
  # gh-edit-allow(#392): crates/gh-edit-allow のビルド成果物(pkgs.dotfiles-tools、
  # flake.nix の rustOverlay)への安定パスの symlink。
  home.file.".claude/hooks/gh-edit-allow".source = "${pkgs.dotfiles-tools}/bin/gh-edit-allow";
  # rulesets-write-guard(ADR-0000-rulesets-declaration-in-repo D7): crates/
  # rulesets-write-guard のビルド成果物への安定パスの symlink(gh-edit-allow
  # と同じ理由付け)。
  home.file.".claude/hooks/rulesets-write-guard".source =
    "${pkgs.dotfiles-tools}/bin/rulesets-write-guard";
  # routines-write-guard(28番、ADR-0000-routines-declaration-in-repo D7):
  # crates/routines-write-guard のビルド成果物への安定パスの symlink
  # (gh-edit-allow と同じ理由付け)。
  home.file.".claude/hooks/routines-write-guard".source =
    "${pkgs.dotfiles-tools}/bin/routines-write-guard";
  # pkexec-guard(29番、docs/claude/pkexec-guard.md): crates/pkexec-guard の
  # ビルド成果物への安定パスの symlink(gh-edit-allow と同じ理由付け)。
  # Codex/Copilot もこの同じ store path を `--agent codex|copilot` 付きで
  # 呼ぶ(per-agent の別コピーは持たない、pkexecGuardCodexCmd/
  # pkexecGuardCopilotCmd 参照)。
  home.file.".claude/hooks/pkexec-guard".source = "${pkgs.dotfiles-tools}/bin/pkexec-guard";
  # new-tool-guard(30番、docs/adr/543-existing-means-and-deterministic-
  # promotion.md): crates/new-tool-guard のビルド成果物への安定パスの
  # symlink(gh-edit-allow と同じ理由付け)。`register` サブコマンドも
  # このパスから手動 Bash 実行される(deny メッセージが案内する)。
  home.file.".claude/hooks/new-tool-guard".source = "${pkgs.dotfiles-tools}/bin/new-tool-guard";
  # cmd-hash-log(ADR-543 段3、crates/cmd-hash-log): gh-edit-allow と同じ
  # 理由付けの安定パス symlink。
  home.file.".claude/hooks/cmd-hash-log".source = "${pkgs.dotfiles-tools}/bin/cmd-hash-log";
  # verdict-escalate(ADR-478、crates/verdict-escalate): 判定を返す hook では
  # ないので register には乗せない — wrapup-stop-gate.sh が同じディレクトリから
  # 絶対パスで見つけて逐次呼ぶ(gh-edit-allow と同じ配置、PreToolUse/PostToolUse
  # の register とは別の消費経路)。
  home.file.".claude/hooks/verdict-escalate".source = "${pkgs.dotfiles-tools}/bin/verdict-escalate";
  # Codex CLI / Copilot CLI 版 adapter(#192)。判定エンジンは持たず、上の
  # .claude/hooks/attribution-guard.sh を `source` するだけの薄い層 —
  # source 先は $HOME/.claude/hooks 経由で解決される(#602)。
  home.file.".codex/hooks/attribution-guard.sh" = {
    source = repoConfig + "/codex/hooks/attribution-guard.sh";
    executable = true;
  };
  home.file.".copilot/hooks/attribution-guard.sh" = {
    source = repoConfig + "/copilot/hooks/attribution-guard.sh";
    executable = true;
  };

  # pr-title-guard(ADR-0031): PR タイトルを commit-message 契約として
  # 作成時に機械強制する(docs/claude/pr-title-contract.md)。
  # attribution-guard.sh を同ディレクトリから source するので、配置は
  # 必ず ~/.claude/hooks/ 直下。
  home.file.".claude/hooks/pr-title-guard.sh" = {
    source = repoConfig + "/claude/hooks/pr-title-guard.sh";
    executable = true;
  };
  # Codex CLI / Copilot CLI 版 adapter(#192 の型を踏襲)。
  # ~/.claude/hooks/pr-title-guard.sh を $HOME 基準で辿る(#602)。
  home.file.".codex/hooks/pr-title-guard.sh" = {
    source = repoConfig + "/codex/hooks/pr-title-guard.sh";
    executable = true;
  };
  home.file.".copilot/hooks/pr-title-guard.sh" = {
    source = repoConfig + "/copilot/hooks/pr-title-guard.sh";
    executable = true;
  };

  # pr-confirm-guard(docs/claude/pr-confirm-guard.md): PR 本文に未チェック
  # の task list を残さない・`## 要確認` の各項目に Issue 参照を持たせる
  # ことを作成時に機械強制する。attribution-guard.sh を同ディレクトリから
  # source するので、配置は必ず ~/.claude/hooks/ 直下。
  home.file.".claude/hooks/pr-confirm-guard.sh" = {
    source = repoConfig + "/claude/hooks/pr-confirm-guard.sh";
    executable = true;
  };
  # Codex 版 adapter(pr-title-guard.sh と同じ型)。
  # ~/.claude/hooks/pr-confirm-guard.sh を $HOME 基準で辿る(#602)。
  # Copilot adapter は作らない(依頼は Codex のみ)。
  home.file.".codex/hooks/pr-confirm-guard.sh" = {
    source = repoConfig + "/codex/hooks/pr-confirm-guard.sh";
    executable = true;
  };

  # git-stash-guard(ADR-0032 Amendment #531): 素の `git stash` を弾く。
  # Codex 用の adapter ファイルは #416 で廃止し、Rust バイナリを --host codex
  # で直接登録する(codexGitStashGuardCmd)。Copilot 版は未展開(#161、
  # MCP tool 名の命名規則が未確認なため attribution-guard 系のみ先行)。

  # stack-base-guard(ADR-0027 Amendment #531): Codex CLI 版 adapter。
  home.file.".codex/hooks/stack-base-guard.sh" = {
    source = repoConfig + "/codex/hooks/stack-base-guard.sh";
    executable = true;
  };

  # adr-number(ADR-380 Amendment #531): Codex CLI 版 adapter。
  home.file.".codex/hooks/adr-number.sh" = {
    source = repoConfig + "/codex/hooks/adr-number.sh";
    executable = true;
  };

  # codex-plan-gate(ADR-0032 Amendment #531、docs/claude/codex-plan-gate.md):
  # 他 hook を source しない独立ファイル。
  home.file.".codex/hooks/codex-plan-gate.sh" = {
    source = repoConfig + "/codex/hooks/codex-plan-gate.sh";
    executable = true;
  };

  # decision-colocation-guard(ADR-396): 決定成果物(ADR/設計文書/skill)の
  # 追加を執行点と同じ PR に機械強制する(docs/claude/decision-colocation.md)。
  # attribution-guard.sh を同ディレクトリから source するので、配置は
  # 必ず ~/.claude/hooks/ 直下。Codex/Copilot adapter は意図的に作らない
  # (CI required check が全エージェント共通の backstop になるため)。
  home.file.".claude/hooks/decision-colocation-guard.sh" = {
    source = repoConfig + "/claude/hooks/decision-colocation-guard.sh";
    executable = true;
  };

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
    source = repoConfig + "/claude/statusline/claude-statusline.sh";
    executable = true;
  };

  # claude-usage: herdr の tab_bar_right command が interval 実行する(Claude Code
  # hook ではない — settings.json には登録しない)。呼び出し側は
  # config/herdr/config.toml。詳細は docs/claude/claude-usage.md。
  # ソースツリー上は config/claude/statusline/ に分離しているが(ADR-0007)、
  # 配備先は herdr のハードコード実行パスに合わせて引き続き ~/.claude/hooks/。
  home.file.".claude/hooks/claude-usage.sh" = {
    source = repoConfig + "/claude/statusline/claude-usage.sh";
    executable = true;
  };

  # worktree-fresh-base: pristine な worktree だけを origin/<base> へ黙って
  # fast-forward する SessionStart hook。
  home.file.".claude/hooks/worktree-fresh-base".source =
    "${pkgs.dotfiles-tools}/bin/worktree-fresh-base";

  # pr-gate: PR completion barrier。判定対象は allowlist に列挙した nwo だけ
  # (既定は本リポジトリのみ)なので、他リポジトリでは完全沈黙する。
  home.file.".claude/hooks/pr-gate.sh" = {
    source = repoConfig + "/claude/hooks/pr-gate.sh";
    executable = true;
  };
  # git-worktree-allow: herdr worktree への `git -C` を検証つきで許可する PreToolUse hook。
  home.file.".claude/hooks/git-worktree-allow".source =
    "${pkgs.dotfiles-tools}/bin/git-worktree-allow";
  # git-stash-guard: 素の `git stash` を deny する PreToolUse hook。
  home.file.".claude/hooks/git-stash-guard".source = "${pkgs.dotfiles-tools}/bin/git-stash-guard";
  # bleep(旧 publish-guard): 会社/private リポジトリの実名が PUBLIC な面に
  # 漏れるのを防ぐ PreToolUse hook。上流を別リポジトリ tarotene/bleep に
  # 切り出し(ADR-0009)、flake input(pinned rev)からツリーごと配備する。
  # ツリー全体を1つの home.file で(個々のファイルを列挙せず)配ることで、
  # hooks/bleep.sh の自己解決(`realpath "$0"`)がそのまま成立する — 詳細は
  # docs/claude/public-publish-guard.md。
  home.file.".claude/hooks/bleep" = {
    source = bleep;
  };
  # bleep-hook(Rust、#561): flake.nix の bleepHookOverlay が同じ `bleep`
  # input から crane でビルドする。gh-edit-allow(1436行目付近)と同じ
  # 「store path を安定パスへ home.file で配備」型 — bleepClaudeCmd 等の
  # BLEEP_HOOK_BIN/BLEEP_LEX_BIN が指す先はこのパス。
  home.file.".claude/hooks/bleep-hook" = {
    source = "${pkgs.bleep-hook}/bin/bleep-hook";
  };

  # Codex/Copilot 版 shim の配線(#160)。Claude Code plugin 相当の配線
  # (上の settings.json マージ)はあったが、Codex CLI (~/.codex/hooks.json) /
  # Copilot CLI (~/.copilot/settings.json) には ADR-0009 決定7で明示的に
  # 対象外としたまま配線していなかった。register-codex-hooks /
  # register-copilot-hooks は worktree.nix / herdr.nix が同じ対象ファイルを
  # 書き換える registrar なので、lost-update 窓(#61 と同種)を避けるため
  # それらの後ろに明示的に順序付ける。#25-28(Rust hook cutover + bleep
  # 改名)で旧 adapters/{codex,copilot}-adapter.sh が upstream から削除された
  # ため、--retire で旧 command 文字列を先に取り除いてから新 bleep.sh の
  # command を登録する(register-{codex,copilot}-hooks に追加した --retire、
  # Claude 側の retiredHookEntries と同型 — 削除せず放置すると存在しない
  # パスを指したまま毎 tool call で失敗する)。
  home.activation.registerCodexBleepHooks =
    lib.hm.dag.entryAfter
      [
        "writeBoundary"
        "registerCodexWorktreeHooks"
        "registerCodexHerdrMetadataHooks"
      ]
      ''
        run ${registerCodexHooks} "$HOME/.codex/hooks.json" \
          --retire PreToolUse ${lib.escapeShellArg legacyPublishGuardCodexAdapterCmd} \
          --retire PreToolUse ${lib.escapeShellArg legacyBleepCodexCmdUnprefixed} \
          --register \
          PreToolUse ${lib.escapeShellArg "Bash|mcp__.*"} ${lib.escapeShellArg bleepCodexCmd} 20
      '';

  home.activation.registerCopilotBleepHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCopilotHerdrMetadataHooks" ]
      ''
        run ${registerCopilotHooks} "$HOME/.copilot/settings.json" \
          --retire preToolUse ${lib.escapeShellArg legacyPublishGuardCopilotAdapterCmd} \
          --retire preToolUse ${lib.escapeShellArg legacyBleepCopilotCmdUnprefixed} \
          --register \
          preToolUse ${lib.escapeShellArg bleepCopilotCmd} 20
      '';

  # attribution-guard の Codex/Copilot 展開(#192)。同じ lost-update 対策で
  # bleep の登録の後ろに明示的に順序付ける。
  home.activation.registerCodexAttributionGuardHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCodexBleepHooks" ]
      ''
        run ${registerCodexHooks} "$HOME/.codex/hooks.json" \
          PreToolUse ${lib.escapeShellArg "Bash|mcp__.*"} ${lib.escapeShellArg codexAttributionGuardCmd} 10
      '';

  home.activation.registerCopilotAttributionGuardHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCopilotBleepHooks" ]
      ''
        run ${registerCopilotHooks} "$HOME/.copilot/settings.json" \
          preToolUse ${lib.escapeShellArg copilotAttributionGuardCmd} 10
      '';

  # pr-title-guard の Codex/Copilot 展開(ADR-0031、#192 の型を踏襲)。同じ
  # lost-update 対策で attribution-guard の登録の後ろに明示的に順序付ける。
  home.activation.registerCodexPrTitleGuardHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCodexAttributionGuardHooks" ]
      ''
        run ${registerCodexHooks} "$HOME/.codex/hooks.json" \
          PreToolUse ${lib.escapeShellArg "Bash|mcp__.*"} ${lib.escapeShellArg codexPrTitleGuardCmd} 10
      '';

  home.activation.registerCopilotPrTitleGuardHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCopilotAttributionGuardHooks" ]
      ''
        run ${registerCopilotHooks} "$HOME/.copilot/settings.json" \
          preToolUse ${lib.escapeShellArg copilotPrTitleGuardCmd} 10
      '';

  # pkexec-guard(29番)の Codex/Copilot 展開: 判定ロジックは持たず、`pkexec`
  # を含む Bash コマンドを常に deny する(docs/claude/pkexec-guard.md「Codex /
  # Copilot からは pkexec を一切許さない」)。attribution-guard/pr-title-guard
  # と同じ lost-update 対策で、その後ろに明示的に順序付ける。
  home.activation.registerCodexPkexecGuardHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCodexPrTitleGuardHooks" ]
      ''
        run ${registerCodexHooks} "$HOME/.codex/hooks.json" \
          PreToolUse ${lib.escapeShellArg "Bash"} ${lib.escapeShellArg pkexecGuardCodexCmd} 10
      '';

  home.activation.registerCopilotPkexecGuardHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCopilotPrTitleGuardHooks" ]
      ''
        run ${registerCopilotHooks} "$HOME/.copilot/settings.json" \
          preToolUse ${lib.escapeShellArg pkexecGuardCopilotCmd} 10
      '';

  # ADR-0032 Amendment #531 段2: 入力が Claude と同形の PreToolUse/
  # PostToolUse adapter 群(git-stash-guard/stack-base-guard/adr-number)+
  # rulesets-write-guard(Rust、直接登録)を Codex CLI に展開する。同じ
  # lost-update 対策で(この段の時点で最後尾だった)pr-title-guard の登録
  # ではなく、後から main に入った pkexec-guard の登録の後ろに明示的に
  # 順序付ける(#534 のリベース追従時に発見 — 同じ registerCodexHooks 対象
  # ファイルを書く2系統を並行させないため)。
  home.activation.registerCodexStagedHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCodexPkexecGuardHooks" ]
      ''
        run ${registerCodexHooks} "$HOME/.codex/hooks.json" \
          --retire PreToolUse ${lib.escapeShellArg legacyCodexGitStashGuardCmd} \
          --register \
          PreToolUse ${lib.escapeShellArg "Bash"} ${lib.escapeShellArg codexGitStashGuardCmd} 10 \
          PreToolUse ${lib.escapeShellArg "Bash|mcp__.*"} ${lib.escapeShellArg codexStackBaseGuardCmd} 20 \
          PreToolUse ${lib.escapeShellArg "Bash"} ${lib.escapeShellArg codexRulesetsWriteGuardCmd} 10 \
          PostToolUse ${lib.escapeShellArg "Bash"} ${lib.escapeShellArg codexAdrNumberCmd} 10
      '';

  # ADR-0032 Amendment #531 段2: SessionStart 系は入力が cwd のみで
  # agent-agnostic なため、adapter を挟まず Claude 版スクリプトをそのまま
  # Codex にも直接登録する(herdr-codex-metadata.sh とは違い、別スクリプトを
  # 新設しない点に注意 — こちらは判定ロジックの単一正本を保つのが目的)。
  home.activation.registerCodexSessionStartHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCodexStagedHooks" ]
      ''
        run ${registerCodexHooks} "$HOME/.codex/hooks.json" \
          --retire SessionStart ${lib.escapeShellArg legacyWorktreeFreshBaseCmd} \
          --register \
          SessionStart ${lib.escapeShellArg "startup|resume|compact"} ${lib.escapeShellArg issueIndexCmd} 10 \
          SessionStart "" ${lib.escapeShellArg wrapupSessionStartCmd} 10 \
          SessionStart ${lib.escapeShellArg "startup|resume"} ${lib.escapeShellArg signPrewarmCmd} 120 \
          SessionStart "" ${lib.escapeShellArg prGateSessionStartCmd} 10 \
          SessionStart ${lib.escapeShellArg "startup|resume"} ${lib.escapeShellArg worktreeFreshBaseCmd} 30
      '';

  # ADR-0032 Amendment #531 段3: Stop/UserPromptSubmit hook。pr-gate.sh は
  # agent 名を参照しないため同じ command 文字列をそのまま(D8 参照)、
  # wrapup-stop-gate.sh/agent-turn-log.sh は環境変数だけ差し替えた command
  # 文字列を登録する — いずれも adapter ファイルは新設しない。同じ
  # lost-update 対策で SessionStart の登録の後ろに明示的に順序付ける。
  home.activation.registerCodexStopHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCodexSessionStartHooks" ]
      ''
        run ${registerCodexHooks} "$HOME/.codex/hooks.json" \
          Stop "" ${lib.escapeShellArg codexWrapupStopCmd} 10 \
          Stop "" ${lib.escapeShellArg codexPrGateStopCmd} 600 \
          UserPromptSubmit "" ${lib.escapeShellArg codexAgentTurnLogCmd} 10 \
          Stop "" ${lib.escapeShellArg codexAgentTurnLogCmd} 10
      '';

  # ADR-0032 Amendment #531 段4: codex-plan-gate(docs/claude/
  # codex-plan-gate.md)。同じ lost-update 対策で Stop hook 登録の後ろに
  # 明示的に順序付ける。plan-scope-gate.sh / plan-precedent-gate.sh 自体は
  # 呼び出すだけで Codex 用に配備しない(~/.claude/hooks/ 配下の既存パスを
  # そのまま参照する)。
  home.activation.registerCodexPlanGateHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCodexStopHooks" ]
      ''
        run ${registerCodexHooks} "$HOME/.codex/hooks.json" \
          Stop "" ${lib.escapeShellArg codexPlanGateCmd} 20
      '';

  # pr-confirm-guard の Codex 展開。同じ lost-update 対策で、この段の時点で
  # 最後尾だった registerCodexPlanGateHooks の後ろに明示的に順序付ける。
  home.activation.registerCodexPrConfirmGuardHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCodexPlanGateHooks" ]
      ''
        run ${registerCodexHooks} "$HOME/.codex/hooks.json" \
          PreToolUse ${lib.escapeShellArg "Bash|mcp__.*"} ${lib.escapeShellArg codexPrConfirmGuardCmd} 10
      '';

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

  # claude-plan-model: Plan 側モデルを Fable ⇄ Opus で切り替える(引数なし=トグル)。
  # hook ではないので ~/.claude/hooks/ ではなく ~/.local/bin に置く — git-shelve や
  # git-prune-branches と同じ「PATH で解決される実行可能ファイル」扱い(ADR-0007 に
  # 従い配備名から .sh を落とす)。activation はこの配備物ではなく store 上の同じ
  # ファイルを `sync` で呼ぶ。
  home.file.".local/bin/claude-plan-model" = {
    source = planModelScript;
    executable = true;
  };

  # コマンドファイルは @home@ プレースホルダを config.home.homeDirectory に
  # 展開する(ADR-0002: literal + 1 変数の replaceVars パターン、
  # desktop.nix の fcitx5.desktop と同型)。altair(darwin, /Users/tarotene)
  # のように homeDirectory がホストごとに異なる環境でも壊れない
  # (PR2, ADR-0018 対応)。
  home.file.".claude/commands/copilot-plan-review.md".source =
    pkgs.replaceVars (repoConfig + "/claude/commands/copilot-plan-review.md")
      {
        home = config.home.homeDirectory;
      };
  home.file.".claude/commands/plan-view.md".source = pkgs.replaceVars (
    repoConfig + "/claude/commands/plan-view.md"
  ) { home = config.home.homeDirectory; };

  # resolve-pr-threads / promote-permissions: 退役済みの私設 AI 設定同期
  # リポジトリ(PRIVATE)から使用実績で選別して収容した command 2 本
  # (128 回・7 回の実使用、いずれもローカル版が正本)。@home@ プレースホルダは
  # 使わないため replaceVars を挟まず literal のまま配る。同時に見つかった
  # weekly-backlog-review は別の PRIVATE リポジトリの運用モデルに構造的に
  # 依存するため収容していない(実機に手動管理のまま残す)。
  home.file.".claude/commands/resolve-pr-threads.md".source =
    repoConfig + "/claude/commands/resolve-pr-threads.md";
  home.file.".claude/commands/promote-permissions.md".source =
    repoConfig + "/claude/commands/promote-permissions.md";

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
  # test-grounding: 複数の実コンポーネントが絡む検証項目・試験手順を書く前に、
  # facts文書+層別モデルで一次資料に当たることを強制する個人スキル。
  home.file.".claude/skills/test-grounding/SKILL.md".source =
    repoConfig + "/claude/skills/test-grounding/SKILL.md";
  home.file.".claude/skills/test-grounding/cases.md".source =
    repoConfig + "/claude/skills/test-grounding/cases.md";
  # living-description: Issue/PR の Description を正本として、コメントで裁定が
  # 確定した時点で編集し続ける習慣。cases.md は追記型の失敗事例集。
  home.file.".claude/skills/living-description/SKILL.md".source =
    repoConfig + "/claude/skills/living-description/SKILL.md";
  home.file.".claude/skills/living-description/cases.md".source =
    repoConfig + "/claude/skills/living-description/cases.md";
  # issue-ref-freshness: 他リポジトリの issue/PR を Open 前提で参照している
  # 記述を、参照先の裁定(close・merge・not planned)を読んで書き直す手順。
  # living-description の外部参照版。詳細は docs/claude/issue-ref-freshness.md。
  home.file.".claude/skills/issue-ref-freshness/SKILL.md".source =
    repoConfig + "/claude/skills/issue-ref-freshness/SKILL.md";
  # pr-description: PR 本文の標準スケルトンと Before/After 視覚証跡の判断知識。
  # 指針は全リポジトリで有効、強制(内容ではなく証跡の有無)は pr-gate.sh の
  # G_visual(~/.claude/pr-gate-repos の allowlist 内のみ)が担う。cases.md は
  # 追記型の失敗事例集。
  home.file.".claude/skills/pr-description/SKILL.md".source =
    repoConfig + "/claude/skills/pr-description/SKILL.md";
  home.file.".claude/skills/pr-description/cases.md".source =
    repoConfig + "/claude/skills/pr-description/cases.md";
  # wrapup-chores: wrap-up inbox のうち判断を要さない軽微な項目を、未起票の inbox
  # 行と起票済みの wrapup 由来 Issue の両方からまとめて triage し、1 回の確認後に
  # 1 つの chores PR で一括対処する判断知識。hook 側(wrapup-stop-gate.sh)には
  # 手を入れず、inbox からの削除は既存の --mark-filed 経由のみを使う。
  home.file.".claude/skills/wrapup-chores/SKILL.md".source =
    repoConfig + "/claude/skills/wrapup-chores/SKILL.md";
  # copilot-model-bump: 外部 AI CLI(Copilot CLI 等)に固定 pin した具体モデル ID を、
  # ベンダー側の GA・廃止サイクルに追従して更新する手順の判断知識。pin 箇所の棚卸し
  # (既定値・selftest 期待値・docs)・上流確認・スラッグ実機確認・完了条件を定型化する
  # (copilot-plan-review.sh の gpt-5.6-sol → gpt-6-astra bump が初出時の実例)。
  home.file.".claude/skills/copilot-model-bump/SKILL.md".source =
    repoConfig + "/claude/skills/copilot-model-bump/SKILL.md";
  # issue-hygiene: open Issue を出自でクラスタリングし、GitHub ネイティブ sub-issues
  # で親子構造を明示、腐った tracking Issue を清算する定期衛生管理の判断知識。
  home.file.".claude/skills/issue-hygiene/SKILL.md".source =
    repoConfig + "/claude/skills/issue-hygiene/SKILL.md";
  # tracking-issue: 複数の子作業を束ねる親 Issue を書く/更新する側の書式規約。
  # issue-hygiene(事後の棚卸し・清算)とは役割が異なる。詳細は
  # docs/claude/tracking-issue.md。
  home.file.".claude/skills/tracking-issue/SKILL.md".source =
    repoConfig + "/claude/skills/tracking-issue/SKILL.md";
  # handoff: ユーザーの指示で作業を途中で打ち切るとき、残タスクを Human / AI
  # 双方に振り分けて起票し、後で再開できる状態にする判断知識(WIP は Draft PR、
  # 担当は閉語彙ラベル handoff:human/handoff:ai、順序は Issue dependencies)。
  # pr-gate.sh の中断ハンドオフ節・issue-index.sh の着手可能な handoff:ai 節と
  # 組で動く。詳細は docs/claude/handoff.md。
  home.file.".claude/skills/handoff/SKILL.md".source = repoConfig + "/claude/skills/handoff/SKILL.md";
  home.file.".claude/skills/handoff/scripts/handoff.sh" = {
    source = repoConfig + "/claude/skills/handoff/scripts/handoff.sh";
    executable = true;
  };
  # claude-routines: Claude Code routine を対象リポジトリ自身の
  # `.claude/routines/<name>.{json,md}` へ as-code 化する判断知識。ローカル
  # session 向けは SKILL.md、クラウド自己監査 routine 向けは auditor.md。
  # 差分コアは scripts/routines-plan.sh(bash + jq)。詳細は
  # docs/claude/claude-routines.md。
  home.file.".claude/skills/claude-routines/SKILL.md".source =
    repoConfig + "/claude/skills/claude-routines/SKILL.md";
  home.file.".claude/skills/claude-routines/auditor.md".source =
    repoConfig + "/claude/skills/claude-routines/auditor.md";
  home.file.".claude/skills/claude-routines/scripts/routines-plan.sh" = {
    source = repoConfig + "/claude/skills/claude-routines/scripts/routines-plan.sh";
    executable = true;
  };
  home.file.".claude/skills/claude-routines/scripts/selftest.sh" = {
    source = repoConfig + "/claude/skills/claude-routines/scripts/selftest.sh";
    executable = true;
  };
  home.file.".claude/skills/claude-routines/scripts/lib.jq".source =
    repoConfig + "/claude/skills/claude-routines/scripts/lib.jq";
  # stacked-pr: PR 同士に依存関係があるとき main 起点で並行させず base を親ブランチ
  # にした stacked PR として積む判断知識。判定条件・rebase.updateRefs による追従・
  # Issue リンクの書き分け・GitHub ネイティブ stack 機能の使い方を持つ。
  home.file.".claude/skills/stacked-pr/SKILL.md".source =
    repoConfig + "/claude/skills/stacked-pr/SKILL.md";
  # scope-inventory: Tracking Issue 等の複数項目の依頼を計画に起こすとき、子タスク
  # を黙って落とさせないための要求インベントリの作り方(gh graphql での sub-issues
  # 列挙、閉じた棄却タグ、Reference-Only: での参照 Issue の書き分け)。強制は
  # plan-scope-gate.sh(段2)が担う。詳細は docs/claude/scope-inventory.md。
  home.file.".claude/skills/scope-inventory/SKILL.md".source =
    repoConfig + "/claude/skills/scope-inventory/SKILL.md";
  # precedent-grounding: Plan に非自明な設計判断を書くとき、確立されたやり方
  # (先行例・文献)と照合した結果を `## 先行例との対比` 節として成果物に残す
  # 書き方(出典・取得日・差分の書式、免除行の条件)。批評(lens A)が何を監査
  # するかもここに持つ。形式検査は plan-precedent-gate.sh(段2)が担う。詳細は
  # docs/claude/precedent-grounding.md、コメント索引 16) 参照。
  home.file.".claude/skills/precedent-grounding/SKILL.md".source =
    repoConfig + "/claude/skills/precedent-grounding/SKILL.md";
  # selection-grounding: 技術・仕組みの選択(ツール・ライブラリ・hook/skill の
  # 要否・置き換え)を表現不可能性 → 還元性 → 先進性の3軸の辞書式順序で評価し、
  # precedent-grounding が確立した `## 先行例との対比` 節に `軸:` トークンと
  # (該当時)`本命:`/`対抗馬:`/`外した候補:` を追加する書き方(ADR-0035)。
  # 形式検査は plan-precedent-gate.sh(precedent-grounding と共有、新規 gate は
  # 作らない)。詳細は docs/claude/selection-grounding.md。
  home.file.".claude/skills/selection-grounding/SKILL.md".source =
    repoConfig + "/claude/skills/selection-grounding/SKILL.md";
  # github-audit-triage: github-audit の findings を入力に複数リポジトリの
  # 一括起草・一括レビュー・一括 PR 化を行う判断知識(ADR-0015 の LLM
  # ノード)。charter-sweep(#180)を巻き取り、完了定義を PR 作成までに
  # 変更している。詳細は docs/claude/github-audit-triage.md。
  home.file.".claude/skills/github-audit-triage/SKILL.md".source =
    repoConfig + "/claude/skills/github-audit-triage/SKILL.md";
  # repo-charter: 新規リポジトリ作成時に README/CONTRIBUTING の charter
  # スキーマ(purpose sentence / Scope / Issues 節 / naming class / topics)を
  # インタビュー形式で埋める判断知識(ADR-0013 + ADR-0016 + ADR-0017)。
  # cases.md は追記型の失敗事例集。
  home.file.".claude/skills/repo-charter/SKILL.md".source =
    repoConfig + "/claude/skills/repo-charter/SKILL.md";
  home.file.".claude/skills/repo-charter/cases.md".source =
    repoConfig + "/claude/skills/repo-charter/cases.md";
  # writing-style: 執筆規約の正本(別 private リポジトリの docs/style/)への
  # 薄いポインタ(#115)。scripts/writing-style-hub がマーカーファイル/環境
  # 変数からハブの絶対パスを解決する。
  home.file.".claude/skills/writing-style/SKILL.md".source =
    repoConfig + "/claude/skills/writing-style/SKILL.md";
  # gas-clasp-ops: Google Apps Script (GAS) を clasp CLI で操作する判断知識
  # (ADR-0030)。初回 GCP セットアップ・ログイン・日常操作・スクリプト側の
  # 規約を持つ。GAS コード自体の正本は各利用リポジトリに分散配置し、ここには
  # ツールのナレッジだけを置く。詳細は docs/claude/gas-clasp-ops.md。
  home.file.".claude/skills/gas-clasp-ops/SKILL.md".source =
    repoConfig + "/claude/skills/gas-clasp-ops/SKILL.md";
  # gpg-subkey-rotation: GPG の機体ローカル [S]/[E] サブ鍵ローテーションを
  # 8 ステップの完了条件付きで終わらせる判断知識(ADR-0003)。rotate だけ
  # 実行して export/nix 編集/GitHub-keyserver 同期/hms 適用のどれかを飛ばす
  # と commit 署名検証が静かに壊れる、という実際の失敗から起票。詳細は
  # docs/claude/gpg-subkey-rotation.md。
  home.file.".claude/skills/gpg-subkey-rotation/SKILL.md".source =
    repoConfig + "/claude/skills/gpg-subkey-rotation/SKILL.md";
  # external-call-scheduling: 電話等 Claude が代行できないハンドオフ作業を、
  # 相手の営業時間とユーザーの空き時間を突き合わせてカレンダーに反映する
  # 判断知識。詳細は docs/claude/external-call-scheduling.md。
  home.file.".claude/skills/external-call-scheduling/SKILL.md".source =
    repoConfig + "/claude/skills/external-call-scheduling/SKILL.md";
  # browser-login-handoff(#562): Playwright MCP・claude-in-chrome での
  # ブラウザ操作中に未ログインを検知したときのハンドオフ手順。両ツールとも
  # Claude Code 固有のため .agents/skills/ へはミラーしない(external-call-
  # scheduling と同じ理由)。詳細は docs/claude/browser-login-handoff.md。
  home.file.".claude/skills/browser-login-handoff/SKILL.md".source =
    repoConfig + "/claude/skills/browser-login-handoff/SKILL.md";
  # slot-availability: 候補日程一覧 × Google Calendar の3コマ当たり判定。
  # 詳細は docs/claude/slot-availability.md。
  home.file.".claude/skills/slot-availability/SKILL.md".source =
    repoConfig + "/claude/skills/slot-availability/SKILL.md";
  home.file.".claude/skills/slot-availability/scripts/slot-hit.sh" = {
    source = repoConfig + "/claude/skills/slot-availability/scripts/slot-hit.sh";
    executable = true;
  };
  home.file.".claude/skills/slot-availability/scripts/selftest.sh" = {
    source = repoConfig + "/claude/skills/slot-availability/scripts/selftest.sh";
    executable = true;
  };
  home.file.".claude/skills/slot-availability/scripts/lib.jq".source =
    repoConfig + "/claude/skills/slot-availability/scripts/lib.jq";
  home.file.".claude/skills/slot-availability/scripts/judge.jq".source =
    repoConfig + "/claude/skills/slot-availability/scripts/judge.jq";
  home.file.".claude/skills/slot-availability/scripts/finalize.jq".source =
    repoConfig + "/claude/skills/slot-availability/scripts/finalize.jq";
  # performance-planning: 演奏本番の練習計画提案・合わせ調整の統合・当日
  # タイムテーブル/遠征の Calendar dispatch。詳細は
  # docs/claude/performance-planning.md(別の private な person-state
  # リポジトリ側の ADR-0010)。
  home.file.".claude/skills/performance-planning/SKILL.md".source =
    repoConfig + "/claude/skills/performance-planning/SKILL.md";
  # venue-search: 合わせ・個人練習の会場探し(空き確認は機械、予約は人間)。
  # ハブは performance-planning の performance-hub を流用する。詳細は
  # docs/claude/venue-search.md(別の private な person-state リポジトリ
  # 側の ADR-0015)。
  home.file.".claude/skills/venue-search/SKILL.md".source =
    repoConfig + "/claude/skills/venue-search/SKILL.md";
  home.file.".claude/skills/venue-search/scripts/venue-urls.sh" = {
    source = repoConfig + "/claude/skills/venue-search/scripts/venue-urls.sh";
    executable = true;
  };
  home.file.".claude/skills/venue-search/scripts/noah-chart.js".source =
    repoConfig + "/claude/skills/venue-search/scripts/noah-chart.js";

  # ADR-0016 (tarotene/dotfiles): skills も AGENTS.md と同型のクロスツール
  # ルーティング対象 — 正本はツール中立の .agents/skills/(Codex CLI・
  # Copilot CLI がネイティブ読取)。上の .claude/skills/ 配下の各スキルと
  # 同一ソースを .agents/skills/ にも張る(コメント・追跡ロジックは
  # .claude/skills/ 側に一本化し、ここは張り替えのみ)。Claude Code が
  # .agents/skills/ をネイティブに読むようになったらこのブロックは撤去する。
  home.file.".agents/skills/diagramming/SKILL.md".source =
    repoConfig + "/claude/skills/diagramming/SKILL.md";
  home.file.".agents/skills/diagramming/cases.md".source =
    repoConfig + "/claude/skills/diagramming/cases.md";
  home.file.".agents/skills/skill-gardening/SKILL.md".source =
    repoConfig + "/claude/skills/skill-gardening/SKILL.md";
  home.file.".agents/skills/test-grounding/SKILL.md".source =
    repoConfig + "/claude/skills/test-grounding/SKILL.md";
  home.file.".agents/skills/test-grounding/cases.md".source =
    repoConfig + "/claude/skills/test-grounding/cases.md";
  home.file.".agents/skills/living-description/SKILL.md".source =
    repoConfig + "/claude/skills/living-description/SKILL.md";
  home.file.".agents/skills/living-description/cases.md".source =
    repoConfig + "/claude/skills/living-description/cases.md";
  home.file.".agents/skills/issue-ref-freshness/SKILL.md".source =
    repoConfig + "/claude/skills/issue-ref-freshness/SKILL.md";
  home.file.".agents/skills/pr-description/SKILL.md".source =
    repoConfig + "/claude/skills/pr-description/SKILL.md";
  home.file.".agents/skills/pr-description/cases.md".source =
    repoConfig + "/claude/skills/pr-description/cases.md";
  home.file.".agents/skills/wrapup-chores/SKILL.md".source =
    repoConfig + "/claude/skills/wrapup-chores/SKILL.md";
  home.file.".agents/skills/copilot-model-bump/SKILL.md".source =
    repoConfig + "/claude/skills/copilot-model-bump/SKILL.md";
  home.file.".agents/skills/issue-hygiene/SKILL.md".source =
    repoConfig + "/claude/skills/issue-hygiene/SKILL.md";
  home.file.".agents/skills/tracking-issue/SKILL.md".source =
    repoConfig + "/claude/skills/tracking-issue/SKILL.md";
  home.file.".agents/skills/handoff/SKILL.md".source = repoConfig + "/claude/skills/handoff/SKILL.md";
  home.file.".agents/skills/handoff/scripts/handoff.sh" = {
    source = repoConfig + "/claude/skills/handoff/scripts/handoff.sh";
    executable = true;
  };
  home.file.".agents/skills/claude-routines/SKILL.md".source =
    repoConfig + "/claude/skills/claude-routines/SKILL.md";
  home.file.".agents/skills/claude-routines/auditor.md".source =
    repoConfig + "/claude/skills/claude-routines/auditor.md";
  home.file.".agents/skills/claude-routines/scripts/routines-plan.sh" = {
    source = repoConfig + "/claude/skills/claude-routines/scripts/routines-plan.sh";
    executable = true;
  };
  home.file.".agents/skills/claude-routines/scripts/selftest.sh" = {
    source = repoConfig + "/claude/skills/claude-routines/scripts/selftest.sh";
    executable = true;
  };
  home.file.".agents/skills/claude-routines/scripts/lib.jq".source =
    repoConfig + "/claude/skills/claude-routines/scripts/lib.jq";
  home.file.".agents/skills/stacked-pr/SKILL.md".source =
    repoConfig + "/claude/skills/stacked-pr/SKILL.md";
  home.file.".agents/skills/scope-inventory/SKILL.md".source =
    repoConfig + "/claude/skills/scope-inventory/SKILL.md";
  home.file.".agents/skills/precedent-grounding/SKILL.md".source =
    repoConfig + "/claude/skills/precedent-grounding/SKILL.md";
  home.file.".agents/skills/selection-grounding/SKILL.md".source =
    repoConfig + "/claude/skills/selection-grounding/SKILL.md";
  home.file.".agents/skills/github-audit-triage/SKILL.md".source =
    repoConfig + "/claude/skills/github-audit-triage/SKILL.md";
  home.file.".agents/skills/repo-charter/SKILL.md".source =
    repoConfig + "/claude/skills/repo-charter/SKILL.md";
  home.file.".agents/skills/repo-charter/cases.md".source =
    repoConfig + "/claude/skills/repo-charter/cases.md";
  home.file.".agents/skills/writing-style/SKILL.md".source =
    repoConfig + "/claude/skills/writing-style/SKILL.md";
  home.file.".agents/skills/gas-clasp-ops/SKILL.md".source =
    repoConfig + "/claude/skills/gas-clasp-ops/SKILL.md";
  home.file.".agents/skills/gpg-subkey-rotation/SKILL.md".source =
    repoConfig + "/claude/skills/gpg-subkey-rotation/SKILL.md";
  home.file.".agents/skills/external-call-scheduling/SKILL.md".source =
    repoConfig + "/claude/skills/external-call-scheduling/SKILL.md";
  home.file.".agents/skills/slot-availability/SKILL.md".source =
    repoConfig + "/claude/skills/slot-availability/SKILL.md";
  home.file.".agents/skills/slot-availability/scripts/slot-hit.sh" = {
    source = repoConfig + "/claude/skills/slot-availability/scripts/slot-hit.sh";
    executable = true;
  };
  home.file.".agents/skills/slot-availability/scripts/selftest.sh" = {
    source = repoConfig + "/claude/skills/slot-availability/scripts/selftest.sh";
    executable = true;
  };
  home.file.".agents/skills/slot-availability/scripts/lib.jq".source =
    repoConfig + "/claude/skills/slot-availability/scripts/lib.jq";
  home.file.".agents/skills/slot-availability/scripts/judge.jq".source =
    repoConfig + "/claude/skills/slot-availability/scripts/judge.jq";
  home.file.".agents/skills/slot-availability/scripts/finalize.jq".source =
    repoConfig + "/claude/skills/slot-availability/scripts/finalize.jq";
  home.file.".agents/skills/performance-planning/SKILL.md".source =
    repoConfig + "/claude/skills/performance-planning/SKILL.md";
  home.file.".agents/skills/venue-search/SKILL.md".source =
    repoConfig + "/claude/skills/venue-search/SKILL.md";
  home.file.".agents/skills/venue-search/scripts/venue-urls.sh" = {
    source = repoConfig + "/claude/skills/venue-search/scripts/venue-urls.sh";
    executable = true;
  };
  home.file.".agents/skills/venue-search/scripts/noah-chart.js".source =
    repoConfig + "/claude/skills/venue-search/scripts/noah-chart.js";

  # rust-repo-governance / typst-repo-governance / astro-site-governance:
  # #151 で ~/.claude/skills/ の未バージョン管理状態から dotfiles 管理に
  # 移設。ディレクトリ全体を単一シンボリックリンクとしてデプロイし(個別
  # ファイル列挙はしない — scripts/templates/reference の下位構造は各
  # スキル側で完結している。templates/.github/rulesets/{security,workflow,
  # review}.json だけは下の repo-governance-common ブロックが上書きする)、
  # 上と同じ理由で .agents/skills/ にも同一ソースを張る。
  home.file.".claude/skills/rust-repo-governance" = {
    source = repoConfig + "/claude/skills/rust-repo-governance";
    recursive = true;
  };
  home.file.".agents/skills/rust-repo-governance" = {
    source = repoConfig + "/claude/skills/rust-repo-governance";
    recursive = true;
  };
  home.file.".claude/skills/typst-repo-governance" = {
    source = repoConfig + "/claude/skills/typst-repo-governance";
    recursive = true;
  };
  home.file.".agents/skills/typst-repo-governance" = {
    source = repoConfig + "/claude/skills/typst-repo-governance";
    recursive = true;
  };
  home.file.".claude/skills/astro-site-governance" = {
    source = repoConfig + "/claude/skills/astro-site-governance";
    recursive = true;
  };
  home.file.".agents/skills/astro-site-governance" = {
    source = repoConfig + "/claude/skills/astro-site-governance";
    recursive = true;
  };

  # repo-governance-common: 上の3 skill が共有する scripts/templates の
  # 正本(#388 の三重化還元)。setup-hooks.sh / apply-repo-settings.sh は
  # 3 skill でロジック差分ゼロだったため丸ごと1本化、
  # templates/.github/rulesets/{security,workflow,review}.json はバイト
  # 単位で完全一致だったため同様に1本化した。quality.json と
  # copy-files.sh/seed.sh はエコシステム固有差分が実在するため各 skill 側
  # に残る(apply 自体は ADR-0000-rulesets-declaration-in-repo D4 により
  # スクリプト側の型分岐そのものを撤去したので、共有コアを source する
  # 層はもう無い)。
  #
  # まずディレクトリ自体を上と同じ recursive マウントで配る。次に、各
  # governance skill の recursive マウントは「そのスキルの**自分の**
  # ソースツリーに存在するファイルだけ」を個別シンボリックリンクするため
  # (setup-hooks.sh 等はソースツリーから削除済み)、ここで正本を指す
  # ネストした home.file エントリを skill × マウントルート(.claude/
  # .agents)の組ごとに個別に宣言して併置する(ADR-0032 と同型の単一正本・
  # 複数マウント、#388)。nix の内包表記(listToAttrs 等)で動的生成する案も
  # 検討したが、このモジュールの規模では生成した attrset を
  # `home.file = ...;` として直接代入する形が既存の dotted attrpath 宣言群
  # と "attribute 'home.file' already defined" で衝突し、回避には
  # モジュール全体を lib.mkMerge で包む必要があった — 1600 行超のファイル
  # 全体の再インデントという不釣り合いなコストを伴うため、30 行の手書き
  # 列挙(このコメントの直後)に倒した。
  home.file.".claude/skills/repo-governance-common" = {
    source = repoConfig + "/claude/skills/repo-governance-common";
    recursive = true;
  };
  home.file.".agents/skills/repo-governance-common" = {
    source = repoConfig + "/claude/skills/repo-governance-common";
    recursive = true;
  };
  home.file.".claude/skills/rust-repo-governance/scripts/setup-hooks.sh" = {
    source = repoConfig + "/claude/skills/repo-governance-common/scripts/setup-hooks.sh";
    executable = true;
  };
  home.file.".claude/skills/rust-repo-governance/scripts/apply-repo-settings.sh" = {
    source = repoConfig + "/claude/skills/repo-governance-common/scripts/apply-repo-settings.sh";
    executable = true;
  };
  home.file.".claude/skills/rust-repo-governance/templates/.github/rulesets/security.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/security.json";
  home.file.".claude/skills/rust-repo-governance/templates/.github/rulesets/workflow.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/workflow.json";
  home.file.".claude/skills/rust-repo-governance/templates/.github/rulesets/review.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/review.json";
  home.file.".agents/skills/rust-repo-governance/scripts/setup-hooks.sh" = {
    source = repoConfig + "/claude/skills/repo-governance-common/scripts/setup-hooks.sh";
    executable = true;
  };
  home.file.".agents/skills/rust-repo-governance/scripts/apply-repo-settings.sh" = {
    source = repoConfig + "/claude/skills/repo-governance-common/scripts/apply-repo-settings.sh";
    executable = true;
  };
  home.file.".agents/skills/rust-repo-governance/templates/.github/rulesets/security.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/security.json";
  home.file.".agents/skills/rust-repo-governance/templates/.github/rulesets/workflow.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/workflow.json";
  home.file.".agents/skills/rust-repo-governance/templates/.github/rulesets/review.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/review.json";
  home.file.".claude/skills/typst-repo-governance/scripts/setup-hooks.sh" = {
    source = repoConfig + "/claude/skills/repo-governance-common/scripts/setup-hooks.sh";
    executable = true;
  };
  home.file.".claude/skills/typst-repo-governance/scripts/apply-repo-settings.sh" = {
    source = repoConfig + "/claude/skills/repo-governance-common/scripts/apply-repo-settings.sh";
    executable = true;
  };
  home.file.".claude/skills/typst-repo-governance/templates/.github/rulesets/security.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/security.json";
  home.file.".claude/skills/typst-repo-governance/templates/.github/rulesets/workflow.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/workflow.json";
  home.file.".claude/skills/typst-repo-governance/templates/.github/rulesets/review.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/review.json";
  home.file.".agents/skills/typst-repo-governance/scripts/setup-hooks.sh" = {
    source = repoConfig + "/claude/skills/repo-governance-common/scripts/setup-hooks.sh";
    executable = true;
  };
  home.file.".agents/skills/typst-repo-governance/scripts/apply-repo-settings.sh" = {
    source = repoConfig + "/claude/skills/repo-governance-common/scripts/apply-repo-settings.sh";
    executable = true;
  };
  home.file.".agents/skills/typst-repo-governance/templates/.github/rulesets/security.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/security.json";
  home.file.".agents/skills/typst-repo-governance/templates/.github/rulesets/workflow.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/workflow.json";
  home.file.".agents/skills/typst-repo-governance/templates/.github/rulesets/review.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/review.json";
  home.file.".claude/skills/astro-site-governance/scripts/setup-hooks.sh" = {
    source = repoConfig + "/claude/skills/repo-governance-common/scripts/setup-hooks.sh";
    executable = true;
  };
  home.file.".claude/skills/astro-site-governance/scripts/apply-repo-settings.sh" = {
    source = repoConfig + "/claude/skills/repo-governance-common/scripts/apply-repo-settings.sh";
    executable = true;
  };
  home.file.".claude/skills/astro-site-governance/templates/.github/rulesets/security.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/security.json";
  home.file.".claude/skills/astro-site-governance/templates/.github/rulesets/workflow.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/workflow.json";
  home.file.".claude/skills/astro-site-governance/templates/.github/rulesets/review.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/review.json";
  home.file.".agents/skills/astro-site-governance/scripts/setup-hooks.sh" = {
    source = repoConfig + "/claude/skills/repo-governance-common/scripts/setup-hooks.sh";
    executable = true;
  };
  home.file.".agents/skills/astro-site-governance/scripts/apply-repo-settings.sh" = {
    source = repoConfig + "/claude/skills/repo-governance-common/scripts/apply-repo-settings.sh";
    executable = true;
  };
  home.file.".agents/skills/astro-site-governance/templates/.github/rulesets/security.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/security.json";
  home.file.".agents/skills/astro-site-governance/templates/.github/rulesets/workflow.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/workflow.json";
  home.file.".agents/skills/astro-site-governance/templates/.github/rulesets/review.json".source =
    repoConfig + "/claude/skills/repo-governance-common/templates/.github/rulesets/review.json";

  # グローバル AGENTS.md(正本): agent 非依存の調査・先行例確認・PR 運用・
  # 生成元明示の方針。Codex CLI(~/.codex/AGENTS.md)・Copilot CLI
  # (~/.copilot/copilot-instructions.md)・Claude Code(~/.agents/AGENTS.md
  # を CLAUDE.md から @import)の 3 CLI に同一ソースをマウントする —
  # ~/.agents/skills/ のクロスツールルーティング(コメント索引 14) 付近)と
  # 同型。詳細は docs/claude/global-agents-md.md。
  home.file.".agents/AGENTS.md".source = repoConfig + "/agents/AGENTS.md";
  home.file.".copilot/copilot-instructions.md".source = repoConfig + "/agents/AGENTS.md";

  # ~/.codex/AGENTS.md: 共有 AGENTS.md(正本) + Codex 固有の施行配線
  # (config/codex/AGENTS.codex.md)を nix 評価時に結合した派生物。Codex は
  # `@import` 構文を持たないため、Claude Code の `~/.claude/CLAUDE.md` =
  # `@~/.agents/AGENTS.md` importと同じ「単一正本 + router」構造を、
  # build 時のファイル結合で実現する(ADR-0032 Amendment #531)。正本は
  # 依然 2 ファイル(共有 AGENTS.md・AGENTS.codex.md)のままで、結合物
  # そのものは手で複写せず nix が毎回生成する。
  home.file.".codex/AGENTS.md".text =
    builtins.readFile (repoConfig + "/agents/AGENTS.md")
    + "\n"
    + builtins.readFile (repoConfig + "/codex/AGENTS.codex.md");

  # グローバル CLAUDE.md: 上記共有 AGENTS.md を @import する router +
  # Claude Code 固有の施行配線(gate/ExitPlanMode/AskUserQuestion まわり)。
  # リポジトリルートの CLAUDE.md(`@AGENTS.md`)と同型の router 構造を
  # グローバル階層にも適用したもの。詳細は上のコメント索引 14) と
  # docs/claude/global-claude-md.md。
  home.file.".claude/CLAUDE.md".source = repoConfig + "/claude/CLAUDE.md";

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
      ${lib.escapeShellArg bleepClaudeCmd} \
      ${lib.escapeShellArg herdrMetadataCmd} \
      ${lib.escapeShellArg worktreeFreshBaseCmd} \
      ${lib.escapeShellArg worktreeCreateGuardCmd} \
      ${lib.escapeShellArg worktreeAuditContextCmd} \
      ${lib.escapeShellArg planScopeGateCmd} \
      ${lib.escapeShellArg planPrecedentGateCmd} \
      ${lib.escapeShellArg planFreshGateCmd} \
      ${lib.escapeShellArg attributionGuardCmd} \
      ${lib.escapeShellArg agentTurnLogCmd} \
      ${lib.escapeShellArg atuinHookClaudeCodeCmd} \
      ${lib.escapeShellArg stackBaseGuardCmd} \
      ${lib.escapeShellArg prTitleGuardCmd} \
      ${lib.escapeShellArg prConfirmGuardCmd} \
      ${lib.escapeShellArg decisionColocationGuardCmd} \
      ${lib.escapeShellArg externalSendGuardCmd} \
      ${lib.escapeShellArg adrNumberCmd} \
      ${lib.escapeShellArg ghEditAllowCmd} \
      ${lib.escapeShellArg rulesetsWriteGuardCmd} \
      ${lib.escapeShellArg routinesWriteGuardCmd} \
      ${lib.escapeShellArg pkexecGuardCmd} \
      ${lib.escapeShellArg newToolGuardCmd} \
      ${lib.escapeShellArg feedbackTargetGuardCmd} \
      ${lib.escapeShellArg repoCreateGuardCmd} \
      ${lib.escapeShellArg cmdHashLogCmd}
  '';

  home.activation.registerClaudeStatusLine = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${syncStatusLine} "$HOME/.claude/settings.json" \
      ${lib.escapeShellArg statusLineCmd}${
        lib.concatMapStrings (c: " \\\n      " + lib.escapeShellArg c) retiredStatusLineCommands
      }
  '';

  # 今のモード(Fable / Opus)は保ったまま、その具体モデル ID だけを claude バイナリの
  # `latest_per_family` から引き直す。hooks・statusLine・permissions と同じ DAG 位置で、
  # 独立した activation として走らせる。
  #
  # claude が未インストールなら(bootstrap 直後)何も書かずに終わる — 起動する claude が
  # 無いのに env だけ置いても意味が無く、中途半端な model 設定のほうが有害だから。
  home.activation.registerClaudeModelConfig = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run env PATH=${planModelSyncPath}:"$HOME/.local/bin":"$PATH" \
      ${pkgs.bash}/bin/bash ${planModelScript} sync
  '';

  # settings.json の permissions.allow/permissions.ask を冪等に拡充する。
  # registerClaudeHooks と同じ DAG 位置(writeBoundary の後)で、独立した
  # activation script として走らせる — 片方が既存の hooks 登録ロジックを
  # 壊さないようにするため、jq マージの責務を混ぜない。
  home.activation.registerClaudePermissions = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${registerPermissions} "$HOME/.claude/settings.json" \
      --retire \
      ${lib.concatMapStringsSep " \\\n      " lib.escapeShellArg retiredPermissionRules} \
      --allow \
      ${lib.concatMapStringsSep " \\\n      " lib.escapeShellArg permissionRules} \
      --retire-ask \
      ${lib.concatMapStringsSep " \\\n      " lib.escapeShellArg retiredAskRules} \
      --ask \
      ${lib.concatMapStringsSep " \\\n      " lib.escapeShellArg askRules}
  '';
}
