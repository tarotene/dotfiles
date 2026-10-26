# new-tool-guard — 新しい道具・単位の着手前に、既存手段で足りないかを問う

設計の正本は `docs/adr/543-existing-means-and-deterministic-promotion.md`(D1、D3、D7)。この文書は、
`crates/new-tool-guard` の 2 つの役割と、台帳(ledger)の扱いをまとめる。

## 判定役(PreToolUse、matcher `Write`)

新しい道具・単位(shebang で始まる新規ファイル、`bin`・`scripts`・`hooks`・`cmd` を構成要素に含む新規パス、
パッケージマニフェストの新設。テスト・fixture・scratchpad 配下と、ディスクに既にあるファイルは対象外)を、
そのパスの `既存手段:` 行が現在の worktree の台帳(git toplevel をキーにした
`~/.claude/new-tool-guard/state/<key>.ledger`)に無いまま `Write` しようとすると deny する。deny の文には
`register` コマンドの完全形が付く。`classify <path>` サブコマンドは、同じ述語を pr-gate の `G_prior` に公開する。

## 記録役(PostToolUse、matcher `ExitPlanMode`)

承認された計画の `## 先行例との対比` の節にある `既存手段:` 行(書式が正しいものだけ)を、現在の worktree の台帳へ
書く。**台帳は計画の派生で、正本は計画**。承認時に `plan-precedent-gate` が読む行と同じ行を、新しい `Cargo.toml` の
`Write` が deny されてから手で `register` し直す二重入力が起きていた(PR #872 の作業中)。

- 拾うのは、節の見出し `## 先行例との対比` から、次の `## ` の手前までにある `既存手段:` の行(行頭の `- ` と空白は落とす)。
  別の節の行と、書式が不正な行(`is_valid_kizon_line` を通らないもの。例: 理由の無い `自前`)は拾わない。
- 同じ行が台帳にあれば足さない。同じ計画を何度承認しても台帳は増えない。
- 計画が取れない、節が無い、git の外、台帳が書けない場合は、何も出力せず何もしない。承認そのものに影響させない
  (記録は best-effort)。
- 行の `<path>` には、**新しく作るファイルのパス**を書く(`既存手段: <新しいパス> — 採用|拡張|自前 — ...`)。
  `拡張:` の後が、既存のパス。台帳のキーは、そのパスを worktree の toplevel からの相対に正規化したもの。
- 手書きの `register` は、計画を書かない小さな作業の経路として残る。

1 バイナリが `hook_event_name` で記録役と判定役を分ける型は、`crates/gh-edit-allow`(PostToolUse の記録と
PreToolUse の判定)と同じ。

## 既存手段と代替

- 既存手段: 拡張 — `crates/gh-edit-allow` の 2 イベント型と、`hook_io::plan::plan_text`(計画の取得)、
  `SessionLedger`(台帳)。新しい crate は作らない。
- 対抗馬: 手順への 1 行の追記(`register` を着手前に実行する)。二重入力が残る。`plan-precedent-gate` の通過時に
  登録する案は、拒否された計画でも登録が残る。`new-tool-guard` の deny 時に最新の計画を参照して allow する案は、
  並列セッションの別の計画を指しうる。
- 軸: 還元 — 台帳を計画に還元して手入力を消す。Bash の heredoc 経路は検出のみのまま。

## 試験

`crates/new-tool-guard/tests/cli.rs` が実バイナリで、承認された計画からの転記(転記後に新規 `Cargo.toml` の
`Write` が通る)、不正な行・別の節・重複を拾わないこと、節が無い計画と `ExitPlanMode` 以外のツールの無視、
git の外での沈黙を確かめる。`lib.rs` の単体テストが節の抽出を確かめる。

## 登録

`home/modules/agents.nix`(PreToolUse の判定役と PostToolUse の記録役を、同じ command で宣言)、
`pipeline-map.toml`(`new-tool-guard` と `new-tool-guard-record`)。
