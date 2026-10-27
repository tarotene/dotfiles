# hook-router — ツール呼び出しの検査を、1 つの登録にまとめる

実装: `crates/hook-router`(`hook-router pre|post`)
規則の表: `crates/hook-router/src/rules.rs`
登録: `home/modules/agents.nix` の `claudeHookDeclarations`(`hook-router-pre` / `hook-router-post`)
地図: `pipeline-map.toml` の `hook-router-pre` / `hook-router-post`、ADR-872 Amendment

Codex も同じ規則表を `pre|post --agent codex` で使う。登録は同じ宣言から生成し、
`apply_patch` の入力変換は `codex.rs` が担う。許可は別の `permission --agent codex`
入口で PermissionRequest に返す。Atuin とローカルHookの互換入口も規則表とは別である。
対応範囲と上流の制約は [Codexとの対応表](../claude/codex-hook-parity.md) を参照。

## 何のための仕組みか

Claude Code の hook は、検査 1 つごとに別々の登録だった(`claude.nix` に約 50 件)。ツール呼び出しに付く
検査は、`matcher`(`Bash|mcp__.*` のような文字列)と `if`(`Bash(gh *)` のような permission rule 構文)で、
どの呼び出しに当てるかを絞っていた。この文字列を書き間違えると、**検査が黙って素通りになる**。旧登録の
注意書きに実例が残っている: Bash と MCP を 2 つの hook に分けて同じ command を 2 つの matcher で宣言すると、
`(event, command)` が宣言の一意キーなので 2 つ目が無視され、MCP の経路が無検査のまま残る(bleep の旧実装が
`Bash` 単体の matcher だったために持っていた最大の欠陥)。

ルータは、ツール呼び出しに付く検査のうち、権限の逸脱検査・拒否の検査と記録役(PostToolUse の
4 件)を、**1 つのプロセスで、規則の表に従って**走らせる。Claude Code には `PreToolUse` と `PostToolUse` の
2 件だけを、`matcher` なしで登録する。どのツールにどの検査を当てるかは、表の [`Tools`](型) が持つ:
`All` / `Bash` / `BashOrMcp` / `Mcp` / `Named(&[…])` の 5 通りだけが書ける。文字列の書き間違いはコンパイルが
通らず、表に載せ忘れた検査は pipeline-map の `cargo test` が「auto なのに登録箇所が無い」で落とす。

## 載せるもの

ルータの表(`rules.rs`)に載せた検査は 22 件(拒否だけを出す PreToolUse の 18 件と、記録役の PostToolUse の 4 件)。共有権限の検査は撤去した。

| イベント | 検査 |
|---|---|
| PreToolUse | attribution-guard、ci-wait-guard、decision-colocation-guard、external-send-guard、feedback-target-guard、git-stash-guard、main-checkout-guard-pre、new-tool-guard、pkexec-guard、plan-precedent-gate、plan-scope-gate、pr-followup-guard、pr-title-guard、repo-create-guard、routines-write-guard、rulesets-write-guard、stack-base-guard、worktree-create-guard |
| PostToolUse | adr-number、cmd-hash-log、gh-edit-allow-record、new-tool-guard-record |

載せる検査が満たす条件(載せるかどうかは、検査ごとにこの 4 つで決める):

1. **出せる判定が決まっている。** PreToolUse なら deny だけ(allow・ask は、共通権限の検査だけ)、PostToolUse なら
   `additionalContext` だけ。`Outcome` に無い形(Stop の block、`PermissionRequest`)を出す検査は載せられない。
2. **プロセス全体の状態を変えない。** 規則は同じプロセスの別スレッドで並列に走る。作業ディレクトリ
   (`set_current_dir`)、umask、シグナルは全スレッドに及ぶので、触る検査は載せられない。作業ツリーを書き換える
   検査も、並列の別規則が読む状態を変えるので載せない。
3. **時間が規則の予算(10〜20 秒)に収まる。** 越えたら結果を捨てて素通りにする契約なので、越えるのが常態の検査は
   「検査しない」になる。
4. **`hook(raw) -> Option<Outcome>` として、標準入力の文字列だけから判定できる。** 標準出力・標準エラー・終了コードを
   `main` に残せる。

## 載せなかった検査の裁定(#991)

検査ごとに、「残す」(今の登録のまま)か「載せた」かと、その理由を書く。出典の欄が `読んだ` のものは、2026-10-27 に
該当のファイルを読んで確かめた。`#991` は、Issue の調査(2026-10-24)の記述を、読み直さずに写したもの。

| 検査(pipeline-map の node) | イベント | 裁定 | 理由 | 出典 |
|---|---|---|---|---|
| plan-scope-gate | PreToolUse(ExitPlanMode) | **載せた** | 条件 1〜4 を満たす。`plan_scope_gate::hook` は deny だけを返し(allow・ask の経路が無い)、標準出力・標準エラーへ書かない。外へ出るのは `git remote get-url` と `gh api graphql`(フォールバックは `gh issue view`)の読み取りだけで、失敗は参照 Issue 単位で黙って諦める。`gh` の呼び出しは、判定の副作用ではなく入力。かつての `timeout` 20 秒を `budget` に写した。作業ディレクトリへ移っていた `enter_cwd` は、`HookInput::effective_cwd`(移動しない)に置き換えた | 読んだ: `crates/plan-scope-gate/src/lib.rs`(`hook`)、`main.rs` |
| plan-precedent-gate | PreToolUse(ExitPlanMode) | **載せた** | 判定は計画の本文への正規表現だけで、`gh`・ネットワークを呼ばない。出力は deny だけ。かつての `timeout` 10 秒を `budget` に写した | 読んだ: `crates/plan-precedent-gate/src/lib.rs`(`hook`)、`main.rs` |
| plan-fresh-gate | PreToolUse(ExitPlanMode) | 残す | 条件 2・3 に反する。pristine な worktree を `git merge --ff-only` で動かす(作業ツリーを書き換える)。`git fetch` に最大 15 秒を許し、判定を含めた `timeout` は 30 秒で、ルータの最大の予算(20 秒)を越える | 読んだ: `crates/plan-fresh-gate/src/lib.rs:10`、`main.rs:15,76`、`agents.nix` の `timeout = 30` |
| plan-view | PreToolUse(ExitPlanMode) | 残す | 条件 2 に反する。起動時に `restrict_umask()` でプロセス全体の umask を 077 に落とし、`setsid` で切り離したブラウザを起動する。ルータに載せると、umask がほかの規則のスレッドが作るファイルにも及ぶ。判定(deny)も出さない表示役 | 読んだ: `crates/plan-view/src/main.rs:6`、`lib.rs:236-249,461-465` |
| copilot-plan-review | PreToolUse(ExitPlanMode) | 残す | 条件 3 に反する。外部の LLM(Copilot CLI)のレビューを待つ `timeout` は 300 秒で、ルータの最大の予算の 15 倍。ルータは越えた規則を素通りにするので、載せると、レビューが常に「検査しない」になる | 読んだ: `agents.nix` の `timeout = 300`。内容は #991 |
| git-worktree-allow | PreToolUse(Bash、`if: Bash(git -C *)`) | 残す | 条件 1 に反する(allow を出す)。`if` の絞り込みが許可の範囲を決め、その評価は Claude Code の意味である。型付きの述語に写し直すと、複合コマンドでの許可の範囲が変わりうる。**白紙から選んでも、許可の範囲を決める文字列は、評価する側(Claude Code)に置く** | #991、本ドキュメントの旧「載せない」の節 |
| gh-edit-allow(判定役) | PreToolUse(Bash、`if: Bash(gh *)`) | 残す | 同上。記録役(PostToolUse の `gh-edit-allow-record`)は、判定を出さないので載せた | 同上 |
| pr-gate | Stop | 残す | 条件 1・3 に反する。CI の完了を最大 300 秒待ち(`PR_GATE_CI_TIMEOUT`、`timeout` は 600 秒)、出力は Stop の block。`stop_hook_active` を見ない設計(再呼び出しでも判定する) | 読んだ: `docs/agents/pr-gate.md:675`、`crates/pr-gate/src/lib.rs:62`、`agents.nix` の `timeout = 600` |
| session-wrapup(wrap-up inbox とレトロの Stop) | Stop | 残す | 条件 1 に反する。出力は終了コード 2 + 標準エラーで、1 つの入口の中でも `stop_hook_active` の扱いが違う(レトロは抜けない、wrap-up inbox は即 exit 0)。Stop のたびにネットワークを呼ぶ(#991) | 読んだ: `crates/session-wrapup/src/lib.rs:37-51`、`crates/session-retrospective/src/lib.rs:349`、`crates/issue-inbox/src/lib.rs:887` |
| main-checkout-guard(stop) | Stop | 残す | 条件 1 に反する(Stop の block)。`stop_hook_active` を読んで扱いを変える。事後検出で、触れた checkout の状態差分を取る | 読んだ: `crates/main-checkout-guard/src/main.rs:71`。ほかは #991 |
| agent-turn-log | Stop・UserPromptSubmit | 残す | ツール呼び出しに付かない(`tool_name` が無い)ので、`Tools` で絞る対象ではない。ルータの存在理由(ツール名の `matcher` の書き間違いで素通りになる事故)が当たらない | `agents.nix` の宣言(`matcher` なし) |
| SessionStart の 11 件(session-wrapup-start、issue-index、esa-mirror-context、daily-desk-reminder、worklog、gpg-prewarm、worktree-base-sync、herdr-status-advisory、gate-failopen-advisory、pr-gate-session-start、worktree-audit-context) | SessionStart | 残す | ツール呼び出しに付かない。ルータの `Event` は PreToolUse と PostToolUse の 2 つだけで、`render` は SessionStart の出力(複数の検査の `additionalContext` を 1 つにまとめる形)を持たない(条件 1 に反する)。`gpg-prewarm` は対話的で、`timeout` が 120 秒(条件 3 に反する)。`worktree-base-sync` は worktree を `git merge --ff-only` で動かす(条件 2 に反する) | 読んだ: `docs/agents/gpg-prewarm.md:145`、`docs/agents/plan-fresh-gate.md:12-14`(worktree-base-sync の動き)、`agents.nix` の宣言 |
| herdr-agent-metadata | SessionStart・UserPromptSubmit・PreToolUse・Stop・SessionEnd | 残す | herdr の hook は、pane ごとの状態と環境変数に依存する(この crate ではなく、herdr の側が持つ契約) | #991。`agents.nix` の宣言(5 イベント) |
| bleep | PreToolUse(`Bash|mcp__.*`) | 残す | 外部ツール(上流 `tarotene/bleep`、ADR-0009)。この crate の `hook` 関数に切り出せない | `agents.nix` の宣言、ADR-0009 |
| atuin-history | PreToolUse・PostToolUse・PostToolUseFailure(Bash) | 残す | 外部ツール(`atuin`)の入口で、この crate のものではない | `agents.nix` の宣言 |

読み方: 「残す」は、**今のところ載せない** であって、永久に載せない約束ではない。表の理由(条件 1〜4 のどれに
反するか)が解ける形になったら、載せる。たとえば Stop を載せるなら、`Outcome` に Stop の結果(block と
`stop_hook_active` の扱い)を型で足し、規則ごとの出力の 2 流儀(exit 2 + 標準エラーと、JSON の block)を
そろえる必要がある。そのときは ADR-872 の Amendment ではなく、ルータの契約を広げる ADR を立てる。

## 契約

旧構成は、検査ごとに別の hook を Claude Code が並列に走らせ、結果をまとめていた。ルータは同じ契約を、
プロセスの中で守る(`router.rs`)。

- **並列**: 当たる規則ごとに別スレッド。待ち時間は、最も遅い規則で決まる(合計ではない)。
- **規則ごとの打ち切り**: 各規則に、かつて個別の hook として宣言していた `timeout` と同じ時間
  (10〜20 秒、`Rule::budget`)を許す。越えた規則の結果は捨てて素通り(fail-open)にする。
- **panic の隔離**: 規則が panic しても、ほかの規則は走る。panic した規則は素通りにする。
- **素通りの知らせ**: panic・時間切れがあった呼び出しでは、応答に `systemMessage` を付ける(「次の検査を
  素通りにしました: …」)。あわせて、`gate-events.jsonl` に `fail-open` を記録する。
- **まとめ方**: PreToolUse は deny → ask → allow → 通常処理の順で採る。同じ判定の理由は表の順にすべて(空行で区切って)並べる。
  `additionalContext` は、表の順に改行でつなぐ。PostToolUse は判定を出さない。
- **判定できない入力**(JSON でない、`tool_name` が無い)は素通す。

各検査の判定は、それぞれの crate の `hook` 関数(`main` が標準入力を読んだあとの処理)で、`main` とルータの
両方が同じ関数を呼ぶ。standalone の bin の出力は変わらない(既存の trycmd の fixture が、そのまま通る)。
standalone の bin は、Copilot の登録と、手で叩く用に残してある。

## 旧登録の記録(matcher と timeout の理由)

旧登録のコメントにあった、設計の理由を残す。

- **複合 matcher `Bash|mcp__.*`**(attribution-guard・stack-base-guard・pr-title-guard・pr-followup-guard・
  decision-colocation-guard・repo-create-guard): `gh pr create` などの外向きの操作は、Bash でも MCP の GitHub
  ツールでも起こりうる。Bash 単体の matcher だと、MCP 接続の瞬間に無検査になる。→ `Tools::BashOrMcp`。
- **`mcp__.*` だけ**(external-send-guard): Gmail・Slack の送信系 MCP ツールだけが対象で、Bash 経由の送信は
  原理的に検出できない。→ `Tools::Mcp`。
- **広い matcher + 規則内部の判定**(routines-write-guard、new-tool-guard): `RemoteTrigger` や `Write` は Bash では
  ないので、`Bash(...)` の `if` 構文が使えない。対象外の呼び出しは、規則自身が `None` を返して素通りにする。
  → `Tools::Named(&["RemoteTrigger"])`、`Tools::Named(&["Write"])`。
- **`ExitPlanMode` の matcher**(plan-scope-gate・plan-precedent-gate、#991): 旧登録は、ほかの ExitPlanMode の
  検査(copilot-plan-review・plan-view・plan-fresh-gate)と同じ matcher の別エントリで、Claude Code が並列に
  走らせていた。→ `Tools::Named(&["ExitPlanMode"])`。Codex には ExitPlanMode のツール呼び出しが無い
  ([codex-plan-gate.md](../claude/codex-plan-gate.md))ので、`pre --agent codex` では当たらない。
- **timeout**: gh の往復を持たないもの(attribution-guard・pr-title-guard・pr-followup-guard・external-send-guard・
  plan-precedent-gate ほか)は 10 秒。`gh pr list` や git の走査を含む stack-base-guard・decision-colocation-guard、
  `gh api graphql` を呼ぶ plan-scope-gate は 20 秒、repo-create-guard は 15 秒。→ `Rule::budget` に同じ値。ルータ自身の `timeout` は、最大の 20 秒に起動と出力の
  余裕を足した 30 秒。

## `if` を外した影響(より厳しくなる向き)

旧登録で `if` を持っていた拒否の検査 3 件は、`if` の先頭一致のせいで、**複合コマンドの 2 つ目以降に対象が
あると、そもそも呼ばれなかった**(旧コメントの「既知のトレードオフ」):

| 検査 | 旧 `if` | 影響 |
|---|---|---|
| git-stash-guard | `Bash(git *)` | 規則自身が複合コマンドも解析する(理由文に「複合コマンドの中に git stash が含まれています」)。新しく見るのは、`git` で始まらないコマンドの中の `git stash`(`cd x && git stash` など) |
| rulesets-write-guard | `Bash(gh *)` | 厳しい語分割(`hook_io::shell::split`)は、複合コマンドを「判定できない」として素通りにする。旧と同じで、より緩くはならない |
| feedback-target-guard | `Bash(gh *)` | `guard-core` の複合コマンド対応の解析なので、`cd x && gh issue create --label feedback …` のような呼び出しを新しく検出する |

どれも、規則の判定そのものは変えず、規則に入る入力が増える(拒否が増えうる)だけで、許可が増える変更は無い。

## 移行

`agent-settings-reconcile` の所有の記録(`docs/agents/agent-settings-reconcile.md`)は、前回書いた登録を消す。記録の無い
ホスト(記録を入れた直後の最初の switch)は、前の世代が書いた 19 件の旧登録を知らないので、退役の一覧
(`agents.nix` の `retiredHookEntries`)に 19 件を挙げた(初回だけ消す。全ホストに記録ができたら、一覧ごと
消す、#979)。ExitPlanMode の 2 件(#991)も、同じ理由で、一覧に足した。

## 規則の足し方

1. 検査の crate に `pub fn hook(raw: &str) -> Option<hook_io::Outcome>` を置き、`main` はそれを呼んで、これまでと
   同じ書式で出力する(bin の出力を変えない)。
2. `rules.rs` の表に、`node`・`event`・`tools`・`budget`・`run` を持つ `Rule` を足す。`node` は
   `pipeline-map.toml` のノード id(無ければ足す)。
3. 表の集合を固定しているテスト(`the_table_holds_exactly_the_agreed_rules`)を、意識して直す。

許可(allow)を出す検査や、Stop・SessionStart の検査は、足さない(「載せなかった検査の裁定」の表の理由)。

## 実測

2026-10-24、x86_64-linux(14 コア)、`cargo build --release`(LTO off)、`hyperfine -N`(200 回、一時の HOME、
実リポジトリを cwd にした入力)。

| 呼び出し | 平均 [ms] | p99 [ms] |
|---|---:|---:|
| `hook-router pre`(Bash の `ls -la`、当たる規則 12 件) | 4.14 | 7.35 |
| `hook-router pre`(Read、当たる規則 1 件) | 2.17 | 3.06 |
| `hook-router post`(Bash、当たる規則 3 件) | 2.41 | 2.91 |
| 参考: 旧構成の `stack-base-guard` 単独 | 3.18 | 4.65 |
| 参考: 旧構成の `pr-title-guard` 単独 | 1.98 | 2.68 |

予算(hook の起動 50 ms、ADR-0024)の 1/6 以下。

プロセスの数(`ls` のような、`git`・`gh` で始まらない Bash の呼び出し 1 回): 旧構成は PreToolUse に 12 個
(検査 9 件 + bleep・herdr・atuin)と PostToolUse に 4 個(検査 3 件 + atuin)、新構成は PreToolUse に 4 個
(ルータ + bleep・herdr・atuin)と PostToolUse に 2 個(ルータ + atuin)。Claude Code 側のプロセス生成の
費用は、この実測に含まれない。
