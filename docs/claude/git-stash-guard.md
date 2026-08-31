# git-stash-guard — 素の `git stash` を弾く PreToolUse hook

stash スタックは git ではリポジトリ単位で、herdr が worktree ごとに切る**セッション
単位ではない**。実測で dotfiles だけで 9 個の herdr worktree が同時に存在していた
——それらは全て同じ stash スタックを共有する。素の `git stash pop` / `git stash apply`
（SHA 無し）は、直前に別セッションが積んだ WIP を自分のものと取り違えて適用する。
実装は `config/claude/hooks/git-stash-guard.sh`(配備先 `~/.claude/hooks/`)、
登録は `home/modules/claude.nix` の `registerHooks`。

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

複合コマンド(`;` `&` `|` `$(` バッククォート・リダイレクト・改行)は、
`git-worktree-allow.sh` とは**向きが逆**に倒す: allow 側は複合コマンドを対象外にして
フォールスルーする(素通し=無害)が、この hook では stash を含む複合コマンドを
フォールスルーすると事故そのものになるので、サブコマンドへの厳密な分解を諦めて
deny 側に倒す。

## 既知の限界

`if` は `Bash(git *)` という「コマンド文字列の先頭一致」なので、`cd wt && git stash pop`
のように git が先頭語でない複合コマンドは、そもそも hook を spawn させない
(`git-worktree-allow.sh` にも同じ形の限界がある)。脅威モデルは敵対的入力ではなく
Claude 自身が生成するコマンドなので許容している。

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
