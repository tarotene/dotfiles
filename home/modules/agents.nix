# Claude Code のフック群 — plan-review ゲート・wrap-up inbox・plan-view・
# gpg-prewarm・pr-gate・issue-index を home-manager で配備する。
#
# 【2026-10-24 の注記】ツール呼び出しに付く検査 19 件(PreToolUse の拒否 15 件と、PostToolUse の
# 記録役 4 件)は、検査ごとの登録をやめ、`hook-router`(hookRouterPreCmd / hookRouterPostCmd の
# 2 件)が規則の表(crates/hook-router/src/rules.rs)に従って走らせる(docs/agents/hook-router.md)。
# 下の各項(8〜38 番)にある、その 19 件の「matcher: …」「if: …」の記述は、旧登録の記録として残す。
# 【#991】ExitPlanMode の拒否 2 件(plan-scope-gate・plan-precedent-gate、15・16 番)も、同じ表に
# 載せた(合わせて 21 件、PreToolUse の拒否 17 件)。15・16 番の「ExitPlanMode matcher に N 番目の
# エントリとして並ぶ」の記述は、旧登録の記録として残す。
# ci-wait-guard(Bash と Monitor、docs/agents/ci-wait-guard.md)も同じ表に載せた(合わせて 22 件、
# PreToolUse の拒否 18 件)。
#
# 1) plan-review ゲート(PreToolUse / ExitPlanMode):
#    GitHub Copilot CLI(read-only custom agent)によるプランの自動レビュー。
#    gate は acceptance-convergent であり、critic は verdict を出さず、
#    judge(jq)が決定論的に gate 適格性を判定する。最終ラウンドは closer。
#    詳細は docs/agents/copilot-plan-review.md(旧 Codex 版の設計経緯も同ファイルに
#    履歴として残る)。
#
# 2) wrap-up inbox(SessionStart + Stop):
#    スコープ外の気づきの「収集」と「起票」を分離する。SessionStart hook が
#    additionalContext で「気づきは state 領域の inbox(JSONL)に --add で追記せよ」
#    と注入し、Stop hook が inbox 非空かつ起票可能(gh あり・git repo・GitHub
#    remote あり)なら exit 2 + stderr 指示でフルコンテキストの本体 Claude に
#    gh issue create させる。それ以外は黙って exit 0(ADR-0005 の binary-existence
#    gating に倣う)。詳細は docs/agents/wrapup-inbox.md。
#
# 3) plan-view(PreToolUse / ExitPlanMode + CLI):
#    プランを pandoc で HTML にして Chrome の専用窓(--app)に飛ばす。LLM は呼ばず、
#    Markdown を 1:1 で写すだけの表示専用の道具である。plan-review gate と同じ
#    matcher に別エントリとして並び、並列に走る(= review の結果を待たない)。承認
#    フローに干渉しないため、成否に関わらず stdout に何も出さず exit 0 する。
#    詳細は docs/agents/plan-view.md。
#
# 4) gpg-prewarm(SessionStart, matcher: startup|resume):
#    git commit の署名パスフレーズ入力を、ログイン後最初に Claude を開いた安全な
#    瞬間に前倒しする。home/modules/gpg.nix が gpg-agent の cache TTL を実質無限に
#    したことで再入力は「ログインに 1 回」まで落ちるが、その 1 回を放置すると
#    Claude の Bash 呼び出し中に GUI pinentry が grab 付きで出現し、キー入力を
#    奪ったままコミットが固まる。判定は SessionStart 時の cwd に依存せず、常に
#    グローバルな git 設定だけを見る。additionalContext は出さない(副作用だけの
#    hook)。詳細は docs/agents/gpg-prewarm.md、脅威モデルの変化は ADR-0003 Amendment 2。
#
# 5) pr-gate(SessionStart + Stop):
#    「CI 待ちのまま完了を宣言する」「push し忘れたまま完了する」「PR を Issue に
#    繋がないまま終わる」「見た目の変更なのに視覚証跡が無いまま終わる」の 4 事故を、
#    Stop の 1 点だけで hard gate する(base 鮮度・未コミット変更は advisory)。
#    判定対象は ~/.claude/pr-gate-repos に列挙した nwo だけ(既定は本リポジトリの
#    み)で、それ以外では完全沈黙する。中心不変条件は「揃っていない集合を緑と読ま
#    ないこと」— 期待される required check をサーバの ruleset から取り、
#    `gh pr checks --watch` の exit code ではなく取り直した --json で判定
#    する。実体は crates/pr-gate(#415、ADR-0024 Stage 4a)。視覚証跡(G_visual)は 12) の pr-description スキルが定める本文スケルト
#    ンの `## Before / After` 節を検査し、証跡の有無だけを機械強制する(対比の完全
#    性はスキル側の責務)。詳細は docs/agents/pr-gate.md。
#
# 6) issue-index(SessionStart, matcher: startup|resume|compact):
#    自分に関係する open Issue の索引(番号・タイトル・ラベル・起票者)だけを
#    additionalContext で注入する。本文は渡さない — 深掘りは Claude 自身に
#    `gh issue view` を叩かせる。データ源は `gh issue list` ではなく GitHub
#    Search API(`gh api search/issues`): --limit は取得上限であり総数ではない
#    ので、それで総数を数えると嘘になる。assignee:@me が 0 件なら repo 全体の
#    open にフォールバックし、他人起票の行にだけ起票者を明記する(タイトルは
#    untrusted なので制御文字除去 + 120 文字切り詰めもするが、これは防御ではなく
#    payload 制御に過ぎない)。詳細は docs/agents/issue-index.md。
#
# 7) git-worktree-allow(PreToolUse, matcher: Bash, if: "Bash(git -C *)"):
#    herdr worktree を外から駆動する `git -C <worktree> <サブコマンド>` を検証して
#    プログラム的に許可する。`Bash(git -C * add *)` のような中間ワイルドカードの
#    permission rule は、`-C` の位置への任意オプション挿入(--exec-path 等 = 任意
#    コード実行)を素通しするため Claude Code が毎セッション警告し、しかも中間 `*`
#    は実際にはマッチしない。hook なら「実在する ~/.herdr/worktrees/ 配下のパス +
#    許可サブコマンド + 単一 git 呼び出し」を検証してから allow を返せる。
#    詳細は docs/agents/git-worktree-allow.md。
#
# 8) git-stash-guard(PreToolUse, matcher: Bash, if: "Bash(git *)"):
#    素の `git stash` / `git stash pop` / SHA 無しの apply・drop・裸の push・
#    clear を deny する。stash スタックはリポジトリ単位で herdr の worktree 間
#    (=セッション間)で共有されているため、素の stash は他セッションの WIP を
#    取り違えて pop/apply する事故につながる。git-worktree-allow(allow 側)とは
#    非対称: allow 側は if 不一致でも安全(単に許可を出さないだけ)だが、この
#    hook は deny 側なので if 不一致は検査されない素通り = 事故そのものになる。
#    かつ hook 宣言の一意キーは (event, command 文字列) なので、
#    同一スクリプトを 2 つの if で二重登録することもできない。よって if は
#    "Bash(git *)" まで広げ、絞り込みは hook 内部の早期 exit(grep → jq)に移した。
#    詳細は docs/agents/git-stash-guard.md。
#
# 9) herdr-sidebar-metadata(5 イベント + statusLine):
#    各エージェントの permission mode・モデル・context%・コスト・effort を Herdr
#    サイドバーに常時表示する。mode は hook input からしか取れず、モデル等は
#    statusline JSON からしか取れないため 2 チャネル構成になっている。トークン色は
#    静的指定のみなので、mode の色分けは「モード毎に別トークン + 非アクティブは
#    null クリア」で表現する。詳細は docs/agents/herdr-sidebar-metadata.md。
#
# 10) worktree-base-sync(SessionStart, matcher: startup|resume):
#    herdr の Workspace Fork は親チェックアウトの HEAD を fetch なしで使うため、
#    新しい worktree が古い base から生まれることがある。まだ何も積んでいない
#    pristine な worktree(作業ツリークリーン かつ ahead==0 かつ behind>0)に限り、
#    fetch 後に `git merge --ff-only` で origin/<base> へ黙って揃える。履行歴を
#    持つブランチは動かさない — 既存 pr-gate の base 追従 advisory とは非対称に
#    「本当に何もない」ケースだけを能動的に解消する。詳細は
#    docs/agents/worktree-base-sync.md。
#
# 11) Opus Plan Mode のモデル実体(crates/claude-plan-model + settings.json):
#    `model: "opusplan"` は「Plan 中は opus エイリアス、実行中は sonnet エイリアス」
#    という *エイリアスのペア* であり、各エイリアスがどの具体モデルに解決されるかは
#    settings.json の env で別に宣言できる。したがってモードは常に
#    (Plan 側, 実行側) のペアで、3 つある: opus/sonnet(既定)・fable/sonnet・
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
# 12) 個人スキル(diagramming, agent-tooling-authoring, living-description,
#     issue-ref-freshness, pr-description,
#     backlog-cleanup, model-pin-update, issue-hygiene, tracking-issue, stacked-pr):
#    hook ではなく ~/.claude/skills/ 配下に置く判断知識。diagramming は作図時に
#    「内容の型に合うジャンル・技術を選ぶ」処方と、手書き SVG に落ちた場合の
#    技術非依存の不変条件(矢印端点をボックス定義から導出する・完成の定義に視認を
#    含める等)を持つ。agent-tooling-authoring はこの知見をどう dotfiles(公開)に固定化
#    するかのメタスキルで、公開リポジトリ向けサニタイズ規則の正本を持つ。
#    living-description は Issue/PR の本文(Description)を「起票時点のスナップ
#    ショット」ではなく「現在の合意状態を表す正本」として扱い、コメントで裁定が
#    確定した時点で本文を編集し続ける習慣(複数の関連Issueに仕様が重複している
#    場合は横断的に同期する)。issue-ref-freshness はその外部参照版で、他リポジトリ
#    の issue/PR を Open 前提で参照している記述を、参照先の裁定を読んで書き直す
#    (常時監視の timer は置かず、機会発動 + 手動の一括点検)。pr-description は PR 本文の標準スケルトン(課題・
#    解決策・Before/After・検証・要確認)と、見た目に影響する変更には Before/After
#    証跡を必ず添える習慣を持つ — 証跡の有無は 5) の pr-gate(G_visual)が機械強制
#    し、対比の完全性(ペア性)はこのスキルの責務として二層に分ける。backlog-cleanup
#    は 2) の wrap-up inbox のうち判断を要さない軽微な項目(未起票の inbox 行 +
#    起票済みの wrapup 由来 open Issue)をまとめて triage し、1 回の確認後に 1 つの
#    chores PR で一括対処する習慣 — hook 自体には手を入れず、削除は既存の
#    `--mark-filed` 経由のみを使う。model-pin-update は外部 AI CLI(Copilot CLI 等)
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
#    docs/agents/diagramming.md、docs/agents/agent-tooling-authoring.md、
#    docs/agents/living-description.md、docs/agents/issue-ref-freshness.md、
#    docs/agents/pr-description.md、
#    docs/agents/backlog-cleanup.md、docs/agents/model-pin-update.md、
#    docs/agents/issue-hygiene.md、docs/agents/tracking-issue.md、
#    docs/agents/stacked-pr.md
#    (腐る事実は docs/stacked-pr-github-native.md に切り出す、ADR-0008)。
#    presentation-authoring は発表のスライドを作るとき、発表原稿(単独で練習でき、
#    ホワイトボードで話しても成り立つプレインテキスト)を正本として先に全文書き、
#    スライドをその補助資料として派生させる手順を持つ。正本は Marp Markdown 1
#    ファイルで、語りは各スライド末尾のコメントに置き、練習用の原稿は
#    `marp --notes` で導出する。見た目に凝った HTML だけを出す第三者スキル
#    frontend-slides はスピーカーノートを持たないため、スキルとしては置かず
#    (description の発火衝突を避ける)、rev 固定で ~/.local/share/frontend-slides/
#    に配備して、本人が明示したときだけ presentation-authoring が呼ぶ。詳細は
#    docs/agents/presentation-authoring.md。
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
#    メモ追記ショートカットは書き込み失敗する — 知見の永続化は agent-tooling-authoring
#    の PR フローに乗せる想定であり、意図的な設計。詳細は
#    docs/claude/global-claude-md.md と docs/agents/global-agents-md.md、
#    docs/adr/0032-global-agent-instructions-canon.md の Amendment。
#
# 15) requirements-inventory(個人スキル、global CLAUDE.md の 1 節)+ plan-scope-gate
#     (PreToolUse / ExitPlanMode):
#    Tracking Issue のような複数項目を含む依頼を Plan Mode に投げると、作業スコープ
#    の増大を気にして依頼された範囲を黙って縮小した計画を返してくることが多い。
#    global CLAUDE.md に「複数項目の依頼は計画冒頭に要求インベントリ(逐語列挙 +
#    Rn の ID)を置く」という短い規律を追加し、requirements-inventory スキルがその作り方
#    (gh graphql での sub-issues 列挙・閉じた棄却タグ Blocked-Upstream/Obsolete/
#    User-Excluded・参照 Issue を Reference-Only: で書き分ける手順)を持つ。
#    plan-scope-gate(crates/plan-scope-gate)は指示文だけでは足りない部分(BAITBENCH: 明示的に禁止しても
#    ショートカット使用率は平均50%超)を機械検査で塞ぐ — LLM を呼ばず、文字列と gh
#    だけで判定する純粋な judge。plan-review / plan-view と同じ matcher に 3 つ目の
#    エントリとして並ぶ。経路A(ユーザー発言から参照 Issue を抽出し、子 sub-issues
#    のカバレッジを検査)と経路B(`## 要求インベントリ` 節内の処分の整合性を検査)の
#    2本立て。当初検討した「プラン中の縮小マーカーを起点に検査する」設計は、過去
#    プラン327本の実測で誤検知率が高すぎて棄却した。詳細は docs/agents/requirements-inventory.md。
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
#    機械 gate(crates/plan-precedent-gate)が担う。plan-review / plan-view /
#    plan-scope-gate と同じ ExitPlanMode matcher に 4 つ目のエントリとして並ぶ。
#    詳細は docs/adr/0012-precedent-grounding-over-prompted-adversarial-review.md
#    と docs/agents/precedent-grounding.md。
#
# 17) attribution-guard(PreToolUse, matcher: "Bash|mcp__.*"):
#    Claude が GitHub に書く外向きテキスト(PR/Issue の create・edit、Issue/PR
#    コメント、gh pr review のレビュー本体)に attribution フッターが載って
#    いることを保証する。PR 本文のフッターは harness 側の attribution 指示
#    由来なので、(a) コメント投稿には一切付かず、(b) 本文側も repo に強制が
#    無い(pr-gate は G_link / G_visual しか見ない)という 2 つの穴があった。
#    投稿は通知が飛ぶ不可逆操作なので Stop hook では取り返せず、PreToolUse で
#    deny する。抜け道は本文マーカー No-Attribution: <理由>(pr-gate の
#    No-Issue: と同型、理由必須で grep 可能)。
#
#    matcher は bleep と同じ複合 1 本 "Bash|mcp__.*" — MCP GitHub は
#    現在未接続だが、matcher を Bash 単体にすると接続した瞬間に無検査になる
#    (下の bleep の登録コメントが、旧実装の同じ欠陥を「最大の機能
#    欠陥」と記録している)。絞り込みは hook 内部の早期 exit に置く。
#    詳細は docs/agents/attribution-guard.md。
#
#    Codex CLI / Copilot CLI にも同じ判定エンジンを展開する(#192)。
#    判定は Rust(crates/attribution-guard、ADR-0024 Stage 4a #415)で、
#    3 エージェントから同じ 1 バイナリを `--agent claude|codex|copilot`
#    で呼ぶ(#391、pkexec-guard と同じ型)— bash 時代の per-agent adapter
#    (config/{codex,copilot}/hooks/attribution-guard.sh)は廃止した。
#    登録は bleep の Codex/Copilot 登録ブロック(下方)と同じ lost-update
#    対策の順序付けに続ける。
#
# 18) plan-fresh-gate(PreToolUse / ExitPlanMode):
#    herdr worktree を並行 Plan モードでパイプライン駆動する運用では、先発の
#    PR が merge された後も後発のエージェントがセッション開始時点の古い
#    コードベースを見たままプランを承認してしまう。worktree-fresh-base.sh は
#    SessionStart 限定の pristine ff-only 追従しか持たず、長い Plan セッション
#    中の drift はノーガードだった。この hook が ExitPlanMode 承認点そのもので
#    その隙間を塞ぐ: 常に fetch し、pristine(worktree-fresh-base.sh と同じ
#    5 条件)なら ff-only で追従、origin/<base> の進行分がプラン参照ファイルと
#    交差するときだけ deny する。pr-gate の G_base は同種の drift を
#    advisory に留めるが、その根拠(block が rebase → force-push ループを
#    誘発する)はここでは成立しない — 要求するのは履歴改変ではなく「再読 +
#    再 ExitPlanMode」だけで、deny 済み SHA のセッション state 記録により
#    有限回に収束する。plan-review / plan-view / plan-scope-gate /
#    plan-precedent-gate と同じ matcher に 5 つ目のエントリとして並ぶ。
#    詳細は docs/agents/plan-fresh-gate.md。
#
# 19) stack-base-guard(PreToolUse, matcher: "Bash|mcp__.*", ADR-0027):
#    セッション内の複数 PR は依存関係を予測せず常に作成順の単一チェーンに
#    積む(uncertainty-first stacking)ことを、`gh pr create` / `gh pr edit
#    --base`(および相当する MCP GitHub 呼び出し)の作成時に機械強制する。
#    層(i) 状態レスの祖先一致検査(HEAD が他の open PR のコミットを祖先に
#    含むなら base はその PR の head でなければならない — タグでも抜けられ
#    ない)と、層(ii) セッション ID 単位の状態(2 本目以降のチェーン外
#    ブランチは本文 `Independent-PR: <理由>` を要求する)の 2 層で判定する。
#    判定不能はすべて fail-open。判定は Rust(crates/stack-base-guard、
#    コマンド解析は crates/github-command、ADR-0024 Stage 4a #415)で、Claude と
#    Codex から同じ 1 バイナリを `--agent claude|codex` で呼ぶ。完了時の
#    対になる強制(`gh stack link` の要求)は pr-gate の G_stack が担う。
#    詳細は docs/adr/0027-uncertainty-first-stacking.md と
#    docs/agents/stack-base-guard.md。
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
#    Angular 慣行の 11 type 閉集合(crates/pr-title-check の check_title が
#    判定エンジンの単一ソース)に非適合なら deny する。発火は owner が tarotene
#    のリポジトリ限定(会社ホストにも common 層として配備されるため)。判定不能・
#    非 tarotene owner はすべて fail-open。一時解除は環境変数
#    PR_TITLE_GUARD_ALLOW=1。判定は Rust(crates/pr-title-guard、ADR-0024
#    Stage 4a #415)で、3 エージェントから同じ 1 バイナリを
#    `--agent claude|codex|copilot` で呼ぶ(#391、attribution-guard と同じ型)。
#    詳細は
#    docs/adr/0031-pr-title-as-commit-message-contract.md と
#    docs/agents/pr-title-contract.md。
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
#    矛盾するため。詳細は docs/agents/external-send-guard.md。
#
# 23) human-task-scheduling(個人スキル):
#    電話・来店・窓口対応など Claude が代行できないハンドオフ作業を、トーク
#    スクリプトを文書化して渡すだけで終わらせない。相手の営業時間・定休日と
#    ユーザーの予定表上の空き時間を突き合わせ、衝突しない枠にリマインダー
#    予定を作成するところまでを手順化する。Claude が提案する予定専用の
#    カレンダーが既にあればそれを使い、無ければ書き込み先をユーザーに確認
#    する。カレンダー・メール等の外部サービスに送る文章は agent-tooling-authoring の
#    サニタイズ規則(固有名詞・URL・内部識別子を書かない)を適用する対象だと
#    明記している — リポジトリ内の成果物とは別の基準がかかる。詳細は
#    docs/agents/human-task-scheduling.md。
#
# 24) decision-colocation-guard(PreToolUse, matcher: "Bash|mcp__.*", ADR-396):
#    決定成果物(ADR/設計文書/skill)の新規追加、または既存 ADR への
#    `## Amendment` 追加を、その決定を執行する実ファイルの同梱なしに `gh pr
#    create` させない。判定は Rust(crates/decision-colocation、ADR-0024
#    Stage 4a #415)の check::run_check 1 本で、CI required check
#    (decision-colocation-check bin)と同じ関数を共有する(単一ソース)。
#    執行点として認めるパスは「非 .md かつ docs/ 配下でない」の 2 述語のみ。
#    実在するが無変更のパスの併記だけでは合格しない(ADR-387 を意図的に
#    不合格側に倒して検算 — docs/agents/decision-colocation.md 参照)。
#    コマンド解析は crates/github-command(bash 時代の「attribution-guard.sh を
#    source して is_target_at を上書きする」型の置き換え)。Codex/Copilot
#    adapter は作らない — CI required check が全エージェント共通の backstop
#    になるため。一時解除は環境変数 SKIP_DECISION_COLOCATION_GUARD=1。詳細は
#    docs/adr/396-decision-colocation.md と docs/agents/decision-colocation.md。
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
#    human-task-scheduling とは発火条件が異なる(こちらは Claude 自身が
#    実行できる回答作業)ため独立したスキルにした。個人のカレンダー allowlist
#    等は private な person-state リポジトリに置き、既定パス
#    `~/.config/slot-availability/config.toml` へ手動シンボリックリンクで
#    接続する(dotfiles には置かない)。詳細は docs/agents/slot-availability.md。
#
# 26) music-performance-planning(個人スキル):
#    演奏本番(アマオケ・室内楽・ソロ)の練習計画提案(需要 vs 供給・
#    マイルストーン到来の確認)・合わせ調整の統合(slot-availability の
#    judge/finalize に委譲し、確定後を person-state リポジトリ側の TOML へ
#    反映)・当日タイムテーブル/遠征の Calendar dispatch を行う。データの
#    正本(`state/performances/`、ADR-0009)は別の private な person-state
#    リポジトリに置き、`performance-hub`(crates/hub-resolve、writing-style-hub と
#    同型のマーカーファイル/環境変数間接参照)でパスを解決する。yq の
#    TOML エンコーダが配列・テーブルの書き込みをサポートしないため、TOML
#    への実際の書き込みは Claude が Edit ツールで行う分担にしている。
#    詳細は docs/agents/music-performance-planning.md。
#
# 27) handoff(個人スキル)+ pr-gate の中断ハンドオフ節 + issue-index の
#    着手可能な handoff:ai 節:
#    ユーザーの指示で作業を途中で打ち切るとき、残タスクを Human/AI 双方に
#    振り分けて GitHub Issue に起票し、後で再開できる状態にする。WIP は
#    commit → push → Draft PR(本文に `Handoff: #N`)、担当は閉語彙ラベル
#    `handoff:human`/`handoff:ai`、順序は GitHub ネイティブの Issue
#    dependencies(blocked_by)で表す。誤りやすい API 操作(ラベル確認・
#    新設、blocked_by の設定)は `scripts/handoff.sh` の決定論的サブコマンドに
#    寄せる(番号→id の解決を含む)。詳細は docs/agents/handoff.md。
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
#    docs/agents/pkexec-guard.md。
#
# 30) new-tool-guard(PreToolUse, matcher: Write、ADR-543「既存手段の前倒し
#    接地と、決定論への昇格導線」、2026-09-28、段2/4):
#    「同じことをフルスクラッチではなく既存の枯れた技術で実現できないか」
#    (Q1)を、気付いたときではなく新しい道具・単位を Write する着手の瞬間
#    に前倒しで問う。対象は shebang(`#!`)で始まる新規ファイル、
#    `bin`/`scripts`/`hooks`/`cmd` を構成要素に含む新規パスの新規ファイル、
#    パッケージマニフェスト(`Cargo.toml`/`package.json`/`pyproject.toml`/
#    `go.mod`/`flake.nix`)の新設のいずれか(判定は crates/new-tool-guard
#    の `is_new_tool_unit` が単一正本で、`classify` サブコマンドと
#    pr-gate の G_prior が同じ関数を使う)。テスト・fixture・scratchpad 配下と、ディスク上に
#    既に存在するファイル(上書き)は対象外。該当かつ、現在の worktree の
#    session ledger(git toplevel キー、`~/.local/state/claude/new-tool-guard/state/`)
#    にそのパスの `既存手段:` 行が未登録なら deny する——deny メッセージが
#    `register` コマンドの完全形を同梱するので、登録して再試行すれば通る。
#    段1(ADR-543)の `## 先行例との対比` 節側の `既存手段:` 必須化との
#    使い分けは「着手前に Plan を書いたか」で決まらない: Plan を書かない
#    小さな作業でもこの hook が発火する。Claude Code 専用(#531、Codex/
#    Copilot 展開は別 Issue)。
#    記録役(PostToolUse, matcher: ExitPlanMode): 承認された計画の
#    `## 先行例との対比` にある `既存手段:` 行(書式が正しいものだけ)を、現在の
#    worktree の台帳へ転記する。承認時に plan-precedent-gate が読む行と同じ行を、
#    手で `register` し直す二重入力をなくす(台帳は計画の派生、正本は計画)。
#    手書きの `register` は、計画の無い作業の経路として残る。
#
# 31) feedback-target-guard(PreToolUse, matcher: Bash(gh *)、ADR-543、
#    2026-09-28、段3/4):
#    Q2(LLM/散文 → 決定論への昇格)の兆候の1つ「同じ規範・skill を指す
#    feedback Issue の再発」を機械的に集計できるようにする。`gh issue
#    create --label feedback`(カンマ区切りの複数ラベルも可)を検出し、
#    本文に `Target: skill/<name>` / `Target: agents-md/<節見出し>` /
#    `Target: hook/<name>` のいずれかが無い、またはその実体が存在しない
#    場合に deny する。判定は Rust(crates/feedback-target-guard、ADR-0024
#    Stage 4a #415)で、コマンド解析(heredoc 分離・トークナイザ・本文抽出)
#    は crates/github-command を共有する。
#
# 32) cmd-hash-log(PostToolUse, matcher: Bash、ADR-543、2026-09-28、段3/4):
#    Q2 の兆候の2つ目「同じコードブロックが改変なしに繰り返し実行されて
#    いる」(逐語反復)を検出するための入力を記録する。実行された Bash
#    コマンドの正規化(空白圧縮・行末継続結合)+ハッシュ(FNV-1a、依存
#    ゼロ)だけを `~/.local/state/claude/cmd-hashes.jsonl` に1行1レコード
#    で追記する——コマンド本文そのものはどこにも書かない(ADR-0011 の
#    プライバシー規約 `agent-turn-log` をさらに強めた形)。判定は返さ
#    ず、記録が失敗しても常に exit 0(fail-open)。正規化+ハッシュ関数は
#    `hook_io::cmd_hash` が単一正本で、段4 の promotion-detect が SKILL.md
#    のコードブロックを同じ関数でハッシュして照合する。
#
# 33) pr-followup-guard(PreToolUse, matcher: "Bash|mcp__.*"、
#    docs/agents/pr-followup-guard.md、2026-09-30 改定):
#    PR 本文に (i) 未チェックの task list(`- [ ]`)、または (ii) `## 要確認`
#    に Issue 参照(`#N` または issues URL)を持たない項目があれば
#    `gh pr create/edit` の呼び出し時に deny する。人の確認が要る残作業は
#    後続 Issue へ払い出し、「要確認」はその Issue へのポインタだけを書く
#    節に転換した(社内の別リポジトリで観測された、未チェック task list を
#    残したままマージされた実例が動機)。全リポジトリで発火する(owner スコープ
#    を持たない — attribution-guard.sh と同じ「書き手側の規律」の類型)。
#    escape hatch は PR_CONFIRM_GUARD_ALLOW=1。判定は Rust(crates/
#    pr-followup-guard、ADR-0024 Stage 4a #415)で、Codex にも同じバイナリを
#    `--agent codex` で登録する(下記 registerCodexPrConfirmGuardHooks)。
#
# 34) herdr-status-advisory(SessionStart, matcher: startup|resume、
#    docs/agents/herdr-status-advisory.md):
#    herdr の client だけが更新され server が古い版のまま残ると、
#    `herdr worktree create` が `protocol_mismatch` で失敗し、別リポジトリでの
#    作業が止まる。気づくのが作業ツリーを切る瞬間になってからで、server の
#    再起動は全ペインのプロセスを終了させるのでエージェントは復旧できない。
#    `herdr status --json` が示す食い違い(またはその前兆)を、セッション開始時に
#    additionalContext で先に伝える。復旧は何も実行しない。fail-open。
#
# 35) esa-mirror-context(SessionStart, matcher: startup|resume|compact、
#    docs/agents/esa-mirror-context.md):
#    エージェントが社内 wiki(esa)を見に行かなくなる問題への対処。指示文だけでは
#    実行がモデル任せになるので、日次 mirror のローカル clone の所在・検索手順・
#    引用形式・鮮度を additionalContext で運ぶ。発火は設定ファイル
#    (~/.config/esa-mirror-context/config.json、home/modules/esa-mirror-context.nix が option から
#    描画し、実値は private wrapper flake が注入する)と、cwd の origin owner が
#    設定の org に含まれることの AND。どちらかが欠ければ無音。出力は ADR-625 の
#    `<hook-directive>` 形式。fetch はせず、鮮度の維持は git-checkout-sync
#    タイマー(dotfiles.checkoutFreshness.extraPaths)に任せる。
#
# 36) inbox-adjudication(個人スキル、docs/agents/inbox-adjudication.md):
#    溜まった受信箱の処理を、既読化からでなく返信と判断の裁定から始める判断知識。
#    未読を全件読み、返信が要るものは下書きを非公開ページにまとめて本人が裁定し、
#    確定事項を予定と記録に反映し、フィルタにできる差出人を標準ルールに積み、
#    受信箱の整理は最後にする。送信・削除・退会は本人の操作。事例は cases.md。
#
# 37) gate-failopen-advisory(SessionStart, matcher: startup|resume、
#    docs/agents/gate-failopen-advisory.md、#822):
#    fail-open で設計した hook(copilot-plan-review)は、依存先が壊れても止まらない
#    代わりに、「壊れたまま素通りし続けている」ことに気づく手段が毎回の警告を人が
#    読むことしか無かった(固定したモデルが catalog から消えて約 2 週間半、気づかれ
#    なかった)。gate が gate-events.jsonl に残す fail-open / pass を末尾から数え、
#    同じ gate の最後の pass より後の fail-open が閾値(既定 3)以上なら、セッション
#    開始時に additionalContext で伝える。何も実行しない。fail-open。
#
# 38) daily-desk(個人スキル)+ daily-desk-reminder(SessionStart, matcher: startup、
#    docs/agents/daily-desk.md):
#    その日にさばく作業(返信が要るメール、日程回答、人手タスク、未消費 PR の裁定、
#    受信箱)の起点が、発火装置の違うものに散っていた問題への対処。手順の正本は
#    private なハブ側の desk.md で、スキルはハブ(performance-hub)を解決して従うだけの
#    薄いポインタ。hook は「今日の机は未実施」を注入し、実施済みの印
#    (~/.local/state/claude/desk/<JST の日付>)があれば無音。ハブ未設定の機体でも無音。
#
# 39) worklog(SessionStart, matcher: startup、docs/agents/worklog.md):
#    Git で管理するには重すぎる粒度の作業ログを、Obsidian vault の当日ノートへ 1 行ずつ
#    書かせる。hook は書く時点と kind の規範と、同じリポジトリの直近 7 日の stuck / found を
#    注入する。vault は ~/.config/dotfiles/obsidian-vault(home/modules/obsidian.nix が置く)で
#    宣言され、無い機体では黙って no-op するため、hook は全ホストへ無条件配備する。
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
  frontend-slides,
  ...
}:
let
  repoConfig = ../../config;
  hooksDir = "${config.home.homeDirectory}/.claude/hooks";
  # #412 (ADR-0024 Stage 4b): ExitPlanMode の plan hook 5 本(copilot-plan-review /
  # plan-view / plan-scope-gate / plan-precedent-gate / plan-fresh-gate)は Rust
  # バイナリ(crates/<name>、pkgs.dotfiles-tools)になり、下の home.file が
  # ~/.claude/hooks/<name>(拡張子無し)として配備する。bash を介さず直接起動
  # する。旧 `bash '….sh'` の command は retiredHookEntries で完全一致削除する
  # (legacyPlan*Cmd)。
  planReviewCmd = "'${hooksDir}/copilot-plan-review'";
  # #413 (ADR-0024 Stage 4d): session/表示系の hook は Rust バイナリ
  # (crates/wrapup-stop-gate など、pkgs.dotfiles-tools)になり、下の home.file が
  # ~/.claude/hooks/<name>(拡張子無し、ADR-0007)として配備する。bash を介さず
  # 直接起動する。旧 `bash '….sh'` の command は retiredHookEntries / --retire で
  # 完全一致削除する(legacy*Cmd、#638 と同じ形)。
  wrapupStopCmd = "'${hooksDir}/session-wrapup' stop";
  legacyWrapupStopCmd = "bash '${hooksDir}/wrapup-stop-gate.sh'";
  # Codex 向けは同じファイルを、フッターの agent 名/URL だけ環境変数で
  # 差し替えて直接登録する(adapter ファイルを新設しない — ADR-0032
  # Amendment #531、D8)。attribution-guard の Codex adapter と同じ
  # 定数値。
  priorCodexWrapupStopCmd = "ATTRIBUTION_AGENT_NAME='Codex CLI' ATTRIBUTION_AGENT_URL='https://learn.chatgpt.com/docs/codex/cli' '${hooksDir}/wrapup-stop-gate'";
  codexWrapupStopCmd = "WRAPUP_TRIAL_AGENT=codex ATTRIBUTION_AGENT_NAME='Codex CLI' ATTRIBUTION_AGENT_URL='https://learn.chatgpt.com/docs/codex/cli' ${wrapupStopCmd}";
  legacyCodexWrapupStopCmd = "ATTRIBUTION_AGENT_NAME='Codex CLI' ATTRIBUTION_AGENT_URL='https://learn.chatgpt.com/docs/codex/cli' bash '${hooksDir}/wrapup-stop-gate.sh'";
  wrapupSessionStartCmd = "'${hooksDir}/session-wrapup' start";
  retiredAgentPermissionsStartCmd = "'${hooksDir}/agent-permissions' hook session-start";
  retiredAgentPermissionsEndCmd = "'${hooksDir}/agent-permissions' hook session-end";
  codexWrapupSessionStartCmd = "WRAPUP_TRIAL_AGENT=codex ${wrapupSessionStartCmd}";
  legacyWrapupSessionStartCmd = "bash '${hooksDir}/wrapup-session-start.sh'";
  planViewCmd = "'${hooksDir}/plan-view'";
  issueIndexCmd = "'${hooksDir}/issue-index'";
  legacyIssueIndexCmd = "bash '${hooksDir}/issue-index.sh'";
  esaIndexCmd = "'${hooksDir}/esa-mirror-context'";
  deskStartCmd = "'${hooksDir}/daily-desk-reminder'";
  worklogStartCmd = "'${hooksDir}/worklog' session-start";
  signPrewarmCmd = "'${hooksDir}/gpg-prewarm'";
  legacySignPrewarmCmd = "bash '${hooksDir}/sign-prewarm.sh'";
  # #415 (ADR-0024 Stage 4a): pr-gate は Rust バイナリ(crates/pr-gate、
  # pkgs.dotfiles-tools)になり、下の home.file が ~/.claude/hooks/pr-gate
  # (拡張子無し)として配備する。旧 `bash '….sh'` の command は
  # retiredHookEntries / --retire で完全一致削除する(legacyPrGate*Cmd)。
  prGateSessionStartCmd = "'${hooksDir}/pr-gate' session-start";
  prGateStopCmd = "'${hooksDir}/pr-gate' stop";
  legacyPrGateSessionStartCmd = "bash '${hooksDir}/pr-gate.sh' session-start";
  legacyPrGateStopCmd = "bash '${hooksDir}/pr-gate.sh' stop";
  # pr-gate の判定(G_pr/G_link/G_visual/G_stack)は agent 名を一切
  # 参照しないため、Codex にも同じファイルを adapter 無しで直接登録する
  # (ADR-0032 Amendment #531)。
  # #416 (ADR-0024 Stage 4c): git/worktree 系の hook は Rust バイナリ
  # (crates/git-worktree-allow など、pkgs.dotfiles-tools)になり、下の
  # home.file が ~/.claude/hooks/<name>(拡張子無し)として配備する。bash を
  # 介さず直接起動する。旧 `bash '….sh'` の command は retiredHookEntries /
  # --retire で完全一致削除する(legacy*Cmd)。
  gitWorktreeAllowCmd = "'${hooksDir}/git-worktree-allow'";
  # hook-router(ツール呼び出しに付く検査 22 件を 1 つのプロセスで走らせる)。
  hookRouterPreCmd = "'${hooksDir}/hook-router' pre";
  hookRouterPostCmd = "'${hooksDir}/hook-router' post";
  gitStashGuardCmd = "'${hooksDir}/git-stash-guard'";
  # main-checkout-guard(docs/agents/main-checkout-guard.md): 本物の checkout
  # (main worktree)を変更させない。pre = PreToolUse(deny + baseline 記録)、
  # stop = Stop(baseline からの事後検出)。判定エンジンは 1 つで、Codex にも
  # 同じバイナリを --agent codex で直接登録する。
  mainCheckoutGuardPreCmd = "'${hooksDir}/main-checkout-guard' pre";
  mainCheckoutGuardStopCmd = "'${hooksDir}/main-checkout-guard' stop";
  codexMainCheckoutGuardPreCmd = "'${hooksDir}/main-checkout-guard' --agent codex pre";
  codexMainCheckoutGuardStopCmd = "'${hooksDir}/main-checkout-guard' --agent codex stop";
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
  # settings.json / hooks.json 系への「宣言 → 実体」の冪等 reconcile は 1 つの
  # Rust バイナリ(crates/agent-settings-reconcile、ADR-0024 / #414)。かつては
  # scripts/register-{codex,copilot}-hooks と、この下の writeShellScript に埋め込んだ
  # jq に分かれていた。サブコマンドごとに引数の形が違う(codex/copilot は旧 bash と
  # 同じ argv、claude-* は `builtins.toJSON` で作った宣言 JSON)。
  settingsReconcile = "${pkgs.dotfiles-tools}/bin/agent-settings-reconcile";
  registerCopilotHooks = "${settingsReconcile} copilot-hooks";
  # attribution-guard の Codex/Copilot 展開(#192)。#415(ADR-0024 Stage 4a)で
  # Rust バイナリ(crates/attribution-guard)になり、Claude と同じ
  # ~/.claude/hooks/attribution-guard を `--agent codex|copilot` 付きで直接
  # 登録する(pkexecGuardCodexCmd と同じ型)。旧 bash adapter の command は
  # register-{codex,copilot}-hooks の --retire で完全一致削除する(legacy*)。
  codexAttributionGuardCmd = "'${hooksDir}/attribution-guard' --agent codex";
  copilotAttributionGuardCmd = "'${hooksDir}/attribution-guard' --agent copilot";
  legacyCodexAttributionGuardCmd = "bash '${config.home.homeDirectory}/.codex/hooks/attribution-guard.sh'";
  legacyCopilotAttributionGuardCmd = "bash '${config.home.homeDirectory}/.copilot/hooks/attribution-guard.sh'";
  # #413: herdr metadata は Claude/Codex/Copilot の 3 本を 1 バイナリ
  # (crates/herdr-agent-metadata、--agent で入力差を吸収)にまとめた。Codex/
  # Copilot 側の登録は home/modules/herdr.nix。
  herdrMetadataCmd = "'${hooksDir}/herdr-agent-metadata'";
  legacyHerdrMetadataCmd = "bash '${hooksDir}/herdr-claude-metadata.sh'";
  worktreeFreshBaseCmd = "'${hooksDir}/worktree-base-sync'";
  herdrStatusAdvisoryCmd = "'${hooksDir}/herdr-status-advisory'";
  gateFailopenAdvisoryCmd = "'${hooksDir}/gate-failopen-advisory'";
  worktreeCreateGuardCmd = "'${config.home.homeDirectory}/.local/libexec/git-worktree-create-guard'";
  worktreeAuditContextCmd = "'${config.home.homeDirectory}/.local/bin/git-audit-worktrees' --context";
  planScopeGateCmd = "'${hooksDir}/plan-scope-gate'";
  planPrecedentGateCmd = "'${hooksDir}/plan-precedent-gate'";
  planFreshGateCmd = "'${hooksDir}/plan-fresh-gate'";
  legacyPlanReviewCmd = "bash '${hooksDir}/copilot-plan-review.sh'";
  legacyPlanViewCmd = "bash '${hooksDir}/plan-view.sh'";
  legacyPlanScopeGateCmd = "bash '${hooksDir}/plan-scope-gate.sh'";
  legacyPlanPrecedentGateCmd = "bash '${hooksDir}/plan-precedent-gate.sh'";
  legacyPlanFreshGateCmd = "bash '${hooksDir}/plan-fresh-gate.sh'";
  # #415(ADR-0024 Stage 4a): attribution-guard は Rust バイナリ(crates/
  # attribution-guard、判定エンジンは crates/github-command)になり、下の
  # home.file が ~/.claude/hooks/attribution-guard(拡張子無し)として配備する。
  # 旧 `bash '….sh'` の command は retiredHookEntries で完全一致削除する。
  # bash 版 attribution-guard.sh 自体は、下の stack-base-guard.sh 等がまだ
  # `source` するので配備を続ける(hook としては登録しない)。
  attributionGuardCmd = "'${hooksDir}/attribution-guard'";
  legacyAttributionGuardCmd = "bash '${hooksDir}/attribution-guard.sh'";
  # stack-base-guard(ADR-0027): #415(ADR-0024 Stage 4a)で Rust バイナリ
  # (crates/stack-base-guard)になり、下の home.file が
  # ~/.claude/hooks/stack-base-guard(拡張子無し)として配備する。旧 `bash
  # '….sh'` の command は retiredHookEntries で完全一致削除する。
  stackBaseGuardCmd = "'${hooksDir}/stack-base-guard'";
  legacyStackBaseGuardCmd = "bash '${hooksDir}/stack-base-guard.sh'";
  # pr-title-guard(ADR-0031)は #415(ADR-0024 Stage 4a)で Rust バイナリ
  # (crates/pr-title-guard、判定エンジンは crates/github-command と crates/
  # pr-title-check)になり、下の home.file が ~/.claude/hooks/pr-title-guard
  # (拡張子無し)として配備する。Codex/Copilot も同じパスを
  # `--agent codex|copilot` 付きで直接登録する(attribution-guard と同じ型)。
  # 旧 `bash '….sh'` の command は retiredHookEntries / register-{codex,
  # copilot}-hooks の --retire で完全一致削除する(legacy*)。
  prTitleGuardCmd = "'${hooksDir}/pr-title-guard'";
  codexPrTitleGuardCmd = "'${hooksDir}/pr-title-guard' --agent codex";
  copilotPrTitleGuardCmd = "'${hooksDir}/pr-title-guard' --agent copilot";
  legacyPrTitleGuardCmd = "bash '${hooksDir}/pr-title-guard.sh'";
  legacyCodexPrTitleGuardCmd = "bash '${config.home.homeDirectory}/.codex/hooks/pr-title-guard.sh'";
  legacyCopilotPrTitleGuardCmd = "bash '${config.home.homeDirectory}/.copilot/hooks/pr-title-guard.sh'";
  # pr-followup-guard(docs/agents/pr-followup-guard.md): #415(ADR-0024 Stage 4a)
  # で Rust バイナリ(crates/pr-followup-guard、判定エンジンは crates/github-command)
  # になり、下の home.file が ~/.claude/hooks/pr-followup-guard(拡張子無し)として
  # 配備する。旧 `bash '….sh'` の command は retiredHookEntries で完全一致削除する。
  prConfirmGuardCmd = "'${hooksDir}/pr-confirm-guard'";
  legacyPrConfirmGuardCmd = "bash '${hooksDir}/pr-confirm-guard.sh'";
  # Codex 展開も同じバイナリを `--agent codex` で直接呼ぶ(bash 時代の
  # ~/.codex/hooks/pr-confirm-guard.sh adapter は廃止、attribution-guard の
  # codexAttributionGuardCmd と同じ型)。旧 command は register-codex-hooks の
  # --retire で完全一致削除する。Copilot adapter は元から無い(依頼は Codex のみ)。
  codexPrConfirmGuardCmd = "'${hooksDir}/pr-confirm-guard' --agent codex";
  legacyCodexPrConfirmGuardCmd = "bash '${config.home.homeDirectory}/.codex/hooks/pr-confirm-guard.sh'";
  # git-stash-guard/stack-base-guard/adr-number の Codex adapter(ADR-0032
  # Amendment #531)。attribution-guard/pr-title-guard の Codex adapter と
  # 同じく ~/.codex/hooks/ 直下に置く(source 先の解決は #602 を参照)。
  # #416: Codex adapter(bash)は廃止し、同じ Rust バイナリを --host codex で
  # 直接登録する。
  codexGitStashGuardCmd = "'${hooksDir}/git-stash-guard' --host codex";
  # #415: stack-base-guard の Codex adapter(bash)も廃止し、Claude と同じ
  # Rust バイナリを --agent codex で直接登録する。旧 command は --retire で
  # 完全一致削除する。
  codexStackBaseGuardCmd = "'${hooksDir}/stack-base-guard' --agent codex";
  legacyCodexStackBaseGuardCmd = "bash '${config.home.homeDirectory}/.codex/hooks/stack-base-guard.sh'";
  # #415(ADR-0024 Stage 4a): adr-number も Rust バイナリ(crates/adr-number)に
  # なり、bash の Codex adapter は廃止。Claude と同じ ~/.claude/hooks/adr-number
  # を `--agent codex` 付きで直接登録し、旧 command は --retire で完全一致削除する。
  codexAdrNumberCmd = "'${hooksDir}/adr-number' --agent codex";
  legacyCodexAdrNumberCmd = "bash '${config.home.homeDirectory}/.codex/hooks/adr-number.sh'";
  # codex-plan-gate(ADR-0032 Amendment #531、docs/claude/codex-plan-gate.md):
  # Codex の Plan mode に ExitPlanMode 相当の機械検査を課す Stop hook。
  # 独立ファイル(他 hook を source しない)なので配置は
  # ~/.codex/hooks/ 直下のみで足りる。
  # #389 (ADR-0024 Stage 4b): Rust バイナリ(crates/codex-plan-gate)になり、
  # 下の home.file が ~/.codex/hooks/codex-plan-gate(拡張子無し)として配備する。
  # 旧 `bash '….sh'` の command は --retire で完全一致削除する。
  codexPlanGateCmd = "'${config.home.homeDirectory}/.codex/hooks/codex-plan-gate'";
  legacyCodexPlanGateCmd = "bash '${config.home.homeDirectory}/.codex/hooks/codex-plan-gate.sh'";
  # rulesets-write-guard(Rust)は Claude/Codex で入出力形が同一
  # (crates/agent-session の Agent::Claude | Agent::Codex が同じ JSON 形を emit
  # すると確認済み — crates/agent-session/src/decision.rs)なので adapter を
  # 挟まず、Claude と同じバイナリを直接 Codex にも登録する。
  codexRulesetsWriteGuardCmd = "'${config.home.homeDirectory}/.claude/hooks/rulesets-write-guard'";
  # decision-colocation-guard(ADR-396, docs/agents/decision-colocation.md)は
  # #415(ADR-0024 Stage 4a)で Rust バイナリ(crates/decision-colocation)に
  # なり、下の home.file が ~/.claude/hooks/decision-colocation-guard(拡張子
  # 無し)として配備する。旧 `bash '….sh'` の command は retiredHookEntries で
  # 完全一致削除する。Codex/Copilot adapter は意図的に作らない — CI required
  # check が全エージェント共通の backstop として機能するため(ADR-396 の
  # 非目標に明記)。
  decisionColocationGuardCmd = "'${hooksDir}/decision-colocation-guard'";
  legacyDecisionColocationGuardCmd = "bash '${hooksDir}/decision-colocation-guard.sh'";
  # adr-number(ADR-380, docs/agents/adr-numbering.md): 段3(利便性層)のみ —
  # deny は一切しない。#415 で Rust バイナリ(crates/adr-number、判定は
  # crates/github-command)になり、下の home.file が ~/.claude/hooks/adr-number
  # (拡張子無し)として配備する。旧 `bash '….sh'` は retiredHookEntries で削除。
  adrNumberCmd = "'${hooksDir}/adr-number'";
  legacyAdrNumberCmd = "bash '${hooksDir}/adr-number.sh'";
  # gh-edit-allow(#392, docs/agents/gh-edit-allow.md): Rust 製(crates/
  # gh-edit-allow、ADR-0024)。bash を挟まず実行ファイルを直接呼ぶ。配置は
  # 他の hook と同じ ~/.claude/hooks/ の安定パス — store path を command に
  # 直接書くと、ビルドのたびに command 文字列が変わり、完全一致で存在判定
  # する agent-settings-reconcile が旧エントリを残し続けるため。
  ghEditAllowCmd = "'${hooksDir}/gh-edit-allow'";
  # rulesets-write-guard(ADR-503 D7):
  # `gh api` による ruleset(required_status_checks 等)の直接書換を deny
  # する。gh-edit-allow と同じ理由(判定に既存の巨大 bash 資産を source
  # する必要が無い)で ADR-0024 の既定どおり Rust。
  rulesetsWriteGuardCmd = "'${hooksDir}/rulesets-write-guard'";
  # routines-write-guard(28番、ADR-519 D7):
  # 宣言外の cron routine create/update(RemoteTrigger)を deny する。
  # rulesets-write-guard と同じ理由で Rust だが、判定対象が Bash コマンド
  # 文字列ではないため env var による bypass は持たない(crates/
  # routines-write-guard/src/lib.rs 参照)。
  routinesWriteGuardCmd = "'${hooksDir}/routines-write-guard'";
  # pkexec-guard(29番、docs/agents/pkexec-guard.md、PR #326 のグリルセッション
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
  # `Target:` 行を要求する。#415(ADR-0024 Stage 4a)で Rust バイナリ(crates/
  # feedback-target-guard、判定エンジンは crates/github-command)になり、下の
  # home.file が ~/.claude/hooks/feedback-target-guard として配備する。旧
  # `bash '….sh'` の command は retiredHookEntries で完全一致削除する。
  feedbackTargetGuardCmd = "'${hooksDir}/feedback-target-guard'";
  legacyFeedbackTargetGuardCmd = "bash '${hooksDir}/feedback-target-guard.sh'";
  # repo-create-guard(ADR-0013 Amendment 2026-09-29, docs/claude/repo-create-
  # guard.md): `gh repo create` / `gh api -X POST user/repos`・`orgs/*/repos`
  # を deny し、repo-charter スキルの手順に強制的に載せる。#415(ADR-0024
  # Stage 4a)で Rust バイナリ(crates/repo-create-guard、判定エンジンは
  # crates/github-command)になり、下の home.file が ~/.claude/hooks/repo-create-guard
  # として配備する。旧 `bash '….sh'` の command は retiredHookEntries で完全一致削除する。
  repoCreateGuardCmd = "'${hooksDir}/repo-create-guard'";
  legacyRepoCreateGuardCmd = "bash '${hooksDir}/repo-create-guard.sh'";
  # cmd-hash-log(ADR-543 段3): 実行された Bash コマンドの正規化ハッシュ
  # だけを記録する(コマンド本文は書かない、ADR-0011 のプライバシー規約を
  # 踏襲)。逐語反復検出(段4 の promotion-detect)の入力。何も判定しない
  # ため ADR-0024 の既定どおり Rust だが、他 hook を deny/allow させる
  # 種類のものではない。
  cmdHashLogCmd = "'${hooksDir}/cmd-hash-log'";
  # external-send-guard(22番、docs/agents/external-send-guard.md): 外部宛
  # メールの直接送信を deny し create_draft へ誘導する。他 hook を source
  # しない独立ファイルだが、配置ディレクトリは揃えておく。
  externalSendGuardCmd = "'${hooksDir}/external-send-guard'";
  legacyExternalSendGuardCmd = "bash '${hooksDir}/external-send-guard.sh'";
  # agent-turn-log(UserPromptSubmit + Stop、docs/adr/0011): 1 スクリプトが
  # 2 イベントに同一 command で登録され、`.hook_event_name` で分岐する
  # (herdr-agent-metadata と同じ形)。出力は
  # ${XDG_STATE_HOME:-~/.local/state}/daily-report/agent-events.jsonl —
  # 別リポジトリ(daily-report)がそのまま読む契約なので、フィールド名は
  # 変更しないこと。
  agentTurnLogCmd = "'${hooksDir}/agent-turn-log'";
  legacyAgentTurnLogCmd = "bash '${hooksDir}/agent-turn-log.sh'";
  # Codex 向けは同じファイルを AGENT_NAME=codex だけ差し替えて直接登録する
  # (adapter ファイルを新設しない — ADR-0032 Amendment #531、D8)。
  codexAgentTurnLogCmd = "AGENT_NAME=codex '${hooksDir}/agent-turn-log'";
  legacyCodexAgentTurnLogCmd = "AGENT_NAME=codex bash '${hooksDir}/agent-turn-log.sh'";
  # Copilot 向けも同様(ADR-0032 Amendment 2026-10-01)。イベント名は PascalCase
  # (UserPromptSubmit/Stop)で登録する — camelCase だと payload が別形(sessionId、
  # hook_event_name なし)になり agent-turn-log が読めない。
  copilotAgentTurnLogCmd = "AGENT_NAME=copilot '${hooksDir}/agent-turn-log'";
  legacyCopilotAgentTurnLogCmd = "AGENT_NAME=copilot bash '${hooksDir}/agent-turn-log.sh'";
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

  # **この一覧に足すのは、記録が入る前の世代が書いた登録を外すときだけ**(2026-10-24)。
  # 宣言から外した hook は、agent-settings-reconcile が所有の記録(下の claudeHooksSpec の
  # `state`)で「前回書いたが今回の宣言に無いもの」として消す。ただし記録は、記録を入れた
  # 時点の宣言しか知らない: 記録の無いホストの最初の switch は、その前の世代が書いた登録を
  # 知らないので、そのとき宣言から外す登録(ルータに載せた 19 件、下)は、ここに挙げる。
  # 記録ができたあとに外す登録は、ここに足さない(記録が消す)。vega・arcturus・altair の
  # すべてに記録ができたら、この一覧と legacy*Cmd は消せる(確認は後続の Issue、#979)。なお `herdr-agent-state.sh`(herdr 自身が .hooks.SessionStart に
  # 書く統合 hook)は dotfiles の所有ではなく、ここにも記録にも載せない — 載せると
  # herdr の統合を壊す。
  #
  # かつて登録したが撤回した hook。activation が全ホストの settings.json から
  # (event, command) の組で完全一致削除する。撤回が宣言的にできる前は、hook
  # スクリプトを home.file から外すと settings.json のエントリだけが残り、消えた
  # パスを指したまま毎イベントで ENOENT を吐き続けた(#44 の実体 — herdr のリネーム
  # ではなく自リポジトリ由来。herdr が settings.json に書くのは
  # integration::claude_settings::rewrite による .hooks.SessionStart の
  # herdr-agent-state.sh だけで、statusLine にも他イベントにも触れない)。
  retiredHookEntries = [
    {
      event = "SessionStart";
      command = retiredAgentPermissionsStartCmd;
    }
    {
      event = "SessionEnd";
      command = retiredAgentPermissionsEndCmd;
    }
    # ルータ(hook-router)に載せた 19 件の旧登録(2026-10-24)。所有の記録が無いホスト(記録を
    # 入れた直後の最初の switch)は、前の世代が書いたこの 19 件を知らないので、ここに挙げて
    # 初回だけ消す。記録のあるホストは、記録(宣言から外れたもの)で消えるので、この項目は
    # 要らない。全ホストに記録ができたら、一覧ごと消せる(#979)。
    {
      event = "PreToolUse";
      command = gitStashGuardCmd;
    }
    {
      event = "PreToolUse";
      command = mainCheckoutGuardPreCmd;
    }
    {
      event = "PreToolUse";
      command = attributionGuardCmd;
    }
    {
      event = "PreToolUse";
      command = worktreeCreateGuardCmd;
    }
    {
      event = "PreToolUse";
      command = stackBaseGuardCmd;
    }
    {
      event = "PreToolUse";
      command = prTitleGuardCmd;
    }
    {
      event = "PreToolUse";
      command = prConfirmGuardCmd;
    }
    {
      event = "PreToolUse";
      command = decisionColocationGuardCmd;
    }
    {
      event = "PreToolUse";
      command = externalSendGuardCmd;
    }
    {
      event = "PostToolUse";
      command = adrNumberCmd;
    }
    {
      event = "PostToolUse";
      command = ghEditAllowCmd;
    }
    {
      event = "PreToolUse";
      command = rulesetsWriteGuardCmd;
    }
    {
      event = "PreToolUse";
      command = routinesWriteGuardCmd;
    }
    {
      event = "PreToolUse";
      command = pkexecGuardCmd;
    }
    {
      event = "PreToolUse";
      command = newToolGuardCmd;
    }
    {
      event = "PostToolUse";
      command = newToolGuardCmd;
    }
    {
      event = "PreToolUse";
      command = feedbackTargetGuardCmd;
    }
    {
      event = "PreToolUse";
      command = repoCreateGuardCmd;
    }
    {
      event = "PostToolUse";
      command = cmdHashLogCmd;
    }
    # 上の 19 件に続いて表に載せた、ExitPlanMode の拒否 2 件の旧登録(#991、2026-10-27)。
    # 同じ理由で、記録の無いホストの最初の switch のためにここへ挙げる。
    {
      event = "PreToolUse";
      command = planScopeGateCmd;
    }
    {
      event = "PreToolUse";
      command = planPrecedentGateCmd;
    }
    # 旧イテレーションは PostToolUse に herdr-claude-metadata.sh を登録していた。
    # 現行は PreToolUse で同じ情報を取る(遅延が小さい)ので、こちらは外す。
    {
      event = "PostToolUse";
      command = legacyHerdrMetadataCmd;
    }
    # Codex → Copilot 移行(docs/agents/copilot-plan-review.md)。旧 command 文字列を
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
    # #412 (ADR-0024 Stage 4b): ExitPlanMode の plan hook 5 本の bash 版 → Rust 版。
    {
      event = "PreToolUse";
      command = legacyPlanReviewCmd;
    }
    {
      event = "PreToolUse";
      command = legacyPlanViewCmd;
    }
    {
      event = "PreToolUse";
      command = legacyPlanScopeGateCmd;
    }
    {
      event = "PreToolUse";
      command = legacyPlanPrecedentGateCmd;
    }
    {
      event = "PreToolUse";
      command = legacyPlanFreshGateCmd;
    }
    # #413 (ADR-0024 Stage 4d): session/表示系 hook の bash 版 → Rust 版。理由は
    # 上の #416 と同じ(register() の存在判定が command の完全一致だけ)。
    {
      event = "Stop";
      command = legacyWrapupStopCmd;
    }
    {
      event = "SessionStart";
      command = legacyWrapupSessionStartCmd;
    }
    {
      event = "SessionStart";
      command = legacyIssueIndexCmd;
    }
    # #415 (ADR-0024 Stage 4a): pr-gate の bash 版 → Rust 版。
    {
      event = "SessionStart";
      command = legacyPrGateSessionStartCmd;
    }
    {
      event = "Stop";
      command = legacyPrGateStopCmd;
    }
    {
      event = "SessionStart";
      command = legacySignPrewarmCmd;
    }
    {
      event = "SessionStart";
      command = legacyHerdrMetadataCmd;
    }
    {
      event = "UserPromptSubmit";
      command = legacyHerdrMetadataCmd;
    }
    {
      event = "PreToolUse";
      command = legacyHerdrMetadataCmd;
    }
    {
      event = "Stop";
      command = legacyHerdrMetadataCmd;
    }
    {
      event = "SessionEnd";
      command = legacyHerdrMetadataCmd;
    }
    {
      event = "UserPromptSubmit";
      command = legacyAgentTurnLogCmd;
    }
    {
      event = "Stop";
      command = legacyAgentTurnLogCmd;
    }
    {
      event = "PreToolUse";
      command = legacyExternalSendGuardCmd;
    }
    # #415 (ADR-0024 Stage 4a): attribution-guard の bash 版 → Rust 版。
    {
      event = "PreToolUse";
      command = legacyAttributionGuardCmd;
    }
    # #415 (ADR-0024 Stage 4a): stack-base-guard の bash 版 → Rust 版。
    {
      event = "PreToolUse";
      command = legacyStackBaseGuardCmd;
    }
    # #415: adr-number の bash 版 → Rust 版。
    {
      event = "PostToolUse";
      command = legacyAdrNumberCmd;
    }
    # #415 (ADR-0024 Stage 4a): decision-colocation-guard の bash 版 → Rust 版。
    {
      event = "PreToolUse";
      command = legacyDecisionColocationGuardCmd;
    }
    # #415 (ADR-0024 Stage 4a): pr-title-guard の bash 版 → Rust 版。
    {
      event = "PreToolUse";
      command = legacyPrTitleGuardCmd;
    }
    # #415 (ADR-0024 Stage 4a): pr-followup-guard / feedback-target-guard /
    # repo-create-guard の bash 版 → Rust 版。
    {
      event = "PreToolUse";
      command = legacyPrConfirmGuardCmd;
    }
    {
      event = "PreToolUse";
      command = legacyFeedbackTargetGuardCmd;
    }
    {
      event = "PreToolUse";
      command = legacyRepoCreateGuardCmd;
    }
  ];

  # かつて配って撤回した statusLine。agent-settings-reconcile claude-statusline が .statusLine.command との
  # 完全一致でキーを削除する対象(その後 statusLineCmd を set するので、置き換えにも
  # 使える)。#413: bash 版 claude-statusline.sh → Rust 版の置き換え。将来この機能
  # 自体を取り下げるときは statusLineCmd もここへ移す。

  # Opus Plan Mode のモデル実体 — 具体値は claude-plan-model(crates/claude-plan-model)が持つ。
  #
  # `model: "opusplan"` は Plan 中に opus エイリアス、実行中に sonnet エイリアスを
  # 解決する。各エイリアスの解決先は settings.json の
  # `.env.ANTHROPIC_DEFAULT_{OPUS,SONNET}_MODEL` で乗っ取れるので、モードは
  # (Plan 側, 実行側) のペアになる — opus/sonnet(既定)・fable/sonnet・fable/opus。
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

  # settings.json に登録する hook の宣言(順序がそのまま settings.json 上の並び)。
  # agent-settings-reconcile claude-hooks が (event, command) をキーに、宣言を正として
  # matcher / if / timeout まで合わせる(かつての register() は command 一致だけで
  # 存在判定し、これらを後から変えても既存エントリが更新されなかった — #414)。
  # matcher / timeout / if は省略するとフィールド自体を出力しない。if はハンドラ
  # レベルの絞り込み(permission rule 構文、例: "Bash(git -C *)")で、マッチしない
  # 呼び出しでは hook プロセス自体が spawn されない。
  claudeHookDeclarations = [
    {
      event = "PreToolUse";
      node = "copilot-plan-review";
      matcher = "ExitPlanMode";
      command = planReviewCmd;
      timeout = 300;
    }
    {
      event = "Stop";
      node = "session-wrapup-stop";
      command = wrapupStopCmd;
    }
    {
      event = "SessionStart";
      node = "session-wrapup-start";
      command = wrapupSessionStartCmd;
    }
    # plan-view は plan-review gate と同じ matcher に、別エントリとして並ぶ。
    # Claude Code は同一 matcher の hook を並列に走らせるので、review の結果を
    # 待たずに窓が開く（= 表示は gate から独立している）。timeout は短く: この
    # hook は pandoc とプロセス fork しかせず、ブラウザの終了は待たない。
    {
      event = "PreToolUse";
      node = "plan-view";
      matcher = "ExitPlanMode";
      command = planViewCmd;
      timeout = 15;
    }
    # plan-scope-gate・plan-precedent-gate(ExitPlanMode の拒否)は、ここに登録しない。
    # 下の hook-router の表に載せた(#991、docs/agents/hook-router.md)。plan-view・plan-fresh-gate・
    # copilot-plan-review は、副作用か待ち時間のために、ここに残す。
    # plan-fresh-gate も同じ matcher に並ぶ。fetch 1 回
    # (timeout 15s)+ diff/grep/awk のみで gh は呼ばない。
    {
      event = "PreToolUse";
      node = "plan-fresh-gate";
      matcher = "ExitPlanMode";
      command = planFreshGateCmd;
      timeout = 30;
    }
    # issue-index は startup/resume/compact でだけ発火する。clear は「文脈を捨てたい」
    # という利用者の意思表示なので外す。compact は逆に文脈を続けたい表示であり、
    # 要約で索引が落ちている可能性が高く再注入の価値が最も高い(autoCompactEnabled
    # は off なので発火は手動 /compact 時のみ)。fork は元セッションの文脈を
    # 引き継ぐので不要。
    {
      event = "SessionStart";
      node = "issue-index";
      matcher = "startup|resume|compact";
      command = issueIndexCmd;
      timeout = 10;
    }
    # esa-mirror-context も同じ matcher: compact では要約で所在の注入が落ちている可能性が
    # 高く、再注入の価値が最も高い(根拠は上の issue-index と同じ)。
    {
      event = "SessionStart";
      node = "esa-mirror-context";
      matcher = "startup|resume|compact";
      command = esaIndexCmd;
      timeout = 10;
    }
    # daily-desk-reminder は startup だけで発火する: 机の未実施は新しいセッションの入口で一度伝えれば
    # 足り、resume・compact で繰り返すと同じ日に何度も出る(印が無い限り startup ごとに出る)。
    {
      event = "SessionStart";
      node = "daily-desk-reminder";
      matcher = "startup";
      command = deskStartCmd;
      timeout = 10;
    }
    # worklog も startup だけ: 規範の再掲は新しいセッションの入口で足りる。resume・compact では
    # 文脈に既にあるか、要約で落ちても書く場面(試した結果・詰まり)は次に来る。
    {
      event = "SessionStart";
      node = "worklog";
      matcher = "startup";
      command = worklogStartCmd;
      timeout = 10;
    }
    # gpg-prewarm は compact を含めない: 同一プロセス内の事象なので agent の
    # キャッシュはすでに温まっているか、そもそもまだ温まっていないかのどちらか
    # であり、compact での再発火は温度判定で黙って no-op になるだけで発火の価値が
    # ない。resume は別ログインからの再開があり得るので含める。
    {
      event = "SessionStart";
      node = "gpg-prewarm";
      matcher = "startup|resume";
      command = signPrewarmCmd;
      timeout = 120;
    }
    # worktree-base-sync: pristine な worktree だけを origin/<base> へ黙って
    # fast-forward する。pr-gate の SessionStart advisory(base 追従)より先に
    # 列挙しているが、Claude Code は同一イベントの hook を並列実行するため
    # 逐次を保証しない — レースは許容し、動かした場合だけこの hook 自身が
    # additionalContext で報告する(docs/agents/worktree-base-sync.md)。
    {
      event = "SessionStart";
      node = "worktree-base-sync";
      matcher = "startup|resume";
      command = worktreeFreshBaseCmd;
      timeout = 30;
    }
    # herdr-status-advisory: herdr の client と server の版が食い違っているときだけ、
    # 作業ツリーを切れなくなる前に additionalContext で知らせる。`herdr status
    # --json` を呼んで読むだけで、何も実行しない(docs/agents/herdr-status-advisory.md)。
    {
      event = "SessionStart";
      node = "herdr-status-advisory";
      matcher = "startup|resume";
      command = herdrStatusAdvisoryCmd;
      timeout = 10;
    }
    # gate-failopen-advisory: fail-open で素通りし続けている gate を、セッション開始時に
    # additionalContext で知らせる。gate-events.jsonl を読むだけで、何も実行しない
    # (docs/agents/gate-failopen-advisory.md、#822)。
    {
      event = "SessionStart";
      node = "gate-failopen-advisory";
      matcher = "startup|resume";
      command = gateFailopenAdvisoryCmd;
      timeout = 10;
    }
    # pr-gate: SessionStart は状態の一覧取得のみ(短時間)。Stop は CI の
    # --watch --fail-fast を timeout 300s 付きで自前で回すので、hook の timeout は
    # それより長く確保する(かつては command 一致だけで存在判定していたので
    # matcher/timeout を後から変えても既存エントリが更新されなかったが、今は
    # agent-settings-reconcile が宣言に合わせて直す — docs/agents/issue-index.md、#414)。
    {
      event = "SessionStart";
      node = "pr-gate-session-start";
      command = prGateSessionStartCmd;
      timeout = 10;
    }
    {
      event = "Stop";
      node = "pr-gate-stop";
      command = prGateStopCmd;
      timeout = 600;
    }
    # git-worktree-allow: if で "Bash(git -C *)" に絞る — それ以外の Bash 呼び出しでは
    # hook プロセス自体が起動しない。判定はすべて hook 側(パス実在・許可サブコマンド・
    # 単一 git 呼び出し)で行い、非該当は無出力 exit 0 で通常の permission フローに
    # フォールスルーする。
    {
      event = "PreToolUse";
      node = "git-worktree-allow";
      matcher = "Bash";
      command = gitWorktreeAllowCmd;
      timeout = 10;
      "if" = "Bash(git -C *)";
    }
    # hook-router(crates/hook-router、docs/agents/hook-router.md): ツール呼び出しに付く検査のうち、
    # 拒否だけを出すもの(18 件)と記録役(4 件)を、1 つのプロセスで、規則の表
    # (crates/hook-router/src/rules.rs)に従って走らせる。matcher は付けない — どのツールに
    # どの検査を当てるかは表(`Tools` という閉じた型)が持つ。matcher の文字列の書き間違いで検査が
    # 黙って素通りになる事故(旧登録の注意書きに、複合 matcher `Bash|mcp__.*` の実例があった)を、
    # 表に載せた検査については起こりえなくする。timeout は、表の規則ごとの許容(最大 20 秒)に、
    # プロセスの起動と出力の余裕を足した値。許可(allow)を出す検査(git-worktree-allow・
    # gh-edit-allow の判定役)は、`if` の評価を Claude Code に任せるので、載せていない。
    {
      event = "PreToolUse";
      node = "hook-router-pre";
      command = hookRouterPreCmd;
      timeout = 30;
    }
    {
      event = "PostToolUse";
      node = "hook-router-post";
      command = hookRouterPostCmd;
      timeout = 30;
    }
    # 事後検出: Bash の sed -i やリダイレクトなど、字面からは書き込みと
    # 分からない迂回(#661)を、触れた checkout の状態差分で捕まえる。
    {
      event = "Stop";
      node = "main-checkout-guard-stop";
      command = mainCheckoutGuardStopCmd;
      timeout = 30;
    }
    # bleep(旧 publish-guard、上流分離、ADR-0009): 会社/private リポジトリの
    # 実名が git push・gh pr/issue の create/edit/comment・MCP tool call 経由で
    # PUBLIC な面に漏れるのを防ぐ(docs/agents/public-publish-guard.md)。
    # matcher は複合1本 "Bash|mcp__.*" — Bash と MCP を2つの hook エントリに
    # 分けない。(event, command) が宣言の一意キーなので、同一 command を
    # 2つの matcher で宣言しても 2つ目は無視され(agent-settings-reconcile が警告する)、
    # MCP 経路が無検査のまま残ってしまう(旧実装が matcher "Bash" 単体だった
    # ために持っていた最大の機能欠陥そのもの)。
    # if は付けない(Bash(*) のような permission rule 構文は MCP の tool 名
    # には一致しないため) — git-stash-guard と同じ理由(deny/ask 側は
    # matcher/if 不一致 = 素通りが事故になる)で、絞り込みは hook 内部の
    # 早期 exit に置く。gh api の生呼び出しの往復も想定してタイムアウトは
    # やや長め。
    {
      event = "PreToolUse";
      node = "bleep";
      matcher = "Bash|mcp__.*";
      command = bleepClaudeCmd;
      timeout = 20;
    }
    # herdr-claude-metadata は permission mode の遷移を Herdr サイドバーに流す。
    # 同一 command を 5 イベントに登録する(スクリプト側が hook_event_name で分岐):
    # SessionStart=初期値+残留上書き / UserPromptSubmit=アイドル中の Shift+Tab を
    # 1 プロンプト 1 回で拾う / PreToolUse=plan 承認→acceptEdits を最小遅延で拾う
    # (スクリプト側の前回値キャッシュで、モードが同じなら jq 1 回で即抜ける)/
    # Stop=ttl リフレッシュ / SessionEnd=トークン全クリア。PostToolUse は
    # PreToolUse と同情報で遅延だけ悪いので登録しない(旧イテレーションは
    # PostToolUse に登録していたので retiredHookEntries で外す)。Herdr 外では
    # 即 no-op。
    {
      event = "SessionStart";
      node = "herdr-agent-metadata";
      command = herdrMetadataCmd;
      timeout = 10;
    }
    {
      event = "UserPromptSubmit";
      node = "herdr-agent-metadata";
      command = herdrMetadataCmd;
      timeout = 10;
    }
    {
      event = "PreToolUse";
      node = "herdr-agent-metadata";
      command = herdrMetadataCmd;
      timeout = 10;
    }
    {
      event = "Stop";
      node = "herdr-agent-metadata";
      command = herdrMetadataCmd;
      timeout = 10;
    }
    {
      event = "SessionEnd";
      node = "herdr-agent-metadata";
      command = herdrMetadataCmd;
      timeout = 10;
    }
    # The timer is the primary detector; SessionStart also exposes pending state
    # directly to the agent that is in a position to clean it up.
    {
      event = "SessionStart";
      node = "worktree-audit-context";
      matcher = "startup|resume";
      command = worktreeAuditContextCmd;
      timeout = 30;
    }
    # agent-turn-log(docs/adr/0011): 純粋なロガーで判定を持たないため matcher/if
    # は不要。UserPromptSubmit/Stop の両方に同一 command で登録し、スクリプト側が
    # `.hook_event_name` で分岐する(herdr-agent-metadata と同じ形)。
    {
      event = "UserPromptSubmit";
      node = "agent-turn-log";
      command = agentTurnLogCmd;
      timeout = 10;
    }
    {
      event = "Stop";
      node = "agent-turn-log";
      command = agentTurnLogCmd;
      timeout = 10;
    }
    # atuin hook claude-code(docs/adr/0011): atuin 自身が提供するエージェント
    # フックを、Bash tool の PreToolUse/PostToolUse/PostToolUseFailure 3 イベント
    # すべてに同一 command で登録する(成功/失敗どちらの exit code も history.db
    # に残すため、PostToolUse だけでは足りない)。matcher は Claude Code 標準の
    # tool 名一致("Bash")であり、他 hook が使う permission-rule 構文の `if`
    # ではない。
    {
      event = "PreToolUse";
      node = "atuin-history";
      matcher = "Bash";
      command = atuinHookClaudeCodeCmd;
      timeout = 10;
    }
    {
      event = "PostToolUse";
      node = "atuin-history";
      matcher = "Bash";
      command = atuinHookClaudeCodeCmd;
      timeout = 10;
    }
    {
      event = "PostToolUseFailure";
      node = "atuin-history";
      matcher = "Bash";
      command = atuinHookClaudeCodeCmd;
      timeout = 10;
    }
    {
      event = "PreToolUse";
      node = "gh-edit-allow-judge";
      matcher = "Bash";
      command = ghEditAllowCmd;
      timeout = 10;
      "if" = "Bash(gh *)";
    }
  ];

  # 各宣言の `node` は pipeline-map.toml(自動発火する機構の地図、docs/pipeline-map.md)
  # のノード id を指すポインタ。nix の評価時に「node がある」「manifest に定義済み」を
  # 保証するので、ポインタの無い hook 宣言は適用(hms)に至らない。agent-settings-reconcile の
  # 宣言(Register)は未知のキーを拒否するため、`node` は渡す前に取り除く。逆向き(auto な
  # ノードに登録が 1 つも無い)は crates/pipeline-map の cargo test が見る。
  pipelineNodeIds = config.dotfiles.pipeline.nodeIds;
  declareHook =
    d:
    assert lib.assertMsg (
      d ? node
    ) "claudeHookDeclarations: node が無い宣言がある (event = ${d.event}, command = ${d.command})";
    assert lib.assertMsg (builtins.elem d.node pipelineNodeIds)
      "claudeHookDeclarations: node `${d.node}` が pipeline-map.toml に定義されていない";
    builtins.removeAttrs d [ "node" ];

  # Codex の `/hooks` は、statusMessage の無い hook をすべて `Unnamed hook` と表示する(#1122)。
  # 表示名は pipeline-map.toml のノードの label(ASCII)を使う。Claude の宣言には付けない
  # (Claude は statusMessage をフック実行中の表示にも使う)。
  declareCodexHook =
    d: declareHook d // { statusMessage = config.dotfiles.pipeline.nodeLabels.${d.node}; };

  claudeHooksSpec = builtins.toJSON {
    retire = retiredHookEntries;
    register = map declareHook claudeHookDeclarations;
    # 所有の記録(crates/agent-settings-reconcile の managed)。前回書いた (event, command) の集合を
    # ここに残し、次の switch で宣言から外れたものを settings.json から消す。
    state = "${config.home.homeDirectory}/.local/state/dotfiles/managed-hooks-claude.json";
  };

  # Codex derives its registrations from the same declarations. Only protocol
  # differences are mapped here; a newly added shared hook cannot be forgotten.
  # (plan-scope-gate・plan-precedent-gate はルータの表にあり、Codex には ExitPlanMode が無いので
  # `pre --agent codex` では当たらない。Codex の計画の検査は codex-plan-gate の Stop が担う。)
  codexPlanNodes = [
    "copilot-plan-review"
    "plan-view"
    "plan-fresh-gate"
  ];
  codexHookDeclarations =
    lib.concatMap (
      d:
      assert
        !(d ? "if")
        || builtins.elem d.node [
          "git-worktree-allow"
          "gh-edit-allow-judge"
        ];
      if builtins.elem d.node codexPlanNodes then
        [ ]
      else if d.node == "herdr-agent-metadata" then
        [ ] # herdr.nix owns the agent-specific metadata lifecycle
      else if
        builtins.elem d.node [
          "git-worktree-allow"
          "gh-edit-allow-judge"
        ]
      then
        [ ] # Native Auto-review must receive approvals; retain Claude Allow hooks.
      else if d.event == "PostToolUseFailure" then
        assert d.node == "atuin-history";
        [ ] # Codex reports failed commands through PostToolUse
      else
        [
          (
            (builtins.removeAttrs d [ "if" ])
            // (
              if d.node == "hook-router-pre" then
                {
                  command = "${hookRouterPreCmd} --agent codex";
                }
              else if d.node == "hook-router-post" then
                {
                  command = "${hookRouterPostCmd} --agent codex";
                }
              else if d.node == "bleep" then
                { command = bleepCodexCmd; }
              else if d.node == "main-checkout-guard-stop" then
                { command = codexMainCheckoutGuardStopCmd; }
              else if d.node == "session-wrapup-start" then
                { command = codexWrapupSessionStartCmd; }
              else if d.node == "session-wrapup-stop" then
                { command = codexWrapupStopCmd; }
              else if d.node == "agent-turn-log" then
                { command = codexAgentTurnLogCmd; }
              else if d.node == "atuin-history" then
                { command = "'${hooksDir}/hook-router' atuin --agent codex"; }
              else
                { }
            )
          )
        ]
    ) claudeHookDeclarations
    ++ [
      {
        event = "Stop";
        node = "codex-plan-gate";
        command = codexPlanGateCmd;
        timeout = 400;
      }
    ];
  codexHooksSpec = builtins.toJSON {
    state = "${config.home.homeDirectory}/.local/state/dotfiles/managed-hooks-codex.json";
    register = map declareCodexHook codexHookDeclarations;
    retire = [
      {
        event = "PermissionRequest";
        command = "'${hooksDir}/hook-router' permission --agent codex";
      }
      {
        event = "SessionStart";
        command = retiredAgentPermissionsStartCmd;
      }
      {
        event = "SessionEnd";
        command = retiredAgentPermissionsEndCmd;
      }
    ]
    ++ (map
      (command: {
        event = "PreToolUse";
        inherit command;
      })
      [
        legacyBleepCodexCmdUnprefixed
        legacyPublishGuardCodexAdapterCmd
        codexAttributionGuardCmd
        legacyCodexAttributionGuardCmd
        codexPrTitleGuardCmd
        legacyCodexPrTitleGuardCmd
        pkexecGuardCodexCmd
        codexGitStashGuardCmd
        legacyCodexGitStashGuardCmd
        codexStackBaseGuardCmd
        legacyCodexStackBaseGuardCmd
        codexRulesetsWriteGuardCmd
        codexMainCheckoutGuardPreCmd
        codexPrConfirmGuardCmd
        legacyCodexPrConfirmGuardCmd
        "'${config.home.homeDirectory}/.local/libexec/git-worktree-create-guard'"
      ]
    )
    ++ (map
      (command: {
        event = "PostToolUse";
        inherit command;
      })
      [
        codexAdrNumberCmd
        legacyCodexAdrNumberCmd
      ]
    )
    ++ (map
      (command: {
        event = "SessionStart";
        inherit command;
      })
      [
        wrapupSessionStartCmd
        legacyWorktreeFreshBaseCmd
        legacyIssueIndexCmd
        legacyWrapupSessionStartCmd
        legacySignPrewarmCmd
        legacyPrGateSessionStartCmd
      ]
    )
    ++ (map
      (command: {
        event = "Stop";
        inherit command;
      })
      [
        priorCodexWrapupStopCmd
        legacyCodexWrapupStopCmd
        legacyPrGateStopCmd
        legacyCodexAgentTurnLogCmd
        legacyCodexPlanGateCmd
      ]
    )
    ++ [
      {
        event = "UserPromptSubmit";
        command = legacyCodexAgentTurnLogCmd;
      }
    ];
  };

  # statusLine / permissions の reconcile も agent-settings-reconcile(crates/
  # agent-settings-reconcile、#414)。かつてここに writeShellScript で埋め込んでいた
  # jq の設計メモ(振る舞いは Rust 版のコメントと fixture に引き継いだ):
  #
  # - claude-statusline: retired を先に処理し、.statusLine.command が retired の
  #   いずれかに完全一致したときだけキーを削除する。無条件 del にしないのは、
  #   /statusline で本人が設定した値を「宣言なし」の switch で奪わないため —
  #   retiredPermissionRules が「かつて自分が配った文字列」だけを消すのと同型。
  #   desired が非空なら最後に set-if-different するので、retire→set の順で
  #   set が勝つ。
  # - claude-permissions: permissions.allow / ask を要素単位で冪等に同期する。
  #   retire_* のルールは削除(無ければ何もしない)、allow / ask は無ければ追加。
  #   ルール文字列を後から書き換えるときは旧文字列を retiredPermissionRules に
  #   移す — これで全ホストが次回の switch で旧ルールを掃除する
  #   (docs/claude/claude-permissions.md)。permissions.defaultMode や
  #   allow / ask 以外のキーには一切触らない。ルール1件ごとに read-modify-write
  #   を回すと、herdr-agent-state.sh や Claude Code CLI 自体との並行書き込みに
  #   対する lost-update 窓がルール数分だけ反復される(#61)ので、全ルールを
  #   1 回の読み書きにまとめる。pkexec-guard(29番、docs/agents/pkexec-guard.md)
  #   向けに ask 側(`retire_ask` / `ask`)もある。`.permissions.ask` は明示 ask
  #   ルールで、auto mode の分類器より先に評価され常に確認を強制する。allow と
  #   違い「常に許可」を意味しないため、#461 の中間ワイルドカード strip(検出
  #   のみで表現不可能にできない理由はそちらのコメント参照)は ask には適用
  #   しない — 末尾 `*` のみの単純なルールしか今のところ書いていない。
  # - #461: 中間 `*` を含む allow rule は settings.json の実体からも一律 strip
  #   する(宣言側は permissionRules の nix eval 時 assert で表現不可能にして
  #   あるが、settings.json は Claude Code 自身の「常に許可」プロンプトや
  #   /promote-permissions からも書かれる実行時の可変状態なので、宣言側の防御
  #   だけでは届かない — こちらは検出のみが上限)。判定は `hasMidWildcard`
  #   (下)と同じ意図: `(` か空白の直後の `*` に、空白を挟んで `)` 以外の文字が
  #   続くパターン。

  # Claude Code の permission rule 構文は `Tool(specifier)`(裸のコマンド文字列では
  # 認識されない)。Add / Commit / Create PR で毎回止まる直接原因はこの 4 件。
  # 破壊的操作は増やさない — 読み取り・検査系のみ追加する。gh の書き込み系
  # (pr edit / issue create / issue edit)はワイルドカードでは入れない —
  # `Bash(gh pr edit *)` は他人の PR の編集まで許してしまうため。代わりに
  # gh-edit-allow hook(#392、docs/agents/gh-edit-allow.md)が「このセッションが
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
  # (表現不可能)、settings.json という実行時の可変状態は agent-settings-reconcile
  # claude-permissions が一律 strip する(検出のみが上限、上のコメント参照)。

  # Derive the check from the source tree; no second list of skill names.
  skillRoot = repoConfig + "/agents/skills";
  skillNames = builtins.filter (name: builtins.pathExists (skillRoot + "/${name}/SKILL.md")) (
    builtins.attrNames (builtins.readDir skillRoot)
  );
  skillNameMatches =
    name:
    let
      lines = lib.splitString "\n" (builtins.readFile (skillRoot + "/${name}/SKILL.md"));
      declared = lib.findFirst (line: lib.hasPrefix "name: " line) "" lines;
    in
    declared == "name: ${name}";

  # Build regular-file trees before Home Manager links each skill directory.
  # Refer to the complete source root so relative template links can resolve
  # repo-governance-common while cp dereferences them.
  governanceSkillNames = [
    "rust-repo-governance"
    "typst-repo-governance"
    "astro-site-governance"
  ];
  browserReferenceSkillNames = [
    "browser-login-handoff"
    "pr-description"
    "venue-search"
    "inbox-adjudication"
  ];
  ciReferenceSkillNames = governanceSkillNames ++ [
    "inbox-adjudication"
    "pr-description"
    "stacked-pr"
    "github-audit-triage"
  ];
  skillDistributionNames = skillNames ++ [ "repo-governance-common" ];
  skillDistributions = lib.genAttrs skillDistributionNames (
    name:
    pkgs.runCommand "agent-skill-${name}" { } ''
      mkdir -p "$out"
      cp -rL ${skillRoot}/${name}/. "$out/"
      chmod -R u+w "$out"
      ${lib.optionalString (builtins.elem name governanceSkillNames) ''
        mkdir -p "$out/scripts" "$out/templates/.github/rulesets"
        cp ${skillRoot}/repo-governance-common/scripts/{setup-hooks,apply-repo-settings}.sh "$out/scripts/"
        for rule in ${skillRoot}/repo-governance-common/templates/.github/rulesets/*.json; do
          target="$out/templates/.github/rulesets/$(basename "$rule")"
          if [ ! -e "$target" ]; then cp "$rule" "$target"; fi
        done
      ''}
      ${lib.optionalString (builtins.elem name browserReferenceSkillNames) ''
        mkdir -p "$out/reference"
        cp ${repoConfig}/agents/reference/browser-operations.md "$out/reference/"
      ''}
      ${lib.optionalString (builtins.elem name ciReferenceSkillNames) ''
        mkdir -p "$out/reference"
        cp ${skillRoot}/repo-governance-common/reference/ci-failures.md "$out/reference/"
      ''}
      # A distribution must not retain links back to the source tree.
      test -z "$(find "$out" -type l -print -quit)"
    ''
  );
  skillFiles = lib.listToAttrs (
    lib.concatMap
      (
        root:
        map
          (name: {
            name = "${root}/${name}";
            value = {
              source = skillDistributions.${name};
              recursive = false;
            };
          })
          (
            builtins.filter (
              name: root == ".claude/skills" || name != "browser-login-handoff"
            ) skillDistributionNames
          )
      )
      [
        ".claude/skills"
        ".agents/skills"
      ]
  );

in
assert lib.assertMsg (lib.all skillNameMatches skillNames)
  "Agent skill names must match their source directory";
{
  # Merge generated skill entries with the explicit hook/config declarations.
  imports = [
    { home.file = skillFiles; }
    {
      options.dotfiles.feedbackTrial.collector.enable =
        lib.mkEnableOption "the host-local Codex feedback trial collector";
    }
  ];

  # Only the selected host collects its saved conversations. Trial destinations
  # and approval remain private runtime state, never Nix-store values.
  systemd.user.services.feedback-trial-collect =
    lib.mkIf (pkgs.stdenv.isLinux && config.dotfiles.feedbackTrial.collector.enable)
      {
        Unit.Description = "Collect local Codex feedback trial observations";
        Service = {
          Type = "oneshot";
          ExecStart = "${pkgs.dotfiles-tools}/bin/feedback-trial collect";
          # Bypass the interactive credential launcher: collection gets the
          # native Codex binary and never needs GitHub credentials.
          Environment = "FEEDBACK_TRIAL_CODEX_BIN=${config.home.homeDirectory}/.local/bin/codex";
          TimeoutStartSec = "10min";
        };
      };

  dotfiles.pipeline.timerNodes.feedback-trial-collect = "feedback-trial-collect";
  systemd.user.timers.feedback-trial-collect =
    lib.mkIf (pkgs.stdenv.isLinux && config.dotfiles.feedbackTrial.collector.enable)
      {
        Unit.Description = "Collect Codex feedback trial observations every half hour";
        Timer = {
          OnCalendar = "*-*-* *:00,30:00";
          Persistent = true;
          Unit = "feedback-trial-collect.service";
        };
        Install.WantedBy = [ "timers.target" ];
      };

  # plan-review gate の deny 対象 severity とラウンド上限をこの環境向けに再校正する。
  # 既定(BLOCKER,MAJOR / 3 ラウンド)は実測 deny 率 69%、3 ラウンド到達が中位という
  # 結果で、review の価値より摩擦が勝っていた。MAJOR は backlog へ落として報告のみに
  # し、ラウンドも 2 に絞る。gate 本体(crates/copilot-plan-review)は触らない — closer
  # ラウンドが judge() に空文字を渡して「gate 適格 severity なし」を表現する不変条件
  # (bash 版では `${3-$GATE_SEVERITIES}` のコロンなしデフォルト、Rust 版では
  # judge へ渡す空の gate 集合)に影響しないよう、値は env 経由
  # でのみ渡す。sessionVariables は次回ログインから効く。詳細は
  # docs/agents/copilot-plan-review.md の環境変数節。
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

  home.file.".claude/hooks/copilot-plan-review".source =
    "${pkgs.dotfiles-tools}/bin/copilot-plan-review";

  # critic の出力契約(文書兼 jq validator の参照用)。GitHub Copilot CLI には
  # Codex の `exec --output-schema` に相当する強制出力スキーマ機構が無いため、
  # 実際の検証は hook 内の CRITIC_SCHEMA_JQ が行う — この JSON はその契約を
  # 人間 / プロンプト向けに文書化したものである。配備先は hook と同じ
  # ~/.claude/hooks/ に並べて置く(hook が自身のディレクトリ相対で解決するため)
  # が、ソースツリー上は非 hook アセットとして config/agents/assets/ に分離
  # している(ADR-0007)。
  home.file.".claude/hooks/copilot-plan-review.schema.json".source =
    repoConfig + "/agents/assets/copilot-plan-review.schema.json";

  # plan-reviewer: copilot-plan-review が `--agent plan-reviewer` で呼ぶ
  # read-only custom agent。tools は view/grep/glob だけで、
  # write/execute/web/GitHub MCP は与えない(docs/agents/copilot-plan-review.md)。
  home.file.".copilot/agents/plan-reviewer.agent.md".source =
    repoConfig + "/copilot/agents/plan-reviewer.agent.md";

  # wrap-up inbox の 2 hook。session-start は stop-gate と同じパス計算を使い、
  # 同じディレクトリに並んでいることを前提に stop-gate のパスを指示文に埋める。
  # #413: どちらも crates/wrapup-stop-gate の bin。指示文に埋める自身のパスは argv[0]
  # 由来(store path ではなくこの安定パス)。
  home.file.".claude/hooks/wrapup-stop-gate".source = "${pkgs.dotfiles-tools}/bin/wrapup-stop-gate";
  home.file.".claude/hooks/wrapup-session-start".source =
    "${pkgs.dotfiles-tools}/bin/wrapup-session-start";

  # plan-view: プランを HTML にして Chrome の専用窓に飛ばす hook + CLI。
  # バイナリ(crates/plan-view)は CSS を起動パス(argv[0]、シンボリックリンクは
  # 解決しない)のディレクトリ相対で解決するので、配備先では 2 ファイルを
  # ~/.claude/hooks/ に並べて置く。ソースツリー上は config/agents/assets/ に
  # 分離している(ADR-0007)。$HOME/.claude/hooks/plan-view.css も最後の候補として
  # 探すので、~/.local/bin/plan-view からの起動でも解決する。
  home.file.".claude/hooks/plan-view".source = "${pkgs.dotfiles-tools}/bin/plan-view";
  home.file.".claude/hooks/plan-view.css".source = repoConfig + "/agents/assets/plan-view.css";

  # agent-turn-log: UserPromptSubmit / Stop の1ターン境界を JSONL 追記する
  # 純粋なロガー(ゲートではない)。出力契約は docs/adr/0011。
  home.file.".claude/hooks/agent-turn-log".source = "${pkgs.dotfiles-tools}/bin/agent-turn-log";

  # plan-scope-gate: 要求インベントリ(requirements-inventory、15番)の脱落を機械検査する。
  # plan-review / plan-view と同じ matcher に 3 つ目のエントリとして並ぶ。
  home.file.".claude/hooks/plan-scope-gate".source = "${pkgs.dotfiles-tools}/bin/plan-scope-gate";

  # plan-precedent-gate: 先行例との対比(precedent-grounding、16番、ADR-0012)の
  # 脱落を機械検査する。同じ matcher に 4 つ目のエントリとして並ぶ。
  home.file.".claude/hooks/plan-precedent-gate".source =
    "${pkgs.dotfiles-tools}/bin/plan-precedent-gate";

  # plan-fresh-gate: ExitPlanMode 直前に origin/<base> の進行を検出し、プラン
  # 参照ファイルと交差するときだけ deny する(docs/agents/plan-fresh-gate.md)。
  # 同じ matcher に 5 つ目のエントリとして並ぶ。
  home.file.".claude/hooks/plan-fresh-gate".source = "${pkgs.dotfiles-tools}/bin/plan-fresh-gate";

  # attribution-guard: Claude が GitHub に書く外向きテキストに attribution
  # フッターを強制する(docs/agents/attribution-guard.md)。#415 で Rust
  # バイナリ(crates/attribution-guard)への安定パスの symlink になった
  # (gh-edit-allow と同じ理由付け)。Codex/Copilot もこの同じパスを
  # `--agent codex|copilot` 付きで呼ぶ。
  home.file.".claude/hooks/attribution-guard".source = "${pkgs.dotfiles-tools}/bin/attribution-guard";
  # stack-base-guard(ADR-0027): セッション内の複数 PR を常時単一チェーンに
  # 積むことを作成時に機械強制する(docs/agents/stack-base-guard.md)。#415 で
  # Rust バイナリ(crates/stack-base-guard)への安定パスの symlink になった
  # (attribution-guard と同じ理由付け)。Codex もこの同じパスを
  # `--agent codex` 付きで呼ぶ。
  home.file.".claude/hooks/stack-base-guard".source = "${pkgs.dotfiles-tools}/bin/stack-base-guard";
  # feedback-target-guard(ADR-543 段3): #415 で Rust バイナリ(crates/
  # feedback-target-guard)への安定パスの symlink になった(attribution-guard と
  # 同じ理由付け)。
  home.file.".claude/hooks/feedback-target-guard".source =
    "${pkgs.dotfiles-tools}/bin/feedback-target-guard";
  # repo-create-guard(ADR-0013 Amendment 2026-09-29): #415 で Rust バイナリ
  # (crates/repo-create-guard)への安定パスの symlink になった。
  home.file.".claude/hooks/repo-create-guard".source = "${pkgs.dotfiles-tools}/bin/repo-create-guard";
  # external-send-guard(docs/agents/external-send-guard.md): 外部宛メールの
  # 直接送信を deny し create_draft へ誘導する。独立ファイルで他 hook を
  # source しない。
  home.file.".claude/hooks/external-send-guard".source =
    "${pkgs.dotfiles-tools}/bin/external-send-guard";
  # adr-number(ADR-380, docs/agents/adr-numbering.md): PostToolUse で
  # ADR-478 を PR 番号へ自動改番する段3(利便性層)。#415 で Rust バイナリ
  # (crates/adr-number)への安定パスの symlink になった(gh-edit-allow と
  # 同じ理由付け)。Codex もこの同じパスを `--agent codex` 付きで呼ぶ。
  home.file.".claude/hooks/adr-number".source = "${pkgs.dotfiles-tools}/bin/adr-number";
  # gh-edit-allow(#392): crates/gh-edit-allow のビルド成果物(pkgs.dotfiles-tools、
  # flake.nix の rustOverlay)への安定パスの symlink。
  home.file.".claude/hooks/gh-edit-allow".source = "${pkgs.dotfiles-tools}/bin/gh-edit-allow";
  # rulesets-write-guard(ADR-503 D7): crates/
  # rulesets-write-guard のビルド成果物への安定パスの symlink(gh-edit-allow
  # と同じ理由付け)。
  home.file.".claude/hooks/rulesets-write-guard".source =
    "${pkgs.dotfiles-tools}/bin/rulesets-write-guard";
  # routines-write-guard(28番、ADR-519 D7):
  # crates/routines-write-guard のビルド成果物への安定パスの symlink
  # (gh-edit-allow と同じ理由付け)。
  home.file.".claude/hooks/routines-write-guard".source =
    "${pkgs.dotfiles-tools}/bin/routines-write-guard";
  # pkexec-guard(29番、docs/agents/pkexec-guard.md): crates/pkexec-guard の
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
  # ないので register には乗せない — wrapup-stop-gate が同じディレクトリから
  # 絶対パスで見つけて逐次呼ぶ(gh-edit-allow と同じ配置、PreToolUse/PostToolUse
  # の register とは別の消費経路)。
  home.file.".claude/hooks/verdict-escalate".source = "${pkgs.dotfiles-tools}/bin/verdict-escalate";

  # pr-title-guard(ADR-0031): PR タイトルを commit-message 契約として
  # 作成時に機械強制する(docs/agents/pr-title-contract.md)。
  # #415 で Rust バイナリ(crates/pr-title-guard)への安定パスの symlink に
  # なった(attribution-guard と同じ)。Codex/Copilot もこの同じパスを
  # `--agent codex|copilot` 付きで呼ぶ(bash 時代の per-agent adapter は廃止)。
  home.file.".claude/hooks/pr-title-guard".source = "${pkgs.dotfiles-tools}/bin/pr-title-guard";

  # pr-followup-guard(docs/agents/pr-followup-guard.md): PR 本文に未チェック
  # の task list を残さない・`## 要確認` の各項目に Issue 参照を持たせる
  # ことを作成時に機械強制する。#415 で Rust バイナリ(crates/pr-followup-guard)
  # への安定パスの symlink になった。Codex もこの同じパスを `--agent codex`
  # 付きで呼ぶ(bash 時代の ~/.codex/hooks/ の adapter は廃止)。
  home.file.".claude/hooks/pr-followup-guard".source = "${pkgs.dotfiles-tools}/bin/pr-followup-guard";

  # git-stash-guard(ADR-0032 Amendment #531): 素の `git stash` を弾く。
  # Codex 用の adapter ファイルは #416 で廃止し、Rust バイナリを --host codex
  # で直接登録する(codexGitStashGuardCmd)。Copilot 版は未展開(#161、
  # MCP tool 名の命名規則が未確認なため attribution-guard 系のみ先行)。

  # stack-base-guard(ADR-0027 Amendment #531)の Codex 版 adapter ファイルは
  # #415 で廃止し、Rust バイナリを --agent codex で直接登録する
  # (codexStackBaseGuardCmd)。

  # codex-plan-gate(ADR-0032 Amendment #531、docs/claude/codex-plan-gate.md):
  # 他 hook を source しない独立ファイル。
  home.file.".codex/hooks/codex-plan-gate".source = "${pkgs.dotfiles-tools}/bin/codex-plan-gate";

  # decision-colocation-guard(ADR-396): 決定成果物(ADR/設計文書/skill)の
  # 追加を執行点と同じ PR に機械強制する(docs/agents/decision-colocation.md)。
  # #415 で Rust バイナリ(crates/decision-colocation)への安定パスの symlink
  # になった(gh-edit-allow と同じ理由付け)。Codex/Copilot adapter は意図的に
  # 作らない(CI required check が全エージェント共通の backstop になるため)。
  home.file.".claude/hooks/decision-colocation-guard".source =
    "${pkgs.dotfiles-tools}/bin/decision-colocation-guard";

  # issue-index: 自分に関係する open Issue の索引だけを SessionStart で注入する。
  home.file.".claude/hooks/issue-index".source = "${pkgs.dotfiles-tools}/bin/issue-index";

  # esa-mirror-context: 社内 wiki の mirror の所在・検索手順・引用形式・鮮度を SessionStart で注入する。
  home.file.".claude/hooks/esa-mirror-context".source =
    "${pkgs.dotfiles-tools}/bin/esa-mirror-context";
  # daily-desk-reminder: 今日の朝の机(daily-desk)が未実施なら、SessionStart で伝える。ハブ未設定の機体では無音。
  home.file.".claude/hooks/daily-desk-reminder".source =
    "${pkgs.dotfiles-tools}/bin/daily-desk-reminder";
  # worklog: 作業ログの規範と同じリポジトリの直近の stuck / found を SessionStart で注入する。
  # vault を宣言していない機体(~/.config/dotfiles/obsidian-vault が無い)では無音。
  home.file.".claude/hooks/worklog".source = "${pkgs.dotfiles-tools}/bin/worklog";

  # gpg-prewarm: git commit の署名パスフレーズをログイン直後に温める。
  home.file.".claude/hooks/gpg-prewarm".source = "${pkgs.dotfiles-tools}/bin/gpg-prewarm";

  # herdr-sidebar-metadata: permission mode(hook)とモデル・メトリクス(statusline)
  # を Herdr サイドバーのカスタムトークンに流す 2 チャネル構成。表示側の行定義は
  # config/herdr/config.toml(home/modules/herdr.nix が配備)。herdr が自動
  # インストールする統合 hook(herdr-agent-state.sh、herdr 管理)の隣に並ぶが、
  # 互いに自分のエントリしか触らないので衝突しない。
  # #413: Claude/Codex/Copilot 共通の 1 バイナリ(--agent)。Codex/Copilot の登録
  # (home/modules/herdr.nix)もこのパスを指す。
  home.file.".claude/hooks/herdr-agent-metadata".source =
    "${pkgs.dotfiles-tools}/bin/herdr-agent-metadata";

  # claude-usage: herdr の tab_bar_right command が interval 実行する(Claude Code
  # hook ではない — settings.json には登録しない)。呼び出し側は
  # config/herdr/config.toml。詳細は docs/claude/claude-usage.md。
  # 実体は crates/claude-usage(#413)。配備先は herdr のハードコード実行パスに
  # 合わせて引き続き ~/.claude/hooks/(拡張子は ADR-0007 に従い落とした)。

  # worktree-base-sync: pristine な worktree だけを origin/<base> へ黙って
  # fast-forward する SessionStart hook。
  home.file.".claude/hooks/worktree-base-sync".source =
    "${pkgs.dotfiles-tools}/bin/worktree-base-sync";

  # herdr-status-advisory: herdr の client と server の版の食い違いを、
  # セッション開始時に知らせる SessionStart hook。
  home.file.".claude/hooks/herdr-status-advisory".source =
    "${pkgs.dotfiles-tools}/bin/herdr-status-advisory";

  # gate-failopen-advisory: fail-open で素通りし続けている gate を、セッション開始時に
  # 知らせる SessionStart hook(#822)。
  home.file.".claude/hooks/gate-failopen-advisory".source =
    "${pkgs.dotfiles-tools}/bin/gate-failopen-advisory";

  # pr-gate: PR completion barrier。判定対象は allowlist に列挙した nwo だけ
  # (既定は本リポジトリのみ)なので、他リポジトリでは完全沈黙する。
  home.file.".claude/hooks/pr-gate".source = "${pkgs.dotfiles-tools}/bin/pr-gate";
  # git-worktree-allow: herdr worktree への `git -C` を検証つきで許可する PreToolUse hook。
  home.file.".claude/hooks/git-worktree-allow".source =
    "${pkgs.dotfiles-tools}/bin/git-worktree-allow";
  # git-stash-guard: 素の `git stash` を deny する PreToolUse hook。
  home.file.".claude/hooks/git-stash-guard".source = "${pkgs.dotfiles-tools}/bin/git-stash-guard";
  # hook-router: ツール呼び出しに付く検査 22 件の入口(上の hookRouterPreCmd / hookRouterPostCmd)。
  home.file.".claude/hooks/hook-router".source = "${pkgs.dotfiles-tools}/bin/hook-router";
  # main-checkout-guard: 本物の checkout を変更させない PreToolUse + Stop hook。
  home.file.".claude/hooks/main-checkout-guard".source =
    "${pkgs.dotfiles-tools}/bin/main-checkout-guard";
  # bleep(旧 publish-guard): 会社/private リポジトリの実名が PUBLIC な面に
  # 漏れるのを防ぐ PreToolUse hook。上流を別リポジトリ tarotene/bleep に
  # 切り出し(ADR-0009)、flake input(pinned rev)からツリーごと配備する。
  # ツリー全体を1つの home.file で(個々のファイルを列挙せず)配ることで、
  # hooks/bleep.sh の自己解決(`realpath "$0"`)がそのまま成立する — 詳細は
  # docs/agents/public-publish-guard.md。
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
  # bleep の allow-tools.txt(bleep#95、tarotene/bleep#106): Bash 以外のツールのうち、
  # repo-ref の検査から外すものをツール名の拡張正規表現で宣言する。ツール名だけで
  # 実値(org/repo 名)を含まないので、orgs.txt・repos.txt と違い PUBLIC なこの
  # リポジトリで宣言する(docs/agents/public-publish-guard.md)。組織内 Slack の
  # 下書きは本人が確認してから送るもので、送信のツールは宣言しない(外部発信の
  # ガード external-send-guard が下書き止まりにする)。サーバー名は機体で違うので
  # `.*` に任せる。home-manager が読み取り専用のリンクとして配るため、手での追記や
  # エージェントによる書き換えはできない。
  xdg.configFile."bleep/allow-tools.txt".text = ''
    # 組織内 Slack の下書き(送信ではない。本人が確認してから送る)
    mcp__.*__slack_send_message_draft
  '';
  # 配備された bleep hook の canary(#604)。bleep 入力の pin が決める hook の実体を、
  # 登録コマンドと同じ環境で実際に動かして判定を確かめる。home.checks は失敗すると
  # generation の build 自体が失敗し、closure には残らない — つまり bleep 側の破壊的
  # 変更に dotfiles の配線が追従していない pin は、lock でも `hms`(bleep main への
  # override、crates/hms)でも `hms .` でも CI でも、配備される前に弾かれる。
  # 検査対象は home-files(配備後のレイアウトそのもの)。
  home.checks = [
    (pkgs.runCommand "bleep-canary"
      {
        nativeBuildInputs = with pkgs; [
          bash
          coreutils
          gnugrep
          gnused
          jq
          git
        ];
      }
      ''
        # nix のサンドボックスには /usr/bin/env が無く、bleep-hook が exec する bleep 本体
        # (#!/usr/bin/env bash)が起動できない。配備された実体と同じ内容のコピーの
        # shebang だけ store の bash に直して検査する(判定ロジックには触れない)。
        hooks="$TMPDIR/hooks"
        mkdir -p "$hooks"
        ln -s ${config.home-files}/.claude/hooks/bleep-hook "$hooks/bleep-hook"
        cp -rL ${config.home-files}/.claude/hooks/bleep "$hooks/bleep"
        chmod -R u+w "$hooks/bleep"
        patchShebangs "$hooks/bleep"
        bash ${repoConfig}/agents/bleep-canary.sh "$hooks"
        touch $out
      ''
    )
    # スキル配布の検査(#1124)。`checks.<system>.agent-skills`(flake.nix)と同じ定義を、
    # 適用する配布物そのもの(この config の home.file)に掛ける。flake の check は
    # activation build の依存ではなく、`hms .` は検査に落ちた配布物を実ホームへ置けた。
    (import ../tests/agent-skills.nix {
      inherit pkgs lib;
      files = config.home.file;
    })
    # `${pkgs.dotfiles-tools}/bin/<名前>` の参照先が実在することの検査(#1170)。#683 が
    # 実在しない `bin/obsidian-backup` への参照だけを入れ、評価もビルドも通ったまま
    # バックアップが 25 日止まった。参照の一覧は登録簿にせず、生成された出力から導く:
    # home-files の store へのシンボリックリンクと systemd unit、activation script の文字列。
    (pkgs.runCommand "dotfiles-tools-references"
      {
        nativeBuildInputs = with pkgs; [
          coreutils
          findutils
          gnugrep
        ];
        activationScript = lib.concatStringsSep "\n" (
          lib.mapAttrsToList (_: entry: entry.data) config.home.activation
        );
        passAsFile = [ "activationScript" ];
      }
      ''
        tools=${pkgs.dotfiles-tools}
        status=0
        missing() {
          echo "dotfiles-tools-references: $1 を参照しているが、実行ファイルが dotfiles-tools に無い ($2)" >&2
          status=1
        }
        while IFS= read -r -d $'\0' link; do
          target=$(readlink "$link")
          case "$target" in
            "$tools"/*) test -e "$target" || missing "$target" "''${link#${config.home-files}/}" ;;
          esac
        done < <(find ${config.home-files} -type l -print0)
        refs=$(grep -RhoE "$tools/bin/[A-Za-z0-9._+-]+" \
          ${config.home-files}/.config/systemd "$activationScriptPath" 2>/dev/null | sort -u || true)
        for ref in $refs; do
          test -e "$ref" || missing "$ref" "systemd unit または activation script"
        done
        if [ "$status" != 0 ]; then exit 1; fi
        touch $out
      ''
    )
  ];

  # Codex/Copilot 版 shim の配線(#160)。Claude Code plugin 相当の配線
  # (上の settings.json マージ)はあったが、Codex CLI (~/.codex/hooks.json) /
  # Copilot CLI (~/.copilot/settings.json) には ADR-0009 決定7で明示的に
  # 対象外としたまま配線していなかった。agent-settings-reconcile の
  # codex-hooks / copilot-hooks は worktree.nix / herdr.nix が同じ対象ファイルを
  # 書き換える registrar なので、lost-update 窓(#61 と同種)を避けるため
  # それらの後ろに明示的に順序付ける。#25-28(Rust hook cutover + bleep
  # 改名)で旧 adapters/{codex,copilot}-adapter.sh が upstream から削除された
  # ため、--retire で旧 command 文字列を先に取り除いてから新 bleep.sh の
  # command を登録する(register-{codex,copilot}-hooks に追加した --retire、
  # Claude 側の retiredHookEntries と同型 — 削除せず放置すると存在しない
  # パスを指したまま毎 tool call で失敗する)。

  home.activation.registerCopilotBleepHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCopilotHerdrMetadataHooks" ]
      ''
        # pipeline-node: bleep
        run ${registerCopilotHooks} "$HOME/.copilot/settings.json" \
          --retire preToolUse ${lib.escapeShellArg legacyPublishGuardCopilotAdapterCmd} \
          --retire preToolUse ${lib.escapeShellArg legacyBleepCopilotCmdUnprefixed} \
          --register \
          preToolUse ${lib.escapeShellArg bleepCopilotCmd} 20
      '';

  # attribution-guard の Codex/Copilot 展開(#192)。同じ lost-update 対策で
  # bleep の登録の後ろに明示的に順序付ける。

  home.activation.registerCopilotAttributionGuardHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCopilotBleepHooks" ]
      ''
        # pipeline-node: attribution-guard
        run ${registerCopilotHooks} "$HOME/.copilot/settings.json" \
          --retire preToolUse ${lib.escapeShellArg legacyCopilotAttributionGuardCmd} \
          --register \
          preToolUse ${lib.escapeShellArg copilotAttributionGuardCmd} 10
      '';

  # pr-title-guard の Codex/Copilot 展開(ADR-0031、#192 の型を踏襲)。同じ
  # lost-update 対策で attribution-guard の登録の後ろに明示的に順序付ける。

  home.activation.registerCopilotPrTitleGuardHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCopilotAttributionGuardHooks" ]
      ''
        # pipeline-node: pr-title-guard
        run ${registerCopilotHooks} "$HOME/.copilot/settings.json" \
          --retire preToolUse ${lib.escapeShellArg legacyCopilotPrTitleGuardCmd} \
          --register \
          preToolUse ${lib.escapeShellArg copilotPrTitleGuardCmd} 10
      '';

  # pkexec-guard(29番)の Codex/Copilot 展開: 判定ロジックは持たず、`pkexec`
  # を含む Bash コマンドを常に deny する(docs/agents/pkexec-guard.md「Codex /
  # Copilot からは pkexec を一切許さない」)。attribution-guard/pr-title-guard
  # と同じ lost-update 対策で、その後ろに明示的に順序付ける。

  home.activation.registerCopilotPkexecGuardHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCopilotPrTitleGuardHooks" ]
      ''
        # pipeline-node: pkexec-guard
        run ${registerCopilotHooks} "$HOME/.copilot/settings.json" \
          preToolUse ${lib.escapeShellArg pkexecGuardCopilotCmd} 10
      '';

  # agent-turn-log の Copilot 展開(ADR-0032 Amendment 2026-10-01)。同じ
  # lost-update 対策で、最後尾の pkexec-guard の登録の後ろに明示的に順序付ける。
  home.activation.registerCopilotAgentTurnLogHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCopilotPkexecGuardHooks" ]
      ''
        # pipeline-node: agent-turn-log
        run ${registerCopilotHooks} "$HOME/.copilot/settings.json" \
          --retire UserPromptSubmit ${lib.escapeShellArg legacyCopilotAgentTurnLogCmd} \
          --retire Stop ${lib.escapeShellArg legacyCopilotAgentTurnLogCmd} \
          --register \
          UserPromptSubmit ${lib.escapeShellArg copilotAgentTurnLogCmd} 10 \
          Stop ${lib.escapeShellArg copilotAgentTurnLogCmd} 10
      '';

  # worklog の Copilot 展開(ADR-998 の追記)。Codex は共通宣言の SessionStart から展開済み。Copilot
  # CLI の sessionStart は最上位の `additionalContext` だけを読む(GitHub Docs, "GitHub Copilot hooks
  # reference"、取得 2026-10-28)ので、`--agent copilot` でその形を出させる。同じ lost-update 対策で、
  # 最後尾の agent-turn-log の登録の後ろに明示的に順序付ける。
  home.activation.registerCopilotWorklogHooks =
    lib.hm.dag.entryAfter [ "writeBoundary" "registerCopilotAgentTurnLogHooks" ]
      ''
        # pipeline-node: worklog
        run ${registerCopilotHooks} "$HOME/.copilot/settings.json" \
          sessionStart ${lib.escapeShellArg "${worklogStartCmd} --agent copilot"} 10
      '';

  home.file.".claude/pr-gate-repos".text = ''
    # pr-gate が Stop / SessionStart で判定する対象リポジトリ(owner/repo, 1行1つ)。
    # ここに無い repo では完全沈黙する。# 始まりの行と空行は無視。
    tarotene/dotfiles
  '';

  # Hook の外からも使える手動の入口。
  # 同じバイナリを置く(CSS は $HOME/.claude/hooks/plan-view.css へのフォール
  # バックで解決する、上の plan-view 節参照)。
  home.file.".local/bin/plan-view".source = "${pkgs.dotfiles-tools}/bin/plan-view";

  # claude-plan-model: Plan 側モデルを Fable ⇄ Opus で切り替える(引数なし=トグル)。
  # hook ではないので ~/.claude/hooks/ ではなく ~/.local/bin に置く — git-shelve や
  # git-prune-branches と同じ「PATH で解決される実行可能ファイル」扱い(ADR-0007 に
  # 従い配備名から .sh を落とす)。activation はこの配備物ではなく store 上の同じ
  # 実体を `sync` で呼ぶ(Rust 版、#414。bash 版が要した jq・coreutils 等の PATH
  # は不要になった)。

  # Opt-in native Codex launcher; the user's pair and config.toml stay writable
  # and are never seeded/reset by activation (docs/codex-plan.md).
  home.file.".local/bin/codex-plan".source = "${pkgs.dotfiles-tools}/bin/codex-plan";

  # Model equivalence (docs/model-equivalence.md): the vendor-neutral catalog tool and the
  # judge/page generator. Manual CLIs; the key for `fetch-aa` is read from the environment or the
  # login keyring at run time, never deployed.
  home.file.".local/bin/model-catalog".source = "${pkgs.dotfiles-tools}/bin/model-catalog";
  home.file.".local/bin/model-equivalence".source = "${pkgs.dotfiles-tools}/bin/model-equivalence";

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

  # Skill distribution is declared once below; frontend-slides remains opt-in
  # outside the skill discovery roots (presentation-authoring invokes it).
  home.file.".local/share/frontend-slides".source =
    "${frontend-slides}/plugins/frontend-slides/skills/frontend-slides";

  # グローバル AGENTS.md(正本): agent 非依存の調査・先行例確認・PR 運用・
  # 生成元明示の方針。Codex CLI(~/.codex/AGENTS.md)・Copilot CLI
  # (~/.copilot/copilot-instructions.md)・Claude Code(~/.agents/AGENTS.md
  # を CLAUDE.md から @import)の 3 CLI に同一ソースをマウントする —
  # ~/.agents/skills/ のクロスツールルーティング(コメント索引 14) 付近)と
  # 同型。詳細は docs/agents/global-agents-md.md。
  home.file.".agents/AGENTS.md".source = repoConfig + "/agents/AGENTS.md";
  home.file.".copilot/copilot-instructions.md".source = repoConfig + "/agents/AGENTS.md";

  # ~/.codex/AGENTS.md: 共有 AGENTS.md(正本) + Codex 固有の施行配線
  # (config/codex/AGENTS.codex.md)を nix 評価時に結合した派生物。Codex は
  # `@import` 構文を持たないため、Claude Code の `~/.claude/CLAUDE.md` =
  # `@~/.agents/AGENTS.md` importと同じ「単一正本 + router」構造を、
  # build 時のファイル結合で実現する(ADR-0032 Amendment #531)。正本は
  # 依然 2 ファイル(共有 AGENTS.md・AGENTS.codex.md)のままで、結合物
  # そのものは手で複写せず nix が毎回生成する。

  # グローバル CLAUDE.md: 上記共有 AGENTS.md を @import する router +
  # Claude Code 固有の施行配線(gate/ExitPlanMode/AskUserQuestion まわり)。
  # リポジトリルートの CLAUDE.md(`@AGENTS.md`)と同型の router 構造を
  # グローバル階層にも適用したもの。詳細は上のコメント索引 14) と
  # docs/claude/global-claude-md.md。

  # 宣言(claudeHookDeclarations / retiredHookEntries)は JSON で 1 引数に渡す。
  home.activation.registerCodexSharedHooks =
    lib.hm.dag.entryAfter
      [
        "writeBoundary"
        "registerCodexWorktreeHooks"
        "registerCodexHerdrMetadataHooks"
      ]
      ''
        # pipeline-node: hook-router-pre, hook-router-post, codex-plan-gate
        run ${settingsReconcile} codex-hooks-spec "$HOME/.codex/hooks.json" ${lib.escapeShellArg codexHooksSpec}
      '';

  home.activation.registerClaudeHooks = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    run ${settingsReconcile} claude-hooks "$HOME/.claude/settings.json" \
      ${lib.escapeShellArg claudeHooksSpec}
  '';

  # 今のモード(Fable / Opus)は保ったまま、その具体モデル ID だけを claude バイナリの
  # `latest_per_family` から引き直す。hooks・statusLine・permissions と同じ DAG 位置で、
  # 独立した activation として走らせる。
  #
  # claude が未インストールなら(bootstrap 直後)何も書かずに終わる — 起動する claude が
  # 無いのに env だけ置いても意味が無く、中途半端な model 設定のほうが有害だから。

  # settings.json の permissions.allow/permissions.ask を冪等に拡充する。
  # registerClaudeHooks と同じ DAG 位置(writeBoundary の後)で、独立した
  # activation script として走らせる — 片方が既存の hooks 登録ロジックを
  # 壊さないようにするため、jq マージの責務を混ぜない。

  # Temporary compatibility entries. Each delegates to the canonical implementation.
  home.file.".claude/hooks/desk-start".source = "${pkgs.dotfiles-tools}/bin/desk-start";
  home.file.".claude/hooks/esa-index".source = "${pkgs.dotfiles-tools}/bin/esa-index";
  home.file.".claude/hooks/pr-confirm-guard".source = "${pkgs.dotfiles-tools}/bin/pr-confirm-guard";
  home.file.".claude/hooks/sign-prewarm".source = "${pkgs.dotfiles-tools}/bin/sign-prewarm";
  home.file.".claude/hooks/worktree-fresh-base".source =
    "${pkgs.dotfiles-tools}/bin/worktree-fresh-base";
  home.file.".claude/hooks/session-wrapup".source = "${pkgs.dotfiles-tools}/bin/session-wrapup";
  home.file.".claude/hooks/issue-inbox".source = "${pkgs.dotfiles-tools}/bin/issue-inbox";
  home.file.".claude/hooks/session-retrospective".source =
    "${pkgs.dotfiles-tools}/bin/session-retrospective";
  home.file.".claude/hooks/feedback-trial".source = "${pkgs.dotfiles-tools}/bin/feedback-trial";

}
