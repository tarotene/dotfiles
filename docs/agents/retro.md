# 作業終了時のレトロ(ADR-697)

現行の正準入口は `session-wrapup start|stop`。保存と起票は `issue-inbox`、振り返りは
`session-retrospective`、期限付き試行は `feedback-trial` が担当する。
以下の旧 `wrapup-stop-gate` / `wrapup-session-start` による説明は、引き続き使える互換入口と移行の経緯を示す。
実装の配置と引数は[責務一覧](agent-libraries.md)を参照する。


PR を作ったセッション、またはその場で書いたコードの痕跡(下の「その場のコードの痕跡」)がしきい値を超えた
セッションの最初の Stop で、`wrapup-stop-gate` がセッション全体の振り返りを求める。全行は、`--retro-close` が vault の `retro/` に 1 件のノートとして書き出す(PR にはコメントしない。
ADR-1136 の段2、ADR-697 の Amendment)。出口(inbox → `/backlog-cleanup` での裁定)は [wrap-up inbox](wrapup-inbox.md) と
共有し、入口と強制だけが別。`disposition: inbox` の行は Issue にならず、vault のノートに溜まる。決定の理由と代替案は
[ADR-697](../adr/697-retro-stop-gate.md)。

## 流れ

1. PR を作成する(gh-edit-allow の台帳に `pr ` 行が積まれる。Codex は台帳が無いので現ブランチの
   PR のうちセッション開始後に作られたもの)。または、その場で書いたコードの痕跡がしきい値を超える。
2. 次の Stop で `kind="retro"` のポインタが出る。モデルは
   `session-retrospective --retro-procedure <session_id>` を実行して手順書を読む。
3. 手順書には、行に引くべき出来事の id(`verdict:…` / `pr-gate` / `memory:…` / `adhoc:…`)と、
   ユーザー発言の一覧(Claude のみ)が並ぶ。モデルは `--retro-add` で行を書く。
4. 全行がそろったら `--retro-close <session_id>` が、台帳から vault の
   `retro/<年>/<日付>-<owner>-<repo>-<PR 番号>.md` を**決定論的に**書き出し、読み戻して一致を確かめてから
   完了にする(エージェントは表を書かない。PR にはコメントしない)。

## ノート

- 場所: vault の `retro/<年>/<JST の日付>-<owner>-<repo>-<PR 番号>.md`。PR が分からない(手動の `/retro`)
  ときは、番号の代わりに `s<session の先頭 8 文字>`。stack のときの PR は、台帳の最後の `pr` 行
  (最後に作った = 最上段)。同じ日に同じ PR を別のセッションが閉じたときは、後のノートの名前に
  ` ~<hash>` を付け、先のノートを上書きしない。
- frontmatter: `repo`・`pr`・`session`・`date`。本文は、kind ごとの表(`what` / `evidence` /
  `disposition` / `mechanism`。user-correction → friction → gate-hit → insight → none → skipped の順。
  `skipped` の `quote` は表の下の行)。
- vault を宣言していない機体、または宣言が使えない機体では、台帳に残したまま完了にする(`closed` に
  `ledger-only: …` と書く)。使えない理由は stderr に出す。ノートを書けない・読み戻せないときだけ、
  exit 3 で未了のまま(次の Stop で再び求める。上限は同じ 3 回)。
- 古い呼び出し(`--retro-close <session_id> <comment-url>`)の URL は、警告を出して無視する。

## user-correction は inbox に置けない

user-correction は feedback(ユーザーが方針・結果を直した発言)で、共有 AGENTS.md は「不可視な
ローカルメモに閉じ込めず、既定で Issue にする」と定める。inbox は vault のノートなので、`disposition: inbox`
の user-correction は `--retro-add` が exit 64 で拒否する。先に `gh issue create` で Issue にして
`issue:#<N>` と書くか、プロジェクト固有・機微な内容なら `none:<理由>` と書く。

## 行の語彙

| 欄 | 値 |
|----|----|
| `kind` | `user-correction` / `insight` / `friction` / `gate-hit` / `codify` / `none` / `skipped`(`codify` は `adhoc:*` の痕跡を消し込む行。`mechanism` が必須) |
| `disposition` | `inbox` / `issue:#N` / `none:<理由>` |
| `mechanism` | `eliminate` / `unrepresentable` / `gate` / `tool` / `prose` / `existing:<名前>` / `none:<理由>`(user-correction・friction・gate-hit で必須。旧 `script` は `tool` と読む) |
| `signal` | 兆候の id(`vocabulary.json`)。`mechanism` が段のとき必須 |
| `level_reason` | 兆候の既定の段と違う段を選んだときの理由(必須) |
| `existing` / `searched` | `prose` 以外の段のとき必須。`採用:<名前>` / `拡張:<名前>` / `自前` と、探した範囲 |

`none` と `skipped` は `disposition: none:<理由>` が必須。`skipped` は `quote`(ユーザーの
発言の逐語)も必須。`existing:<名前>` は、その名前が `what` に書かれていなければ受け付けない。
段・兆候・既存手段の意味と事例集は [codify-signals](codify-signals.md)(語彙の正本は
`config/agents/codify-signals/vocabulary.json`、ADR-1276)。

## 状態ファイル

`${WRAPUP_RETRO_DIR:-${XDG_STATE_HOME:-~/.local/state}/claude/wrapup/retro}/<session_id>.*`

| 拡張子 | 内容 |
|--------|------|
| `jsonl` | 行(`--retro-add` が検証して追記) |
| `count` | retro の block 回数(上限 3) |
| `escalated` | 上限に達して警告を出した印 |
| `reopen` | 閉じた retro を開き直した回数(上限 2)。開き直すと `closed`・`escalated`・`count` を消す |
| `transcript` | Stop 時に渡された transcript のパス(手順書がユーザー発言を抽出する) |
| `closed` | 完了の印(`--retro-close` がノートを読み戻してから書く。中身はノートの vault 内のパス、または `ledger-only: …`) |

## その場のコードの痕跡(ADR-1276)

PR を作らないセッション(メールの裁定・調査・机の作業)でも、scratchpad に使い捨てのスクリプトを書いたり、
`python3 -c` や長い jq を打ったりする。それを「決定論化できないか」という目で振り返るために、機械で拾える
痕跡を数える。しきい値と、コードの拡張子・形の強弱は、語彙(`config/agents/codify-signals/vocabulary.json`)が正本。

| 痕跡 | 出来事の id | 数え方 |
|------|-------------|--------|
| このセッション専用の scratchpad にあるコード | `adhoc:scratch:<ファイル名>` | コードの拡張子か `#!` のファイルが 1 つ以上 |
| 形を持つコマンドの反復 | `adhoc:repeat:<ハッシュ先頭 8 文字>` | 同じハッシュで、`shape` が空でないものが 2 回以上 |
| その場のスクリプトの形 | `adhoc:shape:<形>` | 強い形(`heredoc-interp` など)が合わせて 3 回以上 |

- いずれかを満たしたら、PR が無くても retro を求める。満たさなければ出来事そのものを出さない(1 回の
  `python3 -c` だけでは求めない)。素の「同じコマンドの反復」は数えない(`cargo test` のやり直しなどで、
  過去 247 セッション中 106 で起きる)。
- 形は `cmd-hash-log` が各レコードに添える(`command_inspection::cmd_shape`)。本文も長さも残さない。
  commit・PR 本文のヒアドキュメントは形に数えない。
- scratchpad は `$CLAUDE_CODE_TMPDIR` と `/tmp/claude-<uid>` の直下を走査して、session_id の一致で選ぶ。
  全セッションで共有の `$TMPDIR` は見ない。
- `adhoc:*` は `kind: codify` の行でしか消し込めない。「決定論化しない」も `codify` 行に
  `mechanism: none:<理由>` で書く(検討を飛ばせない)。
- 閉じた後に、まだ行に引かれていない `adhoc:*` が出たら、その分だけ retro を開き直す(1 セッションあたり
  2 回まで)。「作業の終わり」を機械で推測する案は採らない(#854、ADR-478 D1)ので、痕跡は最初の Stop で拾い、
  その後に書いたコードは開き直しで拾う。

## 待ちターンでは出さない(#854)

作業の途中で、バックグラウンド処理(背景の Bash、非同期の Agent)の完了を待つターンは、
作業の終わりではない。振り返りの対象は「セッション全体」なので、途中で出すと後から出る PR や
指摘が含まれず、やり直しになる。1 つのセッションで複数の PR を積みながら実機の長い実行を待つと、
指示がターンごとに 5 回以上届いた。

Stop の入力にバックグラウンドの状態は無いので、transcript から導く
(`hook_io::transcript::pending_background_tasks`)。開始は `toolUseResult.backgroundTaskId` か、
`toolUseResult.agentId` で `status == "async_launched"`。完了は task-notification の
`<task-id>`(新しい記録形式は `attachment.type == "queued_command"`、古い形式は
`origin.kind == "task-notification"`)。未完了が 1 件以上あれば、retro も inbox の指示も出さず、
上の `count` も消費しない。

- 開始から 1 時間を超えたものは待ちとして数えない。終わらない監視(`tail -F` など)が、
  指示を際限なく黙らせないため。
- transcript が読めない・時刻が読めないときは、従来どおり判定する(読めない開始は新しいものとして
  数える)。誤判定は「従来どおり発火する」側に倒れ、block 側には倒れない(軸: 検出のみ)。
- `last_assistant_message` から「報告で終えるターンか」を推定する案は、非決定論なので採らない(ADR-478 D1)。

## 限界

- `gate-events.jsonl` は session_id を持たないので、突き合わせの対象にできない。
- ユーザーの指摘を見落とさないかは、発言一覧の提示とモデルの判断に頼る(gate は判定しない)。
- 上限(3 回)に達すると警告 1 回で通る。放置を防ぐだけで、強制し続けはしない。
- PR の無いセッションが自動で発火するのは、その場のコードの痕跡がしきい値を超えたときだけ。しきい値に届かない
  ときは、`/retro` で手動に踏む。
- Codex では、scratchpad の痕跡は拾えない(Codex には session 専用の scratchpad が無い)。`cmd-hash-log` の痕跡
  (形を持つコマンドの反復・強い形の回数)は効く。`$TMPDIR` やワークツリーに書いた一時スクリプトは拾えない。
- scratchpad の置き場所は Claude Code の実装で、macOS では `$CLAUDE_CODE_TMPDIR` の下の構成を確かめていない
  (未確認)。見つからなければ、scratchpad の痕跡は無いものとして扱う。
