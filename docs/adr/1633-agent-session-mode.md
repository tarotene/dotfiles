# ADR-1633 — セッション単位のモードは、起動時に 1 つの宣言から展開し、展開できないときは通常のセッションに倒さない

- Status: Accepted
- Date: 2026-11-18
- Issue: No-Issue(巨大なリポジトリで、ADR のような覆しにくい決定に引きずられずに整理を進めたい、という本人の依頼。
  方針は本人が `AskUserQuestion` で選んだ: 環境変数で起動時に展開する・どのリポジトリでも・読ませないものは 4 種・2 段階)
- Amends: なし

## Context

決定の記録(ADR・設計文書)は、accepted のあとは変えない。覆すときは、新しい ADR を足す(ADR-0008)。普段は健全だが、
巨大なリポジトリで大きな整理をするときは、エージェントが記録を読む量と、記録に合わせようとする拘束が、整理の邪魔になる。
dotfiles では `docs/adr` が 96 本、`docs/agents` が 97 本、毎セッション読まれる共通規約が 1,084 行ある(2026-11-18)。

拘束を外す手段は、これまで機体単位(`hms` の適用時に固定)か、gate ごとの `SKIP_<STEM>=1`(人が起動前に 1 つずつ立てる)だった。
セッションごとに、読ませない範囲・追記する文・止める検査を、まとめて切り替える単位が無い。

## Decision

### D1: モードは、宣言(TOML)1 つから、3 つを展開して `claude` を exec する

`CLAUDE_MODE=<名前> claude` と起動すると、zsh の `claude` 関数が `agent-mode exec` に渡し、
`config/agents/modes/<名前>.toml` から、次の 3 つを作る。

- 読み取り禁止の設定: `--settings` にインラインで渡す `permissions.deny` の `Read(..)` と `Edit(..)`。
- 追記する文: `--append-system-prompt-file`。
- 止める検査の束: 既存の `SKIP_<STEM>=1`(セッション単位の逃げ道)。

### D2: 止める検査の名前は、逃げ道の表が唯一の正本で、宣言には stem だけを書く

stem が `escape::ALL` に無い、またはセッション単位を持たないときは、起動を失敗させる。新しい逃げ道は作らず、表にあるものを束ねる。

### D3: 展開できないときは、通常のセッションに倒さない

`CLAUDE_MODE` が指定されたのに、`agent-mode` が無い・宣言が読めない・検証に落ちるときは、起動しない(終了コード 127 または 2)。
拘束を外したつもりで、外れていない(または、外れていない前提で、外れた)セッションを黙って始めないため。

### D4: 第 1 段(モード)は、記録を読めも書けもしない。覆す ADR は第 2 段(通常のセッション)で足す

Claude Code の `Read` の deny は、同じパスへの Edit・Write も止める。したがってモードでは記録を足せない。モードの PR は
`--draft` で出し、本文の `## 触れた決定` に、消した・変えた検査・設定・文書を残す。通常のセッションで、覆す決定ごとに新しい ADR を
**同じブランチ**に足す。

### D5: セッション開始時の文脈の注入は、hook の宣言の `injects` で分け、`true` の hook を包んで、モードの間だけ無音にする

SessionStart の宣言は bool の `injects` を必ず持つ(付け忘れは nix の評価で落ちる)。`true` は文脈を足すだけの hook
(Issue の索引・朝の机・未裁定の気づきの件数など)で、Claude の登録だけを `agent-mode inject-gate -- <元のコマンド>` で包む。
モードが注入を止めている間(`DOTFILES_AGENT_MODE_INJECTION=off`)は元の hook を実行しない。`false` は、注入のほかに状態の書き込み・
登録を担う hook(`session-wrapup-start`・`pr-gate-session-start` など)で、包まない(Stop の検査が前提を失うため)。Codex・Copilot の
登録は包まない(command が変わると、Codex の hook の信頼がやり直しになる)。

### D6: 健全さは、触れた決定の一覧を機械的に出して担保する

既存の検査(`decision-colocation-check`)は、足された ADR の執行点が実在するかだけを見て、**旧 ADR の執行点が消えた・変わったこと**は、
どの検査も見ない。モードの PR は、決定を読まずに執行点を消すので、この穴を通る。`decision-colocation-touched --base <ref>` が、
base 時点の ADR の `## 執行点` のうち、変更・削除・改名されたものを ADR ごとに出し、あわせて、消した・変えたテストと CI の設定を出す。
PR 本文の `## 触れた決定` に貼り、通常のセッションが、覆す決定ごとに新しい ADR(見出しの `- Supersedes:`)を同じブランチへ足す。

### D7: 共通規約の「決定の記録に関わる節」は、正本を 1 ファイルのまま印で囲み、Claude 用に切り分けて、モードが外す

`config/agents/AGENTS.md` と `config/claude/CLAUDE.md` は、決定の記録に関わる節を `<!-- agent-mode:decisions:begin -->` と
`<!-- agent-mode:decisions:end -->` で囲む。`home/lib/split-regions.nix` が、印の中(decisions)と外(core)に分け、
`~/.agents/AGENTS.core.md`・`~/.agents/AGENTS.decisions.md`・`~/.claude/CLAUDE.md`(core)・`~/.claude/CLAUDE.decisions.md` を配る。
`~/.claude/CLAUDE.md` の先頭の import が、その 3 つを読む。モードは decisions の 2 つを `claudeMdExcludes`(宣言の `[norms].excludes`)で
外す。Codex・Copilot・`~/.agents/AGENTS.md` は全文のまま(印が増えるだけ)。印の誤り(入れ子・対応の無い印・領域が無い)は nix の評価で落ちる。

2026-11-18 の実測で、`claudeMdExcludes` が `@import` の先と rules のファイルの両方に効くことを確かめた(`docs/agents/agent-mode.md`)。

## 先行例との対比

- D1: 起動時に、宣言から設定・追記する文・環境変数を束ねて渡す
  先行例: Anthropic, "CLI reference", https://code.claude.com/docs/en/cli-reference (取得 2026-11-18)
  差分: 異なる — 公式は個別のフラグだけで、束ねる仕組みは無い。束ねる側をこのリポジトリが持つ
  軸: 表現不可能 — 宣言 > 手続き
- D2: 逃げ道の名前を表から導く
  先行例: crates/agent-session/src/escape.rs (取得 2026-11-18)
  差分: 一致 — 新しい逃げ道を作らず、表にある名前を束ねる
  軸: 表現不可能 — 閉語彙(表)と突き合わせる
- D3: 展開できないときは起動しない
  先行例: crates/repo-scope-guard の「読めなければ deny」(docs/adr/1608-repo-scope-by-machine-contract.md の D1)
  差分: 一致 — 拘束を変える仕組みは、失敗したら緩める側ではなく、止める側に倒す
  軸: 表現不可能 — 失敗の状態から、通常のセッションに入る経路を無くす

- D5: 注入する hook を、宣言の 1 属性で分けて、実行時に判定する包みで止める
  先行例: home/modules/agents.nix の `enabledClaudeHookDeclarations` (取得 2026-11-18)。option で宣言を出し入れする
  差分: 異なる — 既存は機体単位(`hms` の適用時に固定)で、セッションごとには切り替えられない。モードは起動ごとに変わるので、実行時に判定する。
  各 hook の crate に同じ判定を散らさず、宣言の 1 属性に寄せる
  軸: 表現不可能 — 閉語彙(`injects` が無い宣言は評価で落ちる)。単一正本 > 複写

- D6: 触れた決定の一覧を、既存の検査と同じ crate で、同じ読み方(`extract_paths`)から導く
  先行例: crates/decision-colocation/src/check.rs の `run_check` (取得 2026-11-18)。足された ADR・Amendment・決定成果物だけを見る
  先行例: docs/adr/0008-documentation-artifact-selection.md の 2026-11-09 の Amendment (取得 2026-11-18)。覆すときは新しい ADR が `- Supersedes:` で宣言する
  差分: 異なる — 既存は新しい ADR の執行点が実在するかだけを見る。旧 ADR の執行点が消えたことは、どの検査も見ない
  軸: 表現不可能 — 単一正本(ADR の執行点)から導く > 人が書く一覧

- D7: 正本を複写せず、1 ファイルの中の領域を印で示して、配備のときに切り分ける
  先行例: Anthropic, "How Claude remembers your project", https://code.claude.com/docs/en/memory (取得 2026-11-18)。`claudeMdExcludes` は絶対パスの glob で、`@path` の import 先に効くかの記述は無かった
  先行例: 2026-11-18 の実測(スクラッチのプロジェクト、`claude -p --model haiku`)。import 先と rules のファイルの両方が、除外で `NOT-VISIBLE` になった
  差分: 異なる — 公式の用法は monorepo で他チームの CLAUDE.md を外すこと。ここでは、1 つの正本から切り分けた import 先を外す。実測で、効くことを確かめた
  軸: 表現不可能 — 読み込まれない状態にする > 「適用しない」と書く。正本は 1 ファイルのまま(単一正本 > 複写 + 同期)

## Consequences

- 同じ worktree で、通常のセッションとモードのセッションを並べられる(モードは起動したプロセスだけに効く)。
- 共通規約の「決定の記録に関わる節」は、モードのセッションでは読み込まれない(D7)。印で囲む節を足す・動かすときは、安全に関わる節を
  囲まない。`~/.claude/CLAUDE.md` は、store への symlink でなく、切り分けた本体の文章になる。`feedback-target-guard` は、切り出した節の
  見出しも `~/.claude/CLAUDE.decisions.md` から探す。
- CI の検査(`decision-colocation-check` など)と `pr-gate` は止まらない。
- モードの間も、`session-wrapup-start`(未裁定の気づきの案内)と `pr-gate-session-start`(残骸ブランチの通知)の注入は残る。
  どちらも、状態の書き込み・登録と同じ hook の中にあり、注入だけを分けるには、その hook の中で出力を止める必要がある。
- モードを足すには、`config/agents/modes/<名前>.toml` と追記する文を足し、`home/modules/agents.nix` の配備に 2 行を足す。

## 執行点

- crates/agent-mode/src/lib.rs
- config/agents/modes/refactor.toml
- config/zsh/modules/53-tools-claude.zsh
- home/modules/agents.nix
- crates/decision-colocation/src/touched.rs
- home/lib/split-regions.nix
- home/modules/claude.nix
- crates/feedback-target-guard/src/lib.rs
