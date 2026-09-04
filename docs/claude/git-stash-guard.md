# git-stash-guard — 素の `git stash` を弾く PreToolUse hook

stash スタックは git ではリポジトリ単位で、herdr が worktree ごとに切る**セッション
単位ではない**。実測で dotfiles だけで 9 個の herdr worktree が同時に存在していた
——それらは全て同じ stash スタックを共有する。素の `git stash pop` / `git stash apply`
（SHA 無し）は、直前に別セッションが積んだ WIP を自分のものと取り違えて適用する。
実装は `config/claude/hooks/git-stash-guard.sh`(配備先 `~/.claude/hooks/`)、
登録は `home/modules/claude.nix` の `registerHooks`。

deny 側の代替として `git shelve` / `git unshelve`(`scripts/git-shelve` /
`scripts/git-unshelve`、配備先 `~/.local/bin/`)を用意している。worktree の絶対パス
をタグとして自分の entry だけを解決するラッパーで、詳細は下の「舗装路: `git
shelve` / `git unshelve`」を参照。

## なぜルールではなく hook か

`Bash(git stash)` のような permission rule は「stash という単語で始まるコマンドを
deny する」ことしかできない。`git stash push -u -m <tag>` のような**タグ付きで安全に
積む正規の手順**まで一律に塞いでしまい、実際に使い分けたい境界（SHA 指定の有無、
`-u`/`-m` の有無）を表現できない。hook なら生のコマンド文字列を検証してから
`permissionDecision: "deny"` を返せる。

## なぜ if を `Bash(git -C *)` ではなく `Bash(git *)` にしたか

`git-worktree-allow.sh`(このリポジトリのもう一つの git 系 PreToolUse hook)は
`if: "Bash(git -C *)"` で `git -C <worktree> ...` の形だけに絞っている。これは
**allow 側**の hook だから成立する: `if` が一致しない呼び出しでは hook プロセス自体が
spawn されず、その場合は単に「許可を出さない」だけなので安全側に倒れる
（通常の permission フローに委ねるだけで、何も壊れない）。

この hook は **deny 側**なので、その論理が反転する。`if` が一致しない呼び出しで hook が
spawn されなければ、それは「検査されず素通りする」——つまり**この hook が防ごうとしている
事故そのもの**になる。`git -C <worktree> stash pop` を捕まえるには `if` を
`Bash(git -C *)` にすれば足りるように見えるが、それでは `git stash pop`(`-C` 無し)の
形を落とす。両方を `if` で同時に絞ることもできない —— `registerHooks` の存在判定は
event + command 文字列の完全一致だけなので、**同一スクリプトを 2 つの `if` で
二重登録することもできない**(2 回目の `register` が 1 回目を見つけてスキップする)。

よって `if` は素の `Bash(git *)` まで広げ、絞り込みを hook 内部の早期 exit に移した:

1. jq を呼ぶ前に、stdin 全体に対して `grep -qw stash` する。大多数の git コマンドは
   ここで抜ける(bash 起動 + grep 1 回のコスト)。
2. 残ったものだけ `jq` で `tool_input.command` を取り出す。
3. 抜き出したコマンド文字列に対しても再度同じ `grep -qw stash` を掛け、その上で
   トークン判定に進む(`git commit -m "add stash guard"` のように、文字列に
   "stash" を含むだけの無関係な呼び出しを確実に落とすため)。

代償は git コマンド呼び出しごとに短命なプロセスが 1 個増えること。`if` を広げた分、
誤 deny の影響範囲も git 全体に広がるので、「非該当は無出力 exit 0」を最優先の
不変条件として selftest で固定している。

## deny / 通す の境界

以下を**すべて**満たすときだけ通す(pass through)。ひとつでも外れれば deny:

| サブコマンド | 通す条件 | deny する理由 |
|---|---|---|
| `list` / `show` | 常に通す | 参照のみで stash を変更しない |
| `push`(裸の `git stash` も同じ) | `-u`/`--include-untracked` **かつ** `-m`/`--message` の両方 | タグが無いと後から見分けが付かず、他 worktree の WIP と混ざる |
| `apply` / `drop` | 40 桁 hex の SHA を引数に持つ | SHA 指定なしは「一番上の stash」を触るため、他セッションが後から積んだものを掴む |
| `pop` | 常に deny | apply + drop の合成で、SHA 指定という安全弁が構文的に無い |
| `clear` | 常に deny | 他セッションの WIP を含めて全消去する |

`git -C <dir> stash ...` という前置形も同じ判定を通す。トークン分割は
`git-worktree-allow.sh` と同じ素朴な空白分割 —— `git stash ...` と
`git -C <dir> stash ...` の 2 形だけを厳密に見て、`-c` などのグローバルオプションを
挟む形は対象外(フォールスルーし、通常のプロンプトに委ねる)。

**注意: `git stash drop <SHA>` は git 自身が拒否する死文。** 上表の pass 境界は
コード上 `drop` を `apply` と同一に扱っているが、git 2.54 の実測では
`git stash drop <40桁SHA>` は `error: '<SHA>' is not a stash reference` で拒否
される——`drop`/`pop` が受け付ける参照形は `stash@{N}` のみで、SHA を受けるのは
`apply` だけ。ガードのコード自体はこの形を無害に通している(パス境界としては
安全)が、**案内としては使わない**: deny 理由文は SHA 無し `drop` に対して
`git unshelve`(apply + drop を安全にまとめて行う)だけを案内する。

複合コマンド(`;` `&` `|` `$(` バッククォート・リダイレクト・改行)は、
`git-worktree-allow.sh` とは**向きが逆**に倒す: allow 側は複合コマンドを対象外にして
フォールスルーする(素通し=無害)が、この hook では stash を含む複合コマンドを
フォールスルーすると事故そのものになるので、サブコマンドへの厳密な分解を諦めて
deny 側に倒す。ただし「"stash" という単語を含むか」だけでは誤爆する
(下の「既知の限界」参照)ので、"git" と "stash" が**空白で隣接**するパターン
(`git stash ...` / `git -C <dir> stash ...` に近い形)を要求する。

## 既知の限界

### `if` は best-effort — git 以外のコマンドにも発火する

`if: "Bash(git *)"` は Claude Code の公式仕様(hooks-guide.md の "Filter by tool
name and arguments with the `if` field")だが、**best-effort フィルタ**であり
「コマンド文字列の先頭一致」ではない。パイプ・リダイレクト・コマンド置換・変数
展開を含む Bash コマンドは、Claude Code がその引数パターンを完全に解析できないと
判断すると、パターンに関わらず hook を実行する(公式ドキュメント: "When Claude
Code can't determine which commands the Bash input runs, it runs your hook
regardless of the pattern.")。

実測でこれを踏んだ: `man git-stash 2>/dev/null | col -b`(git を一切実行しない
コマンド)がこの hook を spawn させ、しかも旧実装では「複合コマンドの中に
"stash" という単語があれば deny」という判定だったため、`git-stash` という
ハイフン区切りのコマンド名だけで deny されていた(`grep -w` はハイフンも単語
境界とみなすため、"git" と "stash" を両方単語として要求するだけでは直らない
——実測で確認済み)。現在は上の「"git" と "stash" が空白で隣接」判定でこの
誤爆を避けている。

`if` が「先頭一致ではなく best-effort」であることは実測で確認したが、
`cd wt && git stash pop` のように git が先頭語でない複合コマンドで実際に
hook が spawn されるかどうかは未検証(公式ドキュメントが明示するのは
`$()` ・バッククォート・変数展開を含む場合の run-anyway 挙動で、単純な `&&`
連結の扱いまでは確認していない)。spawn されなかった場合は
`git-worktree-allow.sh` と同じ形の限界が残る。脅威モデルは敵対的入力ではなく
Claude 自身が生成するコマンドなので許容している。

## 舗装路: `git shelve` / `git unshelve`

deny 案内が当初示していた公認フロー(list → SHA 確認 → apply → drop)は、
`git stash drop <SHA>` を git 自身が拒否するために**成立しない**(上の
「deny / 通す の境界」の注意参照)。ガードは正しく安全側に倒れているが、
安全な代替が事実上無かった。`scripts/git-shelve` / `scripts/git-unshelve`
(配備先 `~/.local/bin/`、`home/modules/packages.nix`)はこの穴を埋める:

- `git shelve [<メモ>]` — `git stash push -u -m "shelve:<worktree絶対パス>:
  <メモ>"` のラッパー。worktree の絶対パス(`git rev-parse --show-toplevel`)
  をタグに埋め込む。
- `git unshelve` — 現在の worktree のタグを持つ最新 entry を SHA で解決して
  `apply` し、`drop` する。**drop の安全性**が肝: `drop` は `stash@{N}` 形の
  参照しか受け付けないため、「list で index を確認 → drop」という事前検証
  方式では確認と削除が別コマンドである以上 TOCTOU が残る(他 worktree が
  その間に push/drop すると index がずれ、意図しない entry を消し得る ——
  plan review で指摘され、e2e で実際に再現・検証した)。そこで検証を
  **drop の後**に置く: `git stash drop stash@{N}` は実際に削除した entry の
  SHA を `Dropped stash@{N} (<sha>)` という形で出力する(git 2.54 実測)。
  これを期待 SHA と比較し、不一致なら `git stash store -m <元のmessage>
  <dropped-sha>` で**同一 SHA のまま復元**して index を再解決し、再試行する
  (上限 3 回)。`git stash store` は任意のコミットを stash entry として登録
  し直す操作で、SHA はコミットオブジェクトのハッシュそのものなので、
  drop 前と同一の entry が復元される(実測: 復元後も apply 可能)。
- 両コマンドは同じリポジトリの `claude-shelve.lock`(`git rev-parse
  --git-common-dir` 基準)を `flock` で保持し、舗装路同士の変更系操作を
  直列化する。手動の `git stash push` / `apply <SHA>` とはロックを共有しない
  が、そのレースは上記の事後検証+復元で吸収する設計。
- `git stash list | head -N` 等のように出力を早期に読み止める消費者へ
  パイプすると、git は SIGPIPE ではなく EPIPE を fatal error として
  exit 1 で終了する(実測)。`set -e` 下ではこれが無言のスクリプト異常
  終了を招くため、両スクリプトは git の出力を必ずコマンド置換で全量
  キャプチャしてから、その文字列に対して awk/grep をかける(生の git
  出力を head/`grep -q` に直接パイプしない)。

手動の安全ルート(`push -u -m` / `apply <SHA>`)も引き続き通す。ガードの
deny 理由文はこれらを案内するが、第一選択は常に `git shelve` /
`git unshelve`。

## 縮退と検査

- jq 不在・stdin 不正は黙って exit 0(ADR-0005 の binary-existence gating に倣う)。
- `git-stash-guard.sh --selftest` が deny/pass の全境界(push の -u/-m 有無、
  apply/drop の SHA 有無、pop/clear、複合コマンド、stash と無関係な呼び出し)を
  回帰テストする。CI の shellcheck 対象。

## 登録形

`registerHooks` の `register()` に第 5 引数 `if` を渡し、`PreToolUse` / matcher `Bash` /
`"if": "Bash(git *)"` で登録する。`if` はハンドラレベルの絞り込み(正式仕様)で、
非一致の Bash 呼び出しでは hook プロセス自体が spawn されない —— 上の「なぜ if を
広げたか」で説明した理由により、この hook では意図的に絞り込みを最小化している。
