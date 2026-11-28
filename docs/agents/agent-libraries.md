# エージェント向け共通ライブラリ

機能別の CLI・hook は次のライブラリを直接使う。旧 `hook-io`・`guard-core` は廃止した。

| ライブラリ | 所有する機能 | 旧モジュール |
|---|---|---|
| agent-session | hook の入口(引数・標準入力・返答の出力)・判定・計画・会話とセッションの記録 | decision, entry, event, gate_event, input, ledger, outcome, plan, reply, transcript, hook |
| command-inspection | シェルの解析・コマンド範囲・ハッシュ | shell, command, cmd_hash |
| github-command | GitHub コマンドの引数・本文・本文マーカーの解析、PR・Issue の URL 認識(`url`: `gh-edit-allow` と `codex-plan` が共有する単一正本)、`gh` の実行(`exec`: stdin を閉じ stderr を捨てる呼び出し。`gh_ok`・`stacks`・`open_prs`) | gh, marker, url, exec |
| jq-json-compat | jq 互換の JSON 読み取り・表現 | jqfmt, jqparse, raw |
| process-control | プロセス起動・タイムアウト・実行ファイル探索 | proc |
| git-repository | Git の状態取得・操作と remote の解決 | git, repo |
| civil-time | unix 秒と暦(UTC と固定の +09:00=JST)の相互変換(`days_from_civil`・`civil_from_days`・`parse_day`・`jst_date`・`utc_timestamp` など)。依存ゼロ | (新設。旧 `agent-session::time`) |
| process-env | 環境変数の非空判定(`${VAR:-}`)・`HOME`・XDG 基準ディレクトリの解決(`Xdg`)。依存ゼロ | (新設) |
| herdr-client | herdr との通信 | herdr |
| app-window | ファイルを Chrome の専用ウィンドウ(`--app=file://…`)で切り離して(待たずに)開く。表示可否(画面・ブラウザ・darwin の `.app`)の判定も持つ。`plan-view` と `show-image` が共有する(`docs/agents/plan-view.md`・`docs/agents/show-image.md`) | (新設。旧 `plan-view` の `open_window`・`has_display`・`browser_available`) |
| vault-note | Obsidian vault のノート解決・一時ファイル + rename での書き換え・追記・排他・frontmatter・ノート名規則(wrap-up の気づき・retro・裁定待ちの写し) | (新設) |

## hook の入口と返答は agent-session が持つ

hook のバイナリは `agent_session::entry` を入口とする。標準入力の読み込み(`read_stdin`。UTF-8 でないバイトは置き換え、末尾の改行は落とさない)、`--agent` の解析(`parse_call`)、返答の出力(`emit`)をここ 1 か所に集約している。
典型的な `main` は `run(&SPEC, 検査関数)` の 1 行で済む。手動モード(`--check`)や標準入力を読まないバイナリは、`enter` で引数だけを解析し、必要なときに `read_stdin` と `emit` を呼ぶ。

`--agent` は厳密に解析する。値がない・未知の値・`Spec::agents` にない・2 回以上指定された場合はいずれも、標準エラーに理由を出して終了コード 1 で終了する(終了コード 2 は Claude Code で「停止」を意味するため使わない)。従来は未知の値が黙って Claude 向けとして処理されていた。

返答は `Reply`(判定・`additionalContext`・`systemMessage`)で表し、`Reply::render(agent, event)` で JSON に変換する。
出力形式は `jq .` と同じ整形(2 空白インデント、`"k": v`、キーは挿入順、末尾に改行)に統一している。Claude と Codex は同じ形式、
Copilot は PreToolUse の判定のみネストのないフラットな形式を用いる。Antigravity は封筒もキーも別(`antigravity::render`)であり、入力は検査の手前で `antigravity::normalize` が Claude の形式に変換する。イベントごとの表は `crates/agent-session/src/reply.rs` の冒頭にある。
複数の検査結果を 1 つにまとめる規則(deny > ask > allow、理由は空行、文脈は改行でつなぐ)は
`Reply::from_outcomes` が持ち、`hook-router` の併合規則と同じである。

逃げ道(guard・gate を一時的に無効化する手段)の名前・スコープ・真偽値・案内文は `escape::ALL` の表が唯一の正本である。呼び出し単位は `<STEM>_ALLOW=1`(コマンドの語)、セッション単位は `SKIP_<STEM>=1`(環境変数。skip ファイルを持つ guard はその存在も同じ意味)、真値は `1` のみ。手動モード(`--check`)の出力と終了コードは `cli`(`pass` / `deny: …`、0・1・2)が担う。

Stop の結果はルータの `Outcome` とは別の `StopOutcome`(`Block`・`Notice`。無出力は `None`)で表す。ルータに載せない(`pr-gate` が CI を最大 300 秒待つ)ことが型レベルでも保証される。`stop_hook_active` の扱いは検査ごとに `Reentry`(`Quiet`: フラグが立っていたら黙る、`Recheck`: 参照せずに検査する)として宣言し、読み取りは `StopInput` だけが行う。block は終了コード 0 の JSON(`decision: "block"` と `reason`、`systemMessage` を添えられる)で返す。ブロックしない hook エラーは `Fault` であり、終了コード 0 と 2 は返せない(2 は「停止」の意味)。Stop バイナリの入口は `entry::run_stop`(単一検査)または `entry::finish_stop`(サブコマンドを持つバイナリ)である。

返答の封筒文字列(`hookSpecificOutput`)は `agent-session` だけが生成し、hook バイナリは標準入力を `read_stdin` で読む。
`crates/repo-structure-check` の `hook_io` 検査が、`src/` に封筒文字列を直接書いた crate と、標準入力を直接読むバイナリを検出して失敗させる。
例外は外部由来の JSON を読む 3 つの crate(`codex-plan-gate`・`copilot-plan-review`・`hook-router` の Codex 変換)と、hook ではない CLI や遅延読み込みを行う 5 つのバイナリのみで、理由付きで `hook_io.rs` の定数に列挙している。例外ファイルから違反パターンがなくなれば、例外定義側の検査が失敗する。

unix 秒と暦日の変換は `civil-time` が持つ。Howard Hinnant の変換定数(`719468` と `146097`)を `civil-time` 以外の `crates/*/src` に書くと、`crates/repo-structure-check` の `civil_time` 検査が失敗する(例外はない)。タイムゾーンのデータベースは持たない。

環境変数と基準ディレクトリの解決は `process-env` が担う。`process_env::var_nonempty`(空文字列を未設定として扱う)、`process_env::home`、`process_env::Xdg::{Config,State,Cache,Data}` の `resolve`(`HOME` がなければ `None`)・`resolve_lossy`(空の `HOME` として続行)・`resolve_under(home)`(検査済みの `HOME` を使用)を提供する。各機能の永続状態のパス(`<基準>/claude/wrapup` など)は機能側が持つ。
`crates/repo-structure-check` の `process_env` 検査が、XDG 変数名の文字列リテラルや補助関数の重複定義を、例外を除いて禁止する。

機体の Claude 契約の側(個人 / 会社)は `dotfiles-identity::verify_from_env` が判定し、その側と所有者の表(GitHub ログイン名 → 側)・リポジトリの可視性(`gh api repos/<o>/<r>`、1 日キャッシュ付き)から「読んでよいリポジトリか」を決める判定は `repo-scope`(`Policy::check_repo`・`check_owner`)が持つ。`repo-scope-guard`(拒否)と、所有者を横断して集計するツール群が共有する(`docs/agents/repo-scope-guard.md`)。いずれも責務表の 11 ライブラリ外にある「同じ目的の共有 crate」(`deps::LIBRARIES`)である。

`command-inspection::simple_shell` は旧 hook-io の保守的な分割器、`command-inspection::shell` は旧 guard-core のシェル解析器である。前者は解釈できない入力を返さず、後者は guard の検査範囲を切り出す。
同じ名前の関数でも戻り値の意味を揃えない。
`git-repository::repo` の GitHub remote フォールバックも、origin URL だけを読む API と区別する。

`process-control::output_with_timeout` は出力を捕捉し、`status_with_group_timeout` は呼び出し元の stdio を使う。
worktree 用の fetch は直下の子プロセスを打ち切り、親 checkout 用の fetch はプロセスグループを打ち切る。
`git-repository` の 3 つの fetch 入口で、この既存の違いを保持する。

`git -C <dir> …` を静かに(stdin を閉じ、stderr を捨てて)呼ぶ核は `git_repository::{run, raw, out, ok}` が持つ。`out` は末尾の `\n` だけを取り除き、`raw` はバイト列をそのまま返す(`-z` の出力やファイル内容を読む呼び出しは `raw` を使用する)。`gh` の同型呼び出しは `github_command::exec` が担う。stdout や stderr を引き継ぐ呼び出し、timeout を持つ呼び出し、stdin を引き継ぐ呼び出し(`feedback-trial`・`detect-drift` など)は挙動が異なるため各 crate が持つ。

`jq -r .` の単一値文字列化は `jq_json_compat::raw::jq_raw` が持つ(null と false は `None`、配列とオブジェクトは整形して返す)。
挙動が異なる 3 つの重複実装は各 crate に残している。`external-send-guard` 版は null と false を `"null"`・`"false"` として返す(配列要素を渡すため置き換えると null 要素が消える)。`herdr-agent-metadata` と `copilot-plan-review` 版は配列とオブジェクトをコンパクト形式で出力する。実行ファイルの探索は `process_control::command_exists`(実行ビットを確認する)が担う。`update-own-tools` の `on_path` は実行ビットを見ず(`is_file` のみ)、`issue-inbox` の `on_path` は見つかったパスを返すため、いずれも各 crate に残している。自前の一時ファイル作成と rename を `tempfile`(`Builder::tempfile_in` と `persist`)へ寄せると、権限が 0600 に揃い(`codex-plan-gate`・`hms`・`worktree-lease` で挙動が変わる)、`TMPDIR` が空のときの扱いも変わり得るため、置き換えていない(`gpg-subkey`・`claude-plan-model`・`claude-usage`・`codex-plan-gate` ほか)。これらを統一するには、共有側に別関数を追加するか(`jq -r .` のコンパクト版・`find_executable`)、挙動の変化を許容するかを箇所ごとに判断する必要がある(着手予定あり)。該当箇所の一覧は #1445(段12 の重複整理。Issue は閉じてここへ移管)の 2026-11-10 コメントにある。
対象ブランチ・TTL・通知・失敗後に続行する条件は各 CLI が決める。
`plan-fresh-gate` は計画の鮮度を判定し、checkout の更新操作を `git-repository` へ委ねる。

## 終了処理は責務別の実装を順に呼ぶ

| CLI / ライブラリ | 責務 | 引数 |
|---|---|---|
| issue-inbox | 発見事項の保存(vault のノート、vault のない機体では jsonl)と、`--list-open` / `--set-status` による裁定の入口。Issue 化は `/my-cleanup` | 既存 inbox 操作フラグ(`--procedure` と `--chores-*` は削除) |
| session-retrospective | 振り返りの記録と、台帳から vault の `retro/` ノートへの書き出し(PR コメントは投稿しない) | 既存 --retro-*(`--retro-close` は `<session_id>` のみ) |
| feedback-trial | 期限付き試行の収集・承認・投稿・適用確認 | 旧 --trial 以降のサブコマンド |
| session-wrapup | 上記の逐次実行 | start / stop |

Stop は背景処理待ち → trial → retrospective → 再入条件 → inbox の移行・判定集約・確認の順に動く。
Start は移行 → feedback stamp → inbox context → trial の順に動く。途中の終了条件も維持する。
旧 `wrapup-stop-gate` と `wrapup-session-start` は互換入口であり、別の状態や手順本文を持たない。
trial の承認・投稿・重複抑止・適用確認は、`feedback-trial` が同じ状態・マーカーで扱う。

実装の根拠: dotfiles, `5d499a9a06d5c4c4e3c76ec870f5760be57164ab` の `crates/hook-io/src/`・`crates/guard-core/src/`・`crates/wrapup-stop-gate/src/lib.rs:869-1127`・`crates/git-checkout-freshness/src/lib.rs`・`crates/worktree-fresh-base/src/main.rs`(取得 2026-10-26)。
https://github.com/tarotene/dotfiles/tree/5d499a9a06d5c4c4e3c76ec870f5760be57164ab/crates
