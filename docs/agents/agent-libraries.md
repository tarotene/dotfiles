# エージェント向け共通ライブラリ

機能別の CLI・hook が次のライブラリを直接使う。旧 `hook-io`・`guard-core` は廃止した。

| ライブラリ | 所有する機能 | 旧モジュール |
|---|---|---|
| agent-session | hook の入口(引数・標準入力・返答の出力)・判定・計画・会話とセッションの記録 | decision, entry, event, gate_event, input, ledger, outcome, plan, reply, transcript, hook |
| command-inspection | シェルの解析・コマンド範囲・ハッシュ | shell, command, cmd_hash |
| github-command | GitHub コマンドの引数・本文・本文マーカーの解析、PR・Issue の URL の認識（`url`: `gh-edit-allow` と `codex-plan` が共有する単一正本）、`gh` の実行（`exec`: stdin を閉じ stderr を捨てる呼び出し。`gh_ok`・`stacks`・`open_prs`） | gh, marker, url, exec |
| jq-json-compat | jq 互換の JSON 読み取り・表現 | jqfmt, jqparse, raw |
| process-control | プロセス起動・タイムアウト・実行ファイル探索 | proc |
| git-repository | Git の状態取得・操作と remote の解決 | git, repo |
| civil-time | unix 秒と暦(UTC と固定の +09:00=JST)の相互変換(`days_from_civil`・`civil_from_days`・`parse_day`・`jst_date`・`utc_timestamp` など)。依存ゼロ | (新設。旧 `agent-session::time`) |
| process-env | 環境変数の非空の読み(`${VAR:-}`)・`HOME`・XDG の基準ディレクトリの解決(`Xdg`)。依存ゼロ | (新設) |
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

逃げ道(guard・gate を一時的に止める手段)の名前・スコープ・真偽・案内は `escape::ALL` の表が唯一の正本で、
呼び出し単位は `<STEM>_ALLOW=1`(コマンドの語)、セッション単位は `SKIP_<STEM>=1`(環境変数。skip ファイルを持つ guard は
その存在も同じ意味)、真は `1` だけ。手動モード(`--check`)の出力と終了コードは `cli`(`pass` / `deny: …`、0・1・2)。

Stop の結果は、ルータの `Outcome` とは別の `StopOutcome`(`Block`・`Notice`。沈黙は `None`)で表す。ルータに載せない
(pr-gate が CI を最大 300 秒待つ)ことが、型でも守られる。`stop_hook_active` の扱いは検査ごとに `Reentry`
(`Quiet`: 立っていたら黙る、`Recheck`: 見ずに検査する)として宣言し、読みは `StopInput` だけが持つ。block は終了コード 0 の
JSON(`decision: "block"` と `reason`。`systemMessage` を添えられる)で運ぶ。ブロックしない hook エラーは `Fault` で、終了コード 0 と 2
は作れない(2 は「止める」の意味)。Stop の bin の入口は `entry::run_stop`(単一の検査)か、`entry::finish_stop`(サブコマンドを
持つ bin)。

返答の封筒の文字列(`hookSpecificOutput`)は `agent-session` だけが書き、hook の bin は標準入力を `read_stdin` で読む。
`crates/repo-structure-check` の `hook_io` の検査が、`src/` に封筒の文字列を書いた crate と、標準入力を直接読む bin を落とす。
例外は、外から来た JSON を読む 3 つの crate(`codex-plan-gate`・`copilot-plan-review`・`hook-router` の Codex 変換)と、
hook でない CLI と遅延読みの 5 つの bin で、理由つきで `hook_io.rs` の定数に列挙してある。例外のファイルが
違反の形を失ったら、例外のほうが落ちる。

unix 秒と暦日の変換は `civil-time` が持つ。Howard Hinnant の変換の定数(`719468` と `146097`)を `civil-time` の外の
`crates/*/src` に書くと、`crates/repo-structure-check` の `civil_time` の検査が落ちる(例外は無い)。時刻帯のデータベースは持たない。

環境変数と基準ディレクトリは `process-env` が持つ。`process_env::var_nonempty`(空を未設定として扱う)、`process_env::home`、
`process_env::Xdg::{Config,State,Cache,Data}` の `resolve`(`HOME` が無ければ `None`)・`resolve_lossy`(空の `HOME` として続ける)・
`resolve_under(home)`(検査済みの `HOME` を使う)。各機能の永続状態のパス(`<基準>/claude/wrapup` など)は、機能が持つ。
`crates/repo-structure-check` の `process_env` の検査が、XDG の変数名の文字列と、補助関数の複写を、例外を除いて落とす。

`command-inspection::simple_shell` は旧 hook-io の保守的な分割器、`command-inspection::shell` は
旧 guard-core のシェル解析器である。前者は解釈できない入力を返さず、後者は guard の検査範囲を切り出す。
同じ名前の関数でも戻り値の意味を揃えない。
`git-repository::repo` の GitHub remote フォールバックも、origin URL だけを読む API と区別する。

`process-control::output_with_timeout` は出力を捕捉し、`status_with_group_timeout` は呼出元の stdio を使う。
worktree 用の fetch は直下の子を打ち切り、親 checkout 用の fetch はプロセスグループを打ち切る。
`git-repository` の三つの fetch 入口で、この既存の違いを保持する。

`git -C <dir> …` を静かに(stdin を閉じ、stderr を捨てて)呼ぶ核は `git_repository::{run, raw, out, ok}` が持つ。`out` は末尾の
`\n` だけを落とし、`raw` はバイト列をそのまま返す(`-z` の出力や、ファイルの内容を読む呼び出しは `raw`)。`gh` の同じ形の呼び出しは
`github_command::exec`。stdout や stderr を引き継ぐ呼び出し、timeout を持つ呼び出し、stdin を引き継ぐ呼び出し(`feedback-trial`・
`detect-drift` など)は、挙動が違うので各 crate が持つ。

`jq -r .` の 1 値の文字列化は `jq_json_compat::raw::jq_raw` が持つ(null と false は `None`、配列とオブジェクトは pretty で出す)。
挙動が違う 3 つの複写は、各 crate が持つ。`external-send-guard` の版は、null と false を `"null"`・`"false"` として返す(配列の
要素を渡すので、置き換えると null の要素が消える)。`herdr-agent-metadata` と `copilot-plan-review` の版は、配列とオブジェクトを
compact で出す。実行ファイルの探索は `process_control::command_exists`(実行ビットを見る)が持つ。`update-own-tools` の `on_path` は
実行ビットを見ず(`is_file` だけ)、`issue-inbox` の `on_path` は見つけたパスを返すので、どちらも各 crate が持つ。手で書いた一時
ファイルと rename を `tempfile`(`Builder::tempfile_in` と `persist`)に寄せると、権限が 0600 にそろい(`codex-plan-gate`・`hms`・
`worktree-lease` で変わる)、`TMPDIR` が空のときの扱いも変わりうるので、置き換えていない(`gpg-subkey`・`claude-plan-model`・
`claude-usage`・`codex-plan-gate` ほか)。これらを寄せるには、共有側に別の関数を足すか(`jq -r .` の compact 版・`find_executable`)、
挙動の変化を許すかを、箇所ごとに裁定する必要がある(意図は決まっていて、未着手)。箇所の一覧は、#1445(段12 の複写の整理。
閉じて、ここへ移した)の 2026-11-10 のコメントにある。
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
