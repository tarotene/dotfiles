# wrap-up inbox — スコープ外の気づきを自動で Issue 化する hook 機構

作業中に出た「今回のタスクのスコープ外だが起票価値のある気づき」が揮発する問題への
対処。**収集**(セッション中に inbox へ溜める)と**起票**(`gh issue create`)を分離し、
Claude Code の hook 2 本で回す。

inbox に流すのは、現在進行中の変更と依存関係(`stacked-pr` スキル §1 の判定条件)が
無い気づきに限る。依存があれば stacked PR の追加提案段として受ける選択肢が先に
出るため、この文書が扱うのは「stack せず inbox に落ち着いた」気づきの以降の経路
(SessionStart 注入・Stop ゲート・起票)である。振り分け自体の設計根拠は
`docs/claude/stacked-pr.md`「スコープ外発見を stack の一段として受ける入口」を
参照。

| 部品 | イベント | 役割 |
|------|----------|------|
| `wrapup-session-start.sh` | SessionStart | 「気づきは inbox に `--add` で追記せよ」を `additionalContext` で注入。未処理件数も掲示 |
| `wrapup-stop-gate.sh` | Stop | inbox 非空かつ起票可能なら exit 2 + stderr 指示で、本体 Claude に起票させる |

配備は `home/modules/claude.nix`(スクリプトは `home.file`、`~/.claude/settings.json`
への登録は activation 時の冪等 jq マージ)。全ホスト共通。

## なぜ SessionEnd ではないのか

セッション終了時に起票エージェントを走らせる案は、ドキュメント上の制約で成立しない:

- `SessionEnd` は decision control を持たない。副作用専用で、exit 2 でも何もブロック
  できず、stderr はユーザーにしか見えない — 子エージェントの結果を渡す先がない。
- `SessionEnd` の hook は短い時間予算を共有する(設定で引き上げても上限 60 秒)。

「セッションが死んでいく最中に、時間制限つきで、結果を誰にも報告できない形で」外部
副作用(Issue 作成)を賭けることになるため、採らない。

`Stop` は逆に exit 2 で会話を継続させられ、フルコンテキストと実ツールを持った本体
Claude に起票させられる。起票の実行主体が LLM 本体なので、Issue 本文に発見時の文脈を
盛り込めるのも Stop 側の利点。

## 設計判断(/grilling で確定)

| 論点 | 決定 | 根拠 |
|------|------|------|
| スコープ | user グローバル(`~/.claude/settings.json`) | 気づきの Issue 化はプロジェクト非依存。home-manager 配備で全ホストに自動展開 |
| 粒度 | ターン単位・session_id フィルタなし | 気づいた直後の起票が最も文脈が濃い。残骸はどのセッションでも自己修復的に回収される |
| 収集指示の経路 | SessionStart `additionalContext` 注入 | グローバル CLAUDE.md を store symlink にすると `#` メモリ追記が壊れる。skill は受動的ルールの担い手として発火が不確実 |
| 起票先 | 作業中プロジェクト自身のリポジトリ | スコープ外の気づきもそのリポジトリの事象。起票は hook でなく本体 Claude が行う |
| 縮退 | gh 不在 / git repo 外 / GitHub remote 不在 なら黙って exit 0 | ADR-0005 の binary-existence gating。inbox は残り、条件が揃う環境・セッションで回収される。remote 判定は `git remote -v` の `github.` マッチ(静的検査のみ、hook 内でネットワークに出ない) |
| スキーマ | 最小 JSONL `{"ts", "title", "detail"}`・ラベルなし | 存在しないラベルは `gh issue create` を落とす。出自は本文フッター「🤖 Filed from [Claude Code](https://claude.com/claude-code) wrap-up inbox」で検索可能にする(このリンクが attribution-guard.sh の生成元表示要求も兼ねる、`attribution-guard.md`「wrap-up inbox の出自フッターは生成元表示を兼ねる」参照) |
| inbox 置き場所 | `${XDG_STATE_HOME:-~/.local/state}/claude/wrapup/<slug>.jsonl` | user グローバル機構が任意のリポジトリ(会社リポジトリ含む)の作業ツリーに未追跡ファイルを生やすのは誤コミットリスク |
| slug のキー | 原則リポジトリ単位(`remote.origin.url` の正規化)。remote なし・git repo 外はプロジェクト絶対パスの `/` `.` → `-` 置換にフォールバック | 旧・絶対パス slug は同一リポジトリでも worktree ごとに inbox が分散し、worktree 削除後は orphan 化していた(実測: dotfiles で 3 個以上の分散 inbox、うち 1 個が並行セッション中に無言で 0 バイト化してデータ喪失)。remote URL 単位に正規化すれば worktree 間で inbox を共有できる |

## inbox の整合性(Codex レビューで確定)

inbox の書き込みを LLM の自由編集に任せると、重複起票・失敗行の誤削除・並行セッション
との競合を検証できない。そこでリスクのある操作をすべて `wrapup-stop-gate.sh` の
決定論的サブコマンドに寄せ、selftest(CI)で回帰テストする:

- `--add <inbox> <json1行>` — `mkdir -p` + JSON 検証 + flock 追記。初回利用時の
  ディレクトリ不在で沈黙する事故を防ぐ。
- `--check-dup "<title>"` — `gh issue list --search` で同名 open Issue を検査。
  exit 1 = 重複、exit 3 = 判定不能(その行は残す)。
- `--mark-filed <inbox> <json1行>` — **行全体の完全一致で先頭 1 行だけ**削除
  (`ts` は一意性を保証しないため削除キーにしない)。flock + tmp+mv。削除した
  行は `<inbox>.filed.jsonl` に tombstone として追記してから mv する(#297
  — 起票済み・重複スキップいずれの削除も無検証の一撃消去にしないため)。
  削除対象が無い呼び出しは rewrite 自体をせず tombstone にも追記しない
  (no-op 検出)。mv 前に元ファイルのパーミッションを引き継ぐ(mktemp 既定の
  0600 化を防ぐ)。

flock は `--add` と `--mark-filed` の両方が取る。session_id フィルタを捨てた設計では
複数セッションが同じ inbox に触るため、追記と置換の競合(行の消失・復活)を排他で防ぐ。

Stop ゲートの stderr 指示は「起票成功または重複スキップした行だけ `--mark-filed`、
失敗行は残して次ターンで再試行、直接編集は禁止」と明示する。

## slug のリポジトリ単位化と自己修復マージ

worktree ごとに絶対パスが変わる旧 slug 方式では、同一リポジトリでも inbox が
worktree の数だけ分散し、worktree 削除後は誰も読まない orphan として残り続けた。
`repo_slug()` は `git config --get remote.origin.url` を正規化して
`<host>-<owner>-<repo>` 形式の slug を作る(scheme・認証情報・`.git` suffix・
大文字小文字・https/ssh/scp 表記の差を吸収)。remote が無い、または git repo 外
なら従来の絶対パス slug にフォールバックする。

過去に書かれた旧 slug の inbox を回収するため、`--migrate <project-dir>`
サブコマンド(Stop hook 本体と `wrapup-session-start.sh` の双方が毎回呼ぶ)が
自己修復マージを行う。安全性はこのリポジトリの inbox 整合性モデル(上記
「inbox の整合性」節)を壊さないことを最優先に設計している:

- 使用中になり得る `<inbox>.lock` は削除・再作成しない(#297 でこの主張と
  実装の不一致を修正済み — 修正前は空ファイル判定分岐・全行確認済み分岐の
  両方で `rm -f "$legacy" "$legacy.lock"` と lock も unlink していた)。
  unlink すると、既にその lock を open/flock 待ちしている別プロセスとの
  相互排他が壊れる(unlink 後は同名で新しい inode の lock が作られるため、
  旧 inode を握ったままの保持者と競合しなくなる)。orphan のまま残る lock
  ファイルは空なので実害はない。新旧両方の lock を取るのはこのマージだけで、
  全呼び出し箇所が「新 → 旧」の同一順序で取得するためデッドロックしない。
- 空ファイル判定もロック取得後に行う(空判定直後に並行 `--add` が書いた行を
  消さないため)。
- 新ファイルへは append のみ(truncate/rewrite する経路を持たない)。dedup は
  行全体の完全一致(この機構の行同一性モデルそのもの。`ts`+`title` 一致だと
  `detail` の異なる行を落とすため使わない)。
- 旧の全行が新に存在することを確認できたときだけ旧本体を削除する(lock は
  上記の通り削除しない)。1 行でも欠ければ残して次回呼び出し(次セッション)
  に再試行を委ねる(冪等)。

移行後も、旧パスを埋め込んだ SessionStart 注入を持つ長命セッションが
リテラル旧パスへ `--add` して旧ファイルを再作成することがあり得るが、
次にどのセッションの hook が起動しても `--migrate` が毎回走るため自己修復
される。

## 検証

- `bash config/claude/hooks/wrapup-stop-gate.sh --selftest` — 縮退ゲート・
  `--add`/`--mark-filed`/`--check-dup`・SessionStart 注入の回帰テスト(CI の
  `ci.yml` でも実行)。
- 手動 E2E は inbox にダミー行を `--add` して新しいセッションを開始する:
  SessionStart 注入に未処理件数が出て、ターン終了時に Stop ゲートが発火する。

## feedback 型 auto memory の経路(#328)

wrap-up inbox は「今回のタスクのスコープ外だが Issue 起票の価値がある気づき」
だけを対象にしており、ユーザーから受けた作業方針上のフィードバックを Claude
Code の auto memory(`~/.claude/projects/*/memory/*.md`、frontmatter
`metadata.type: feedback`)に保存するケースは対象外だった。実測
(2026-09-23)では、ローカル auto memory の `type: feedback` ファイル 17 件中
14 件が Issue 化されずローカルに閉じたままだった。

`wrapup-stop-gate.sh` の同じ Stop 本体に、inbox とは独立した第二の検査を
足した: 今セッション中に更新された `type: feedback` メモリで `#N`(Issue
番号)参照が無いものを検出し、inbox が空でも単独でゲートを発火させる。
「今セッション」の境界は `wrapup-session-start.sh` が touch する stamp
ファイル(`~/.claude/wrapup-stop-gate/feedback-session/<session_id>.stamp`)
の mtime を基準に `find -newer` で判定する(GNU/BSD 両対応、epoch 文字列を
扱わない)。stamp が無い(SessionStart 未実行など)場合は判定不能として
何もしない側に倒す(ADR-0005 と同じ fail-open)。

原則そのもの(不可視なローカルメモに閉じ込めない)は共有 AGENTS.md、
auto memory 固有の配線は `config/claude/CLAUDE.md` に持つ。

## 消化経路は 2 つ

上記の Stop ゲートによる個別起票に加えて、判断を要さない軽微な項目をまとめて片す
[`wrapup-chores` スキル](wrapup-chores.md)がある。どちらの経路でも inbox からの
削除は `--mark-filed` のみを介する(直接編集は禁止のまま)。個別起票は「気づいた
文脈が濃いうちに Issue へ記録する」ため、chores スキルは「判断無しで即対処できる
ものを 1 つの PR でまとめて消化する」ためのもので、役割は排他ではなく補完である。
