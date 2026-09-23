# stack-base-guard — セッション内 PR を常時単一チェーンに積む PreToolUse hook

判定エンジン: `config/claude/hooks/stack-base-guard.sh`
決定: `docs/adr/0027-uncertainty-first-stacking.md`
規約側: `config/claude/skills/stacked-pr/SKILL.md`
完了時の対: `config/claude/hooks/pr-gate.sh` の `G_stack`(`docs/claude/pr-gate.md`)

セッション内で複数の PR を作成するとき、後続の PR は常に直前の PR の head
branch を `base` にすることを、`gh pr create` / `gh pr edit --base`(および
相当する MCP GitHub 呼び出し)の作成時に機械強制する。

## なぜ必要だったか

ADR-0027 の Context に記録した通り、ある private リポジトリでの開発
セッションで 1 セッション約 10 PR を作成した際、依存の有無を予測してから
積むかどうかを決める既存の判定条件(`stacked-pr` スキル §1 の (a)/(b))が
外れ、ブランチは物理的に直列に積まれているのに PR の `base` 宣言が
不整合になった。他の open PR のコミットを含んだまま `base` が default
branch のままの PR ができ(汚染 diff)、`gh stack link` も未実行のまま
Web UI で手動 stack を試みて orphan PR が発生した。

`docs/claude/stacked-pr.md` の旧「なぜ pr-gate.sh を触らなかったか」節は、
この種の取り違えを機械的に block する案(`G_stack`)を検討した上で
「実際に取り違えが起きてから block 化を検討する」と明示的に保留していた。
上記のインシデントはその保留条項の発火そのものであり、ADR-0027 で
判定条件による依存予測を「積むか否か」の判定としては廃止し、常時単一
チェーンを両端(作成時・完了時)で機械強制する決定を下した。この hook は
作成時側を担う。

## 2 層の判定

### 層(i): 状態レスの祖先一致検査

HEAD(`gh pr create` の場合)または編集対象 PR の head(`gh pr edit --base`
の場合)が、他の open PR のコミットを祖先として含むなら、`base` はその
PR の head branch でなければならない。

この検査は**セッション状態を一切持たない** — `gh pr list` で取得した
open PR の `headRefOid` それぞれについて、ローカルに実在すること
(`git cat-file -e`)を確かめた上で `git merge-base --is-ancestor` で
祖先関係を調べ、複数該当する場合は `git rev-list --count` で最も近い
祖先(コミット距離が最小)を選ぶ。状態を持たないため、手動操作や別
セッションからの `gh pr edit` にも効く — これが `#112` 型の汚染 diff
事故(先行 PR のコミットを含んだまま base が default branch のまま)を
直接に根絶する。

**`Independent-PR:` タグでも抜けられない。** 祖先を物理的に含んでいる
以上、`base` を別にすれば差分汚染は機械的必然になるため、この層は
「意図の正当性」を問わない。

### 層(ii): セッション ID 単位のチェーン状態

層(i)で祖先が見つからない場合(= このブランチは main 等から新規に
切られている)、セッション内でこれまでに作成した PR の head branch を
`${STACK_BASE_GUARD_DIR:-~/.claude/stack-base-guard}/state/<sid>.chain`
に 1 行 1 branch で記録し、2 本目以降の PR がこのチェーンの最後尾を
`base` にしていなければ deny する。

- 記録は**楽観的**(判定を通過した時点で追記する。PreToolUse は
  `gh pr create` 自体の成否を知れないため)。読み出し時に、記録済みの
  各 branch が現在も open PR として実在するか `gh pr list` の結果と
  照合し、実在しない行は無視する(create が実際には失敗していた場合の
  自己修復)。
- 離脱は本文 `Independent-PR: <理由>` のみ(`pr-gate.sh` の `No-Issue:` /
  `attribution-guard.sh` の `No-Attribution:` と同じ「理由必須の閉じた
  タグ」家系)。理由が空なら成立しない。
- 新しいセッションは記録が空なので新しいチェーンを開始できる
  (ADR-0027 の遡及適用はしない設計と一致)。

## コマンド解析エンジンを共有する

対象コマンドの検出(コマンド位置判定)・heredoc 本体の分離・クォート
解釈トークナイザは `config/claude/hooks/attribution-guard.sh` を
`source` して再利用する。`split_heredoc` / `tokenize` / `is_sep` /
`CMD_SEPS` は attribution-guard.sh 側の定義をそのまま使い、
`is_target_at`(`gh pr create`/`edit` の検出に差し替える必要がある)は
`source` の後に再定義することで上書きする — bash の関数解決は最後の
定義が勝つ。attribution-guard.sh 自身の `decide`/`main`/末尾ディスパッチ
は `[[ "${BASH_SOURCE[0]}" == "$0" ]]` で直接実行時のみに限定されている
ため、`source` しても暴発しない(#192 の Codex/Copilot adapter と同じ
安全策)。

この共有により、コマンド文字列全体を正規表現で見て docs やコミット
メッセージの記述で誤発火する、という attribution-guard.sh が実際に
踏んだ罠(`docs/claude/attribution-guard.md`「対象コマンドの検出は
『コマンド位置』に限る」節)を再度踏まずに済む。

## 対象コマンド

| コマンド | 発火条件 |
|---|---|
| `gh pr create` | 常に判定(base 省略時は `origin/HEAD` の symref、それも無ければ `gh repo view` で default branch を解決) |
| `gh pr edit [<target>] --base <v>` | `--base`/`-B`/`--base=` のいずれかが含まれるときのみ判定。含まなければ base に触れない edit なので対象外 |
| `mcp__github*` の `create_pull*`/`update_pull*` | tool 名を書き込み系の語で絞ってから `.tool_input.base`/`.tool_input.body` を見る(MCP GitHub は現在未接続、命名は attribution-guard.sh と同じく #161 の未確認事項を引き継ぐ) |

`-R`/`--repo` によるクロスリポジトリ指定にも対応する(`gh pr edit <N>
-R owner/repo --base <v>` の形 — 別リポジトリの stack を手元の worktree
から修復する操作は実際に発生する運用)。

`gh pr edit` の対象 PR は、明示的な PR 番号/branch 名(先頭の非フラグ
位置引数)から解決する。既知の限界として、`--title` のように値を取る
未知フラグの値がこの位置引数と誤認されることがあるが、誤認は「対象 PR
が解決できない」方向に倒れ判定不能(pass)になるだけなので安全側
(attribution-guard.sh 冒頭の「既知の限界」記録方針と同型)。

## deny の理由文

祖先一致検査(層(i))が deny するときは、正しい `--base` の値と検出した
親 PR の番号・branch を理由文に必ず含める(往復を 1 回で終わらせるため
— attribution-guard.sh の deny_reason と同じ設計)。

```
HEAD は open PR #7(feat/foo)のコミットを含んでいます。base を main に
すると先行 PR の差分がこの PR に混入します。`gh pr create --base
feat/foo` で作成してください。この積み方は依存の有無によらず常時
とります(ADR-0027)。
```

層(ii)が deny するときは、rebase による積み替えか `Independent-PR:`
タグのどちらかを案内する:

```
このセッションでは既に PR(feat/foo)を作成しています。2 本目以降は
直前の段に積むのが既定です(ADR-0027)。`git rebase --onto feat/foo ...`
で積み替えて `gh pr create --base feat/foo` とするか、真に独立な PR
なら本文に `Independent-PR: <理由>` を書いて明示的に抜けてください。
```

## 縮退

判定不能はすべて fail-open(通す) — ADR-0005 の binary-existence
gating と同じ縮退方針。

| 条件 | 挙動 |
|---|---|
| `jq`/`gh`/`git` のいずれかが不在 | 完全沈黙で通す |
| プロジェクトディレクトリが git リポジトリでない | 完全沈黙で通す |
| `gh pr list` が失敗(未認証・オフライン等) | 判定不能で通す |
| default branch が解決できない(`--base` 省略時) | 判定不能で通す |
| PR の `headRefOid` がローカルに実在しない | その候補を祖先検査から除外(他の候補で判定を続ける) |
| `gh pr edit` の対象 PR が解決できない | 判定不能で通す |
| 本文が読めない(コマンド置換等、層(ii)のみ) | 判定不能で通す |

抜け道: `SKIP_STACK_BASE_GUARD=1`、または
`${STACK_BASE_GUARD_DIR:-~/.claude/stack-base-guard}/skip` ファイルの
存在(`pr-gate.sh` の skip ファイルと同型)。

## 完了時の対 — `pr-gate.sh` の `G_stack`

この hook は作成時の base 宣言だけを見る。GitHub 上の stack オブジェクト
への実際のリンク(`gh stack link`)は完了時に `pr-gate.sh` の `G_stack`
judgement が要求する(`docs/claude/pr-gate.md` 参照)。両者は独立に
縮退する — `stack-base-guard.sh` は `gh` CLI の引数検査だけで完結する
ため、`gh-stack` 拡張の有無に関わらず全環境で base チェーンの正しさを
維持する。

## 検査

- `stack-base-guard.sh --selftest` がネットワーク無しに 15 ケースを
  検査する。gh スタブと、実コミットを持つ使い捨て git リポジトリ
  (main ← stage1 ← stage2、main から直接切った unrelated ブランチ)を
  組み合わせる — 祖先検査は `git merge-base --is-ancestor` に実オブジェクト
  を要求するため、pr-gate.sh の selftest と同じ「real_head」パターンを
  踏襲する。
- `stack-base-guard.sh --check '<コマンド文字列>' [<project-dir>]` で
  手動 e2e ができる。

## 登録形

```
PreToolUse / matcher: "Bash|mcp__.*" / timeout 20
```

`bleep`/`attribution-guard` と同じ複合 matcher 1 本(Bash 単体
だと MCP 接続の瞬間に無検査になる、同じ理由の繰り返し)。`gh pr list`
1 往復を含むため、attribution-guard(timeout 10)よりやや長めに確保する。
