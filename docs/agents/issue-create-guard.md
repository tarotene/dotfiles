# issue-create-guard — `gh issue create` を、ボールのラベルと本文の先頭行を持たせて作らせる

実装: `crates/issue-create-guard`(Rust)。`hook-router` の規則として走る(`crates/hook-router/src/rules.rs`、`tools: Bash`)。
エージェントの種類は問わない(`Bash` のコマンド文字列だけを見る)。
決定の記録: `docs/adr/1379-issue-creation-ball-guard.md`
Issue: No-Issue(本人の Issue 台帳の不変条件を、事後の検査だけでなく作成時にも守らせる設計の依頼)

## 動機

本人は GitHub の Issue を台帳として使い、未完了の Issue に 2 つの不変条件を置く。

1. ボール保持者を、排他ラベル `ball:me` / `ball:them` / `ball:agent` のちょうど 1 つで持つ。
2. 次に動く契機 `next: YYYY-MM-DD` を持つ。置き場は、非公開リポジトリの Issue なら本文の先頭の行。
   公開リポジトリの Issue は、本文に日付を一切書かない(日付は別の非公開の台帳が持つ)。

これまでは事後の検査だけで、ボールを持たない Issue をそもそも作れた。作ってから直す往復と、直されないまま残る Issue が出る。
作成の呼び出し自体を実行前に止めて、不変条件に入らない Issue を作れなくする。

## 判定

コマンド文字列をトークン化し、コマンド位置の `gh issue create` を 1 つずつ見る(`pr-followup-guard` と同じ解析の土台)。

対象は、対象リポジトリの owner が `tarotene` のときだけ。対象リポジトリは `-R` / `--repo`(`HOST/OWNER/REPO` も可)、
無ければ作業ディレクトリの remote から読む。他の org や第三者のリポジトリ、owner を決められない呼び出しは対象外。

| 検査 | 内容 |
|---|---|
| ラベル | `--label` / `-l`(複数指定、カンマ区切り、`--label=…`、`-lfoo` の全部)に、`ball:me` / `ball:them` / `ball:agent` のちょうど 1 つ。0 個、2 つ以上の異なるボール、`ball:` で始まる語彙の外のラベルは deny |
| 本文(非公開) | `--body` / `--body-file`(heredoc を含む)の先頭の非空行が、`next: YYYY-MM-DD`(暦に実在する日付。後ろに空白以外が続かない)。そうでなければ deny |
| 本文(公開) | `next:` `due:` `postponed:` で始まる行(先頭の空白と大文字小文字は無視)が本文にあれば deny。日付を公開に載せない |
| 免除 | ラベルに `ledger-status`(機械が書く常設の状態 Issue)があれば、ラベル・本文の両方を免除する |

ラベルと本文の違反は 1 つの deny にまとめる(書き直しを 1 回にする)。理由文は書き直し方(`--label ball:me`)を含む。

リポジトリの公開/非公開は `gh repo view <owner/repo> --json visibility --jq .visibility`(`PUBLIC` は公開、`PRIVATE`・`INTERNAL` は非公開側)で引く。

### 縮退(fail-open)

判定できないものは通す。

- 公開/非公開を引けない(`gh` が使えない・認証が無い・応答が遅い): 本文の検査を飛ばし、**ラベルだけを検査する**。
- 本文の中身が分からない(本文フラグが無い、`--body-file -`、読めないファイル、`$(…)` などのシェルの展開): 本文の検査を飛ばす。
  明示した空の本文(`--body ''`)は空の本文として扱う。
- ラベルがシェルの展開(`--label "$X"`)で中身が分からない: 全体を判定しない。
- 時間の予算(10 秒)を越えたら、ルータが結果を捨てて素通りにする。

### 限界

- `gh api repos/…/issues -X POST`、Web UI、MCP の GitHub ツールからの作成は見ない(検出のみ)。事後の検査が拾う。
- 対象リポジトリは、作業ディレクトリの remote を、コマンドの途中の `cd` に追従させずに読む(`pr-title-guard` と同じ)。`-R` を付ければ確実。
- 本文のうち、`--body-file` のファイルは hook が走る時点で読める必要がある(同じコマンドの前段で書くファイルは読めない)。読めなければ本文の検査を飛ばす。

## 逃げ道

専用の環境変数は設けない。常設の状態 Issue は `--label ledger-status` で免除される。それ以外でボールを持たない Issue は、
作るべきでない(誰が次に動くかを決められないなら、まず決める)。

## 発火範囲

`tarotene` 配下のリポジトリ。全エージェント。

## 執行点

- `crates/issue-create-guard/src/lib.rs`
- `crates/hook-router/src/rules.rs`
- `pipeline-map.toml`
