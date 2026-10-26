# main-checkout-guard — 本物の checkout を変更させない PreToolUse + Stop hook

設計判断の記録: `docs/adr/705-main-checkout-guard.md`
実装: `crates/main-checkout-guard`(配備先 `~/.claude/hooks/main-checkout-guard`)
配線: `home/modules/agents.nix`(Claude の PreToolUse + Stop、Codex の同じ 2 つ)

herdr は作業単位ごとに worktree を作り、土台に親 checkout の HEAD を使う。親 checkout
(`git rev-parse` の `--git-dir` と `--git-common-dir` が一致する main worktree。
以下「本物の checkout」)が main でクリーンであることは、その前提になる。この hook は、
エージェントがどの本物の checkout にも変更を入れないようにする。

## 何を止めるか

PreToolUse(`main-checkout-guard pre`)が deny する:

| 対象 | 条件 |
|---|---|
| Write / Edit / MultiEdit / NotebookEdit | 対象パスの最も近い実在の祖先が本物の checkout の中 |
| Bash の git | `-C` / `--work-tree` / 先行する `cd` で畳み込んだ向き先が本物の checkout で、サブコマンドが変更系 |

変更系: `switch` `checkout` `commit` `reset` `merge` `rebase` `cherry-pick` `revert` `add` `rm`
`mv` `restore` `am` `apply` `clean`、`stash`(`list`/`show` 以外)、`pull`(`--ff-only` 以外)、
`branch`(作成・移動・削除・upstream 設定。`--list` などの参照は除く)。

通すもの: Read/Grep/Glob、読み取り系 git、`fetch`、`pull --ff-only`、`worktree list|prune`、
`herdr worktree create`。linked worktree(herdr、`.claude/worktrees/agent-*`)は対象外。

## baseline と事後検出

本物の checkout に**初めて触れた**とき(Read/Grep/Glob の対象、読み取り系 git、Bash の cwd と
絶対パス引数)、`~/.local/state/claude/main-checkout/<session_id>.baseline` に 1 行追記する:
`top<TAB>branch<TAB>sha<TAB>status_hash<TAB>pristine<TAB>may_write`。`status_hash` は
`git status --porcelain=v2` の FNV-1a、`pristine` は「クリーンかつ default branch」、
`may_write` は「書きうる操作で触れたか」(下の表)。6 列目の無い旧い書式の行は `may_write = 1`
として読む(後方互換)。

### 書きうる操作か(#898)

| 触れ方 | `may_write` |
|---|---|
| Read / Grep / Glob | 0 |
| 読み取り専用の許可リストにある Bash(下) | 0 |
| 許可リストに無い Bash(`sed -i`・`touch`・`rm`・`cargo`・`make`・`gh` など) | 1 |
| ファイルへ書き出すリダイレクト(`>`・`>>`・`&>`)を含む Bash | 1(全文) |

読み取り専用の許可リスト(`crates/main-checkout-guard/src/lib.rs` の `READ_ONLY_COMMANDS` と
`is_read_only_git`)。Bash のコマンド: `cat` `head` `tail` `less` `more` `bat` `nl` `tac` `grep`
`egrep` `fgrep` `rg` `ag` `ls` `eza` `tree` `stat` `file` `wc` `du` `df` `diff` `cmp` `readlink`
`realpath` `basename` `dirname` `pwd` `echo` `printf` `true` `false` `test` `[` `date` `hostname`
`which` `whoami` `jq` `sort`(`-o` なし)`uniq` `cut` `tr` `column` `find`(`-delete`・`-exec*`・
`-fprint*`・`-fls` なし)`fd`(`-x`・`-X` なし)。git: `log` `show` `diff` `status` `ls-files`
`ls-tree` `ls-remote` `rev-parse` `rev-list` `cat-file` `blame` `grep` `describe` `shortlog`
`show-ref` `show-branch` `for-each-ref` `name-rev` `merge-base` `diff-*` `fetch` `version`、
`reflog`(参照)、`stash list|show`、`worktree list`、`branch`(参照)、`remote`(`-v`・`show`・
`get-url`)、`config`(`--get*`・`--list`)、`tag --list`。`pull`(`--ff-only` を含む)・`gc`・`push`
は載せない(書きうる)。許可リストに漏れたコマンドは「書きうる」側に倒れるだけで、従来どおり
block になる(安全側)。

読み取りで記録した checkout に、後から書きうる操作で触れたときは、その時点の状態で baseline
を取り直す(`may_write = 1` の行を追記)。書きうる操作の前の移動は、このセッションの書き込みの
結果ではないので、基準に含めない。

Stop(`main-checkout-guard stop`)は記録した checkout だけを再確認し、`pristine` かつ
`may_write = 1` だったものが次のいずれかなら `{"decision":"block"}` を返す:

- ブランチが変わった
- 作業ツリーまたは index が変わった(`status_hash` の差)
- HEAD が動き、かつ旧 HEAD の子孫ではない(`pull --ff-only` は子孫なので通す)

`pristine` で `may_write = 0`(読み取りだけで触れた)checkout が同じ変化をしていても、block
しない。原因はこのセッションとは限らない(別セッションや人の rebase など)ので、ユーザーに
だけ見える注意書き(`{"systemMessage": ...}`)を返し、モデルへ報告・復元を促さない(#898)。
Codex での `systemMessage` の表示は未確認(Claude は仕様どおり表示する)。

報告した checkout は baseline を現在の状態へ追記して、次のターンで繰り返さない。
`stop_hook_active` が立っているときは黙る(台帳に書けないときの無限 block 対策)。
block の後は、エージェントが直そうとしても PreToolUse が deny するので、本人に変化を伝えて
`!` 付きのコマンドで戻してもらう。

触れた時点で既に dirty / default branch 以外だった checkout は `pristine = 0` で記録し、
block しない。PreToolUse が `additionalContext` で 1 回だけ注意する(Claude のみ。Codex は
PreToolUse の `additionalContext` を出さない)。

ただし、`origin/HEAD` が無く(既定ブランチが分からない)、main / master でもない所にいる
checkout は、使い捨ての clone とみなして注意しない(#845)。`git clone --branch gh-pages
--depth 5 <url> /tmp/x` のように、デプロイ用ブランチに何が push されたかを読むだけの clone
が典型で、「既定ブランチで clean でない」という文言が虚偽になり、`origin/<default>` が参照
できないので herdr の土台にもならない。`pristine` の記録は従来どおり行い、deny も変わらない。
`~/.ghr` 配下に限る案は、`herdr worktree create --cwd` が任意のパスを取る(ADR-705 の
Alternatives)ので採らない。既定ブランチ以外すべてで注意を消す案は、人が `switch -c` した
親から worktree を切る事故(この注意の本来の用途)で注意が消えるので採らない。

## 逃げ道

ない(env も ask もない)。本人が直接作業するときは `!` 付きのコマンドで操作する。

## 限界(検出のみ)

- Bash が絶対パスを引数に取らず、cwd も向き先でない書き込みは、触れたことを推定できない。
- Codex の編集ツール(`apply_patch`)は PreToolUse の対象外。Stop の事後検出だけが受ける。
- 判定不能な文(展開・未終端引用符)は粗い空白分割で見る。

## 環境変数

- `MAIN_CHECKOUT_GUARD_STATE_DIR` — baseline の置き場所(テスト用。既定
  `~/.local/state/claude/main-checkout`)。

## 検証

```
cargo test -p main-checkout-guard
```

手動(適用後): 別リポジトリの本物の checkout へ Edit すると deny、`git -C <path> log` は通り、
`sed -i` で変更してターンを終えると Stop で block される。
