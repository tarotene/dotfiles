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

## 載せるもの・載せないもの

載せる: 共通権限の PreToolUse 検査、拒否の検査、記録役の PostToolUse(表は `rules.rs`)。

| イベント | 検査 |
|---|---|
| PreToolUse | agent-permissions、attribution-guard、decision-colocation-guard、external-send-guard、feedback-target-guard、git-stash-guard、main-checkout-guard-pre、new-tool-guard、pkexec-guard、pr-followup-guard、pr-title-guard、repo-create-guard、routines-write-guard、rulesets-write-guard、stack-base-guard、worktree-create-guard |
| PostToolUse | adr-number、cmd-hash-log、gh-edit-allow-record、new-tool-guard-record |

載せない(今の登録のまま)と、その理由:

- **許可(allow)を出す検査**(`git-worktree-allow`、`gh-edit-allow` の判定役)。`if`(`Bash(git -C *)`、
  `Bash(gh *)`)の絞り込みが、許可の範囲を決めている。`if` の評価は Claude Code の意味なので、ここで
  再実装して許可の範囲を変えるリスクを取らない。既存の拒否検査の `Outcome::from_decision`
  は、allow / ask を `None` にする契約を維持する。共通権限の検査は専用の `Outcome::Ask` を使う。
- **Stop・SessionStart・ExitPlanMode の検査**。時間のかかるもの(`pr-gate` の Stop は CI の完了を最大 5 分待つ、
  `wrapup-stop-gate` は Stop のたびにネットワークを呼ぶ、`gpg-prewarm` は対話的で最大 180 秒)や、作業ツリーを
  書き換えるもの(`worktree-base-sync`、`plan-fresh-gate`)、プロセス全体の状態を触るもの(`plan-view` の umask と
  切り離したブラウザ)がある。1 つのルータにまとめると hook の `timeout` が 1 つになり、規則ごとの打ち切りを
  自前で作る必要がある。Stop の出力は 2 流儀(exit 2 + 標準エラーと、JSON の block)が混ざり、`stop_hook_active` の
  扱いも規則ごとに違う。載せるかどうかは、別に決める(Issue)。
- **herdr・外部ツール**(bleep、atuin、copilot-plan-review)。herdr の hook は pane ごとの状態と環境変数に
  依存する。外部ツールは、こちらの crate ではない。

## 契約

旧構成は、検査ごとに別の hook を Claude Code が並列に走らせ、結果をまとめていた。ルータは同じ契約を、
プロセスの中で守る(`router.rs`)。

- **並列**: 当たる規則ごとに別スレッド。待ち時間は、最も遅い規則で決まる(合計ではない)。
- **規則ごとの打ち切り**: 各規則に、かつて個別の hook として宣言していた `timeout` と同じ時間
  (10〜20 秒、`Rule::budget`)を許す。越えた規則の結果は捨てて素通り(fail-open)にする。
  `agent-permissions` だけは panic・時間切れ・スレッド作成失敗時に拒否する。スレッドは外から
  止められないので、応答を出したあと、プロセスごと終わる。権限の記録とのずれのうち、追記された許可は
  待機として通し、構造的なずれだけを拒否する。拒否中も、質問と読み取り専用の診断だけは通す
  ([agent-permissions.md](../agent-permissions.md#追記された許可は待機として扱い構造的なずれだけを止める))。
- **panic の隔離**: 規則が panic しても、ほかの規則は走る。panic した規則は素通りにする。
- **素通りの知らせ**: panic・時間切れがあった呼び出しでは、応答に `systemMessage` を付ける(「次の検査を
  素通りにしました: …」)。あわせて、`gate-events.jsonl` に `fail-open` を記録する。
- **まとめ方**: PreToolUse は deny → ask → allow → 通常処理の順で採る。同じ判定の理由は表の順にすべて(空行で区切って)並べる。
  `additionalContext` は、表の順に改行でつなぐ。PostToolUse は判定を出さない。
- **判定できない入力**(JSON でない、`tool_name` が無い)も共通権限の検査へ渡す。
  権限検査は `All` として、Codex のパッチ展開前の生入力で一回だけ走る。ほかの検査は従来どおり素通す。
  全検査を同じ並列バッチで走らせるため、権限検査の予算をほかの検査の待ち時間に加算しない。
  Codex の PermissionRequest による限定 allow にも、同じ権限検査を先に適用する。

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
- **timeout**: gh の往復を持たないもの(attribution-guard・pr-title-guard・pr-followup-guard・external-send-guard
  ほか)は 10 秒。`gh pr list` や git の走査を含む stack-base-guard・decision-colocation-guard は 20 秒、
  repo-create-guard は 15 秒。→ `Rule::budget` に同じ値。ルータ自身の `timeout` は、最大の 20 秒に起動と出力の
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
ホスト(記録を入れた直後の最初の switch)は、前の世代が書いた 19 件の旧登録を知らないので、`claude.nix` の
退役の一覧に 19 件を挙げた(初回だけ消す。全ホストに記録ができたら、一覧ごと消す、#979)。

## 規則の足し方

1. 検査の crate に `pub fn hook(raw: &str) -> Option<hook_io::Outcome>` を置き、`main` はそれを呼んで、これまでと
   同じ書式で出力する(bin の出力を変えない)。
2. `rules.rs` の表に、`node`・`event`・`tools`・`budget`・`run` を持つ `Rule` を足す。`node` は
   `pipeline-map.toml` のノード id(無ければ足す)。
3. 表の集合を固定しているテスト(`the_table_holds_exactly_the_agreed_rules`)を、意識して直す。

許可(allow)を出す検査や、Stop・SessionStart の検査は、足さない(上の理由)。

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
