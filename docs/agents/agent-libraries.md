# エージェント向け共通ライブラリ

機能別の CLI・hook が次のライブラリを直接使う。旧 `hook-io`・`guard-core` は廃止した。

| ライブラリ | 所有する機能 | 旧モジュール |
|---|---|---|
| agent-session | hook の入口(引数・標準入力・返答の出力)・判定・計画・会話とセッションの記録 | decision, entry, event, gate_event, input, ledger, outcome, plan, reply, transcript, hook |
| command-inspection | シェルの解析・コマンド範囲・ハッシュ | shell, command, cmd_hash |
| github-command | GitHub コマンドの引数・本文・本文マーカーの解析、PR・Issue の URL の認識（`url`: `gh-edit-allow` と `codex-plan` が共有する単一正本） | gh, marker, url |
| jq-json-compat | jq 互換の JSON 読み取り・表現 | jqfmt, jqparse, raw |
| process-control | プロセス起動・タイムアウト・実行ファイル探索 | proc |
| git-repository | Git の状態取得・操作と remote の解決 | git, repo |
| herdr-client | herdr との通信 | herdr |
| vault-note | Obsidian vault のノートの解決・一時ファイル + rename での書き換え・追記・排他・frontmatter・ノート名の規則(wrap-up の気づき・retro・裁定待ちの写し) | (新設) |

## hook の入口と返答は agent-session が持つ

hook の bin は、`agent_session::entry` を入口にする。標準入力の読み(`read_stdin`。UTF-8 でないバイトは置き換え、
末尾の改行は落とさない)と、`--agent` の解析(`parse_call`)、返答の出力(`emit`)を、ここ 1 か所が持つ。
典型の `main` は `run(&SPEC, 検査の関数)` の 1 行。手動モード(`--check`)や、標準入力を読まない bin は、
`enter` で引数だけを解析して、必要なときに `read_stdin` と `emit` を呼ぶ。

`--agent` は厳密に解析する。値が無い・未知・`Spec::agents` に無い・2 回以上、のどれも、標準エラーに理由を出して
終了コード 1 で終わる(終了コード 2 は Claude Code で「止める」の意味なので使わない)。従来は、未知の値が黙って
Claude の形に倒れていた。

返答は `Reply`(判定・`additionalContext`・`systemMessage`)で表し、`Reply::render(agent, event)` が JSON にする。
出力の形式は 1 つ: `jq .` と同じ整形(2 空白インデント、`"k": v`、キーは挿入順、末尾に改行)。Claude と Codex は同じ形で、
Copilot は PreToolUse の判定だけが平らな形。イベントごとの表は `crates/agent-session/src/reply.rs` の冒頭にある。
複数の検査の結果を 1 つにする規則(deny > ask > allow、理由は空行、文脈は改行でつなぐ)は
`Reply::from_outcomes` が持ち、`hook-router` の併合と同じ。

`command-inspection::simple_shell` は旧 hook-io の保守的な分割器、`command-inspection::shell` は
旧 guard-core のシェル解析器である。前者は解釈できない入力を返さず、後者は guard の検査範囲を切り出す。
同じ名前の関数でも戻り値の意味を揃えない。
`git-repository::repo` の GitHub remote フォールバックも、origin URL だけを読む API と区別する。

`process-control::output_with_timeout` は出力を捕捉し、`status_with_group_timeout` は呼出元の stdio を使う。
worktree 用の fetch は直下の子を打ち切り、親 checkout 用の fetch はプロセスグループを打ち切る。
`git-repository` の三つの fetch 入口で、この既存の違いを保持する。
対象ブランチ・TTL・通知・失敗後に進む条件は、各 CLI が決める。
`plan-fresh-gate` は計画の鮮度を判定し、checkout の更新操作を `git-repository` へ委ねる。

## 終了処理は責務別の実装を順に呼ぶ

| CLI / ライブラリ | 責務 | 引数 |
|---|---|---|
| issue-inbox | 発見事項の保存(vault のノート、vault の無い機体では jsonl)と、`--list-open` / `--set-status` による裁定の入口。Issue 化は `/my-cleanup` | 既存 inbox 操作フラグ(`--procedure` と `--chores-*` は削除) |
| session-retrospective | 振り返りの記録と、台帳から vault の `retro/` のノートへの書き出し(PR コメントは投稿しない) | 既存 --retro-*(`--retro-close` は `<session_id>` だけ) |
| feedback-trial | 期限付き試行の収集・承認・投稿・適用確認 | 旧 --trial の後ろのサブコマンド |
| session-wrapup | 上記の逐次実行 | start / stop |

Stop は背景処理待ち → trial → retrospective → 再入条件 → inbox の移行・判定集約・確認の順に動く。
Start は移行 → feedback stamp → inbox context → trial の順に動く。途中の終了条件も維持する。
旧 `wrapup-stop-gate` と `wrapup-session-start` は互換入口で、別の状態や手順本文を持たない。
trial の承認、投稿、重複抑止、適用確認は `feedback-trial` が同じ状態・マーカーで扱う。

実装の根拠: dotfiles, `5d499a9a06d5c4c4e3c76ec870f5760be57164ab` の
`crates/hook-io/src/`・`crates/guard-core/src/`・`crates/wrapup-stop-gate/src/lib.rs:869-1127`・
`crates/git-checkout-freshness/src/lib.rs`・`crates/worktree-fresh-base/src/main.rs` （取得 2026-10-26）。
https://github.com/tarotene/dotfiles/tree/5d499a9a06d5c4c4e3c76ec870f5760be57164ab/crates
