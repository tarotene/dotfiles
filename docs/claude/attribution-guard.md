# attribution-guard — Claude 生成テキストに attribution を強制する PreToolUse hook

スクリプト: `config/claude/hooks/attribution-guard.sh`
規約側: `config/claude/CLAUDE.md`「GitHub に投稿するテキストには生成元を明示する」
Issue: #190

Claude が GitHub に書く外向きテキスト（PR / Issue の `create`・`edit`、Issue / PR
コメント、`gh pr review` のレビュー本体）に attribution フッターが載っていることを
保証する。

## なぜ必要だったか

PR 本文に付く `🤖 Generated with [Claude Code](...)` フッターは、**このリポジトリの
ルールではなく harness 側の attribution 指示**（セッションごとに注入される
system-reminder）由来である。そのため 2 つの穴があった。

1. **コメント投稿には一切付かない。** `gh pr comment` / `gh issue comment` /
   `gh pr review --body` で Claude が投稿したテキストは、GitHub 上では人間の発言と
   区別が付かない。mention を含むコメントは相手に直接通知が飛ぶため、人間の発言と
   誤読されるコストが最も高い面である。
2. **PR / Issue 本文側も保証されていない。** `pr-gate.sh` は `G_link`（closing
   keyword）と `G_visual`（視覚証跡）を検査するが attribution は見ていない。harness
   の指示が変わる・欠ける・セッションによって注入されないと、repo 側は何も気付かずに
   静かに落ちる。

規約（自発的に付ける）+ 機械 gate（漏れを拾う）の二層にした。`pr-gate.sh` の
`No-Issue:` / `No-Visual:` と同じ設計思想 — 沈黙を決定に変え、抜けた事実と理由を
grep 可能な形で残す。

## なぜ Stop hook ではなく PreToolUse か

コメント投稿は通知が飛ぶ**不可逆操作**で、事後に怒っても取り返せない。`pr-gate.sh`
が Stop で成立するのは、PR の本文は後から `gh pr edit` で直せるからである。投稿
そのものを止められる位置は PreToolUse しかない。

`ask`（人間に確認を出す）にもしなかった。フッターの追記は Claude が自分でできるので、
確認は人間の手数を増やすだけになる。`deny` なら往復 1 回で、人間の手数は 0。

## なぜ mention の有無で分岐しないか

依頼の発端は「メンション付きコメント」だったが、対象はコメント全般にした。

- mention が無くても watcher / assignee / subscriber には通知が飛ぶので、「人間に
  届くか」は mention の有無で決まらない。
- `@` の出現をパースする条件分岐は偽陰性を生む（コードブロック内の `@`、メール
  アドレス、`@` を付けずに名前を書くケース）。
- AI 生成物である事実は、読者が誰かに依存しない。

## 判定の境界

| 形 | 判定 |
|----|------|
| 本文に attribution フッターがある | 通す |
| 本文に `No-Attribution: <理由>` がある | 通す |
| 本文フラグが無い（`gh pr edit --add-label`） | 通す（判定不能） |
| `--body-file` のファイルが読めない | 通す（判定不能） |
| 本文がコマンド置換（`--body "$(cat f)"`） | 通す（判定不能） |
| 上記以外 | **deny** |

deny の理由文には**抜け道 `No-Attribution:` を明示的に書く**。publish-guard は
`verdict_reason()` に「bypass 手段はここに書かない。README 参照」と逆方針を明記して
いるが、あちらは漏洩防止で、抜け道を教えると自分で抜けてしまう。`No-Attribution:` は
正当な判断であり、Claude が使えないと意味がない。

`No-Attribution:` は理由を伴って初めて成立する（`pr-gate.sh` の `NO_ISSUE_RE` と
同型）。空の `No-Attribution:` を通すと、沈黙を決定に変える目的が崩れる。

## 検査範囲の切り出し — ここが一番の勘所

**コマンド文字列全体でマーカーを探してはいけない。** 次は Claude が自然に書く形だが、
全体スキャンでは comment 側が create 側のフッターによって通ってしまう。

```
gh pr create --title t --body "…🤖 Generated with [Claude Code](…)" \
  && gh pr comment 1 --body "短いコメント"
```

よって「**対象コマンドの出現位置から、次の対象コマンドの出現位置まで**」を 1 投稿ぶんの
範囲として切り出し、範囲ごとに独立に判定する（1 件でも deny なら deny）。

区切りをシェル metachar（`;` `&&` `||` `|`）にしなかった理由: 長文コメントの典型形
`--body "$(cat <<'EOF' … EOF)"` は本文中に metachar を含みうる。metachar で切ると
本文が後段セグメントに落ちてマーカーを見失い、**false deny が頻発する**。「次の対象
コマンドまで」で切れば heredoc 内の metachar は打ち切り要因にならない。

`git-stash-guard.sh` は metachar 分割を採っているが、あちらが分割するのは
`git stash pop` のような短い呼び出しで、本文のような長い値を持たない。
`git-worktree-allow.sh:49` は metachar を含むコマンドで判定自体を諦めているが、あれは
allow 側で「諦める = 許可を出さない = 安全側」。deny 側では諦め = 素通しになるため
同じ倒し方はできない（この非対称性は `git-stash-guard.sh:21-27` が明記している）。

## 対象コマンドの検出は「コマンド位置」に限る

範囲を切る前に、そもそも**対象コマンドが実行されるコマンドなのか、別コマンドに渡される
文字列なのか**を区別しなければならない。ここを外すと gate は運用に耐えない。

初版はコマンド文字列全体を正規表現で見ていた。その結果、**この hook 自身を commit
しようとして止まった** — コミットメッセージ本文に設計の説明として
`gh pr create --body "…" && gh pr comment 1 --body "y"` と書いたためである。続けて、
その誤検知を直すための修正スクリプトのコメント文でも止まった。

`git-stash-guard.sh:139-148` が同じ罠の記録を残している（「`stash` という単語がパスに
含まれるだけの無関係な複数行コマンドまで deny してしまう実害のある誤検知だった
（このファイル自身のパスで実際に踏んだ）」）。ただし違いがある: `stash` はコミット
メッセージに書く語ではないが、**投稿コマンドの綴りはこのリポジトリの docs と
コミットメッセージに頻出する**。だから粗い判定で済ませられない。

2 段で絞る。

1. **heredoc 本体を分離する**（`split_heredoc`）。本体の各行は改行の直後に来るので、
   分離しないと本体に書いた例文がコマンド位置に見える。分離した本体は捨てずに
   `HD_BODIES` に保持し、`--body "$(cat <<'TAG' … TAG)"` の本文候補として使う。
   `<<<`（herestring）は `<<` の次が `<` なのでタグの正規表現にマッチせず、自動的に
   除外される。
2. **残りをトークン化し、コマンド位置の `gh` 呼び出しだけを対象とする**。コマンド位置 =
   トークン列の先頭、または区切りトークン（`;` `|` `&` `(` `)` 改行）の直後。クォート
   された文字列は 1 トークンになるのでコマンド位置には来ない。

この結果、次はすべて素通しになる。

```
git commit -F - <<'MSG' … gh pr comment 1 --body "y" … MSG   # メッセージ本文の例文
grep -n 'gh pr comment 1 --body' docs/claude/attribution-guard.md
echo 'gh issue comment 1 --body x'
```

### wrap-up inbox の出自フッターは生成元表示を兼ねる

`wrapup-stop-gate.sh` の起票手順は出自フッター
`🤖 Filed from [Claude Code](https://claude.com/claude-code) wrap-up inbox` を
要求する。当初は「生成元表示」と「inbox 由来を grep で絞る出自フッター」を
別行で 2 本付けていたが、出自フッター自体に `Claude Code` へのリンクを含めれば
1 行で両方を満たせる。`ATTRIBUTION_RE` はそのため
`(Generated with|Filed from)[[:space:]]*\[?Claude Code` の alternation にして
おり、統合形・旧 2 行形式のどちらも受理する(旧形式で起票済みの Issue を
書き換える必要はない)。`Claude Code` を含まない出自フッター単独(例:
`🤖 Filed from wrap-up inbox`)は引き続き deny される。

### gate 自身を直すときは gate を外す

この guard のバグを直す作業そのものが guard に止められることがある（実際に起きた）。
その場合は `~/.claude/settings.json` から該当エントリを一時的に外して作業し、
修正後に `hms` で戻す。settings.json は activation 時の冪等マージで復元されるので、
手で戻す必要はない。

## 本文の抽出 — なぜ自前トークナイザなのか

範囲内から `--body` / `--body-file` の**値だけ**を取り出す。これをしないと
`--title "🤖 Generated with [Claude Code]" --body "x"` が通る。

**`xargs` は使えない（実測）。** GNU xargs はクォート内の改行を扱えず
`unmatched single quote` で失敗する。

```console
$ printf "%s" "gh pr comment 1 --body '## 見出し
- item'" | xargs -n1
gh
pr
comment
1
xargs: unmatched single quote; by default quotes are special to xargs unless you use the -0 option
```

改行を含む本文は長文コメントの典型なので、xargs では主要ケースが常にフォールバックに
落ちる。

**`read -r -a` の素朴な空白分割も使えない。** `git-stash-guard.sh:63-74` はこれを
採っている（「素朴な空白分割(git-worktree-allow と同じ割り切り)」と明記）が、あちらが
扱うのは `git stash push -u -m <tag>` のような値に空白が入らない短いフラグ列である。
この guard が取り出すのは Markdown 本文で、**`- 箇条書き` が必ず含まれる**。「次の
`-` 始まりトークンまで」で切る実装にすると本文が毎回途中で切れ、末尾のフッターを
常に見失って全 deny になる。

よってクォートを解釈する自前トークナイザ（`tokenize()`）を持つ。`LC_ALL=C` の
バイト単位走査で UTF-8 は安全 — 継続バイトは 0x80-0xBF で、ASCII のクォート・空白と
衝突しない。

## 抽出不能時の倒し方は経路ごとに違う

| 経路 | 倒し方 | 理由 |
|------|--------|------|
| heredoc（`<<`）を含む | 範囲文字列全体を検査 | 本体が `command` 文字列内に実在する |
| コマンド置換のみ（`$(` / `` ` ``） | 判定不能 → 通す | 中身が不明。deny に倒すと `--body "$(cat body.md)"` が常に弾かれる |
| 本文フラグが無い | 判定不能 → 通す | `gh pr edit --add-label` を誤検知しない |
| トークナイザが unmatched quote | 範囲文字列全体を検査 | heredoc と同じ |

「判定できない場合は断定に変えず素通す」は `pr-gate.sh:32-34` の縮退表と同じ思想。
ただし heredoc だけは素通しではなく範囲全体の検索に落とす — 本体が実在するので
判定材料がある。

## 既知の限界（意図的な選択）

- **`gh api` の生呼び出しは判定しない。** 正規経路（`gh pr|issue comment`、
  `gh pr review`）が揃っているので使う必然性がなく、`/issues/N/comments` や
  `/pulls/N/reviews` の URL パターン判定を入れると「インラインコメントは対象外」と
  いう決定と交錯して判定表が太る。規約（CLAUDE.md）側では経路を問わず要求している。
- **コード行へのインラインレビューコメントは対象外。** 1〜2 行が典型で、フッターが
  本文より長くなり S/N を壊す。投稿経路も `gh api` / MCP に発散する。
- **Codex CLI / Copilot CLI は対象外。** publish-guard のような adapter 層を持たない。
  別エージェントには別の文言が必要で、「どのエージェントにどの文言」の管理表が
  生まれる。
- **フォールバック経路では `--title` 等の他フラグにマーカーがあると通る。** 厳密経路
  （本文抽出が成功した場合）では塞がっている。脅威モデルは敵対的入力ではなく Claude
  自身が生成するコマンドなので許容している（`git-stash-guard.sh:31-33` と同じ前提）。
- **1 コマンド中に複数の heredoc がある場合、本体と投稿コマンドの対応付けはしない。**
  範囲内に heredoc リダイレクトがあれば、分離した本体すべてを本文候補にする。
- **MCP GitHub は未接続のまま実装している。** tool 名を書き込み系の語で絞り、
  `.tool_input.body` / `.comment` を見る。実際の命名規則は接続時に確認が必要
  （#161 が publish-guard について同じ課題を抱えている）。matcher を `Bash` 単体に
  しなかったのは、接続した瞬間に無検査になるのを防ぐため — publish-guard の旧実装が
  まさにこれで「最大の機能欠陥」を抱えていた（`home/modules/claude.nix` の登録
  コメントに記録がある）。

## 縮退と検査

- `jq` 不在・stdin 不正は黙って `exit 0`（ADR-0005 の binary-existence gating）。
- `attribution-guard.sh --selftest` が 31 ケースをネットワーク無しに検査する。
  うち 3 件は Copilot plan review の指摘 R1-B-1 の回帰ケース（`&&` 連結での
  取り違え / 手前の `echo` からの混入 / 閉じクォートを理由と誤認）、1 件は
  「Markdown 箇条書きで本文が切れて全 deny になる」false deny の回帰ケース、
  3 件は「コマンド位置にない投稿コマンドの綴りで発火する」false deny の回帰ケース、
  3 件は wrap-up inbox の統合フッター（前節）の受理・非受理・旧形式回帰。
  **false deny 系が落ちると gate は実用上使えない。**
- `attribution-guard.sh --check '<コマンド文字列>'` で手動 e2e ができる。

## 登録形

```
PreToolUse / matcher: "Bash|mcp__.*" / timeout 10
```

publish-guard と同じ複合 matcher 1 本。Bash と MCP を 2 つの hook エントリに分けない
— `register()` の存在判定は command 文字列の完全一致だけで matcher を見ないため、
同一 command を 2 つの matcher で登録しようとすると 2 回目が早期 return し、MCP 経路が
無検査のまま残る。`if` は付けない（`Bash(*)` のような permission rule 構文は MCP の
tool 名に一致しない）。絞り込みは hook 内部の早期 exit に置く。
