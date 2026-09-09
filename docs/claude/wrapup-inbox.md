# wrap-up inbox — スコープ外の気づきを自動で Issue 化する hook 機構

作業中に出た「今回のタスクのスコープ外だが起票価値のある気づき」が揮発する問題への
対処。**収集**(セッション中に inbox へ溜める)と**起票**(`gh issue create`)を分離し、
Claude Code の hook 2 本で回す。

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
| スキーマ | 最小 JSONL `{"ts", "title", "detail"}`・ラベルなし | 存在しないラベルは `gh issue create` を落とす。出自は本文フッター「🤖 Filed from Claude Code wrap-up inbox」で検索可能にする |
| inbox 置き場所 | `${XDG_STATE_HOME:-~/.local/state}/claude/wrapup/<slug>.jsonl` | user グローバル機構が任意のリポジトリ(会社リポジトリ含む)の作業ツリーに未追跡ファイルを生やすのは誤コミットリスク。slug はプロジェクトパスの `/` `.` → `-` 置換 |

## inbox の整合性(Codex レビューで確定)

inbox の書き込みを LLM の自由編集に任せると、重複起票・失敗行の誤削除・並行セッション
との競合を検証できない。そこでリスクのある操作をすべて `wrapup-stop-gate.sh` の
決定論的サブコマンドに寄せ、selftest(CI)で回帰テストする:

- `--add <inbox> <json1行>` — `mkdir -p` + JSON 検証 + flock 追記。初回利用時の
  ディレクトリ不在で沈黙する事故を防ぐ。
- `--check-dup "<title>"` — `gh issue list --search` で同名 open Issue を検査。
  exit 1 = 重複、exit 3 = 判定不能(その行は残す)。
- `--mark-filed <inbox> <json1行>` — **行全体の完全一致で先頭 1 行だけ**削除
  (`ts` は一意性を保証しないため削除キーにしない)。flock + tmp+mv。

flock は `--add` と `--mark-filed` の両方が取る。session_id フィルタを捨てた設計では
複数セッションが同じ inbox に触るため、追記と置換の競合(行の消失・復活)を排他で防ぐ。

Stop ゲートの stderr 指示は「起票成功または重複スキップした行だけ `--mark-filed`、
失敗行は残して次ターンで再試行、直接編集は禁止」と明示する。

## 検証

- `bash config/claude/hooks/wrapup-stop-gate.sh --selftest` — 縮退ゲート・
  `--add`/`--mark-filed`/`--check-dup`・SessionStart 注入の回帰テスト(CI の
  `ci.yml` でも実行)。
- 手動 E2E は inbox にダミー行を `--add` して新しいセッションを開始する:
  SessionStart 注入に未処理件数が出て、ターン終了時に Stop ゲートが発火する。

## 消化経路は 2 つ

上記の Stop ゲートによる個別起票に加えて、判断を要さない軽微な項目をまとめて片す
[`wrapup-chores` スキル](wrapup-chores.md)がある。どちらの経路でも inbox からの
削除は `--mark-filed` のみを介する(直接編集は禁止のまま)。個別起票は「気づいた
文脈が濃いうちに Issue へ記録する」ため、chores スキルは「判断無しで即対処できる
ものを 1 つの PR でまとめて消化する」ためのもので、役割は排他ではなく補完である。
