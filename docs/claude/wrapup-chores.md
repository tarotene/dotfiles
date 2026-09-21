# wrapup-chores — wrap-up inbox を chores PR で一括消化するスキル

## 動機

[wrap-up inbox 機構](wrapup-inbox.md)は、スコープ外の気づきを 1 件ずつ Issue 化する。
これは「気づいた文脈が濃いうちに記録を残す」ことには向くが、実際に inbox に溜まる
項目の多くは判断を要さない軽微な修正で、1 件ずつ Issue → PR を回すのは摩擦が大きい。
Issue が積み上がるだけで着手されず、`git prune-branches` や `git-worktree-audit` の
指摘のように「起票はされたが誰も片さない」残骸になりやすい。

このスキルは、未起票の inbox 行と、起票済みだが未着手の wrapup 由来 Issue の両方を
まとめて棚卸しし、判断を要さないものだけを 1 つの chores PR で一括対処する。

## 器をスキルにした理由

「どれが判断無しで対処できるか」の triage はモデルの判断そのものであり、機械的な
条件だけでは決められない(skill-gardening の器の判断基準そのもの)。hook に落とせる
のは「JSONL の整合性を守る」ような決定論的操作だけで、それは既存の
`wrapup-stop-gate.sh` の `--add` / `--mark-filed` がすでに担っている。このスキルは
hook のその契約に完全に乗っかり、hook 自体には一切手を入れない。

## 内部構成の意図

### 母集団はフッター一致ではなく雑務性判定(2026-09-21 改訂)

当初はフッター
「🤖 Filed from [Claude Code](https://claude.com/claude-code) wrap-up inbox」
(統合形。旧 2 行形式の Issue も残っているため、フィルタは両方にマッチする
正規表現を使う)への本文一致を母集団の一次フィルタにしていた。しかし実測
(2026-09-21)では dotfiles の open Issue のうちフッター一致はごく一部で、
出自が wrap-up 由来であっても汎用の attribution フッター(`🤖 Generated with
[Claude Code](...)`)しか持たない Issue が大半を占め、機械判別できずに
「一撃で回収できない」不満の直接原因になっていた。

母集団は open Issue 全件に広げ、`blocked-by-upstream` ラベルだけをクエリ段階で
機械除外し、残りを LLM が §2 の雑務性基準で判定する構成に改めた
(ADR-0015 の deterministic node + LLM node 構成を踏襲)。フッター一致は
廃止せず、雑務性判定を経ずに無条件で候補に入れる**ヒント**として存続させる
(判定コストを下げつつ、フッターに依存しない全数性を確保する)。雑務性なしと
判定した Issue も理由付きで除外一覧に載せ、黙って母集団から落とさない
(scope-inventory の閉じた処分タグ原則を実行時の triage 画面に移植したもの)。

### なぜ `gh issue list` + jq で、`gh search issues` ではないのか

`gh search issues` は GitHub の Search API に依存し、インデックスの反映に
遅延があるうえ、絵文字を含む文字列のトークナイズ一致は不安定になりやすい。
`gh issue list --json body,labels` で本文とラベルを取得し、jq でラベル除外と
`test()` によるフッターヒント判定を行う方が、遅延なく決定論的に判定できる。
`--limit` は既定値(30)のままだと取りこぼすため 500 を明示し、**取得件数が
limit と一致したら打ち切りの可能性がある**ので limit を倍にして再実行する
規約を SKILL.md に明記した。

### `--mark-filed` を呼ぶタイミング: commit 後・PR 作成後・merge 後の比較

| タイミング | 問題点 |
|---|---|
| commit 直後 | push 前にセッションが落ちると、commit はローカルに残っても記録が誰にも見えないまま inbox からは消えている。気づきが実質的に失われる |
| **PR 作成直後(採用)** | 対処内容が PR 本文・commit として GitHub 上に恒久的に残るため、以降 PR がどうなっても気づきの記録は失われない。かつ inbox からは即座に消えるので、PR が merge されるまでの間に別セッションが同じ項目を再び起票する事故を防げる |
| merge 後 | PR 作成から merge までの間、対処済みの項目が inbox に残り続ける。この機構には「merge をトリガに inbox を消す主体」が存在しない(Stop hook はセッション単位で動き、他セッションが立てた PR の merge を監視していない)ため、その間に走った別セッションの Stop ゲートが同じ項目を重複起票してしまう |

したがって、記録の保全と重複起票の回避の両方が成立するのは PR 作成直後のみであり、
これを採用する。

### inbox 直接消化分を新規 Issue にしない判断

inbox 由来で対応する Issue が無い項目まで儀礼的に起票すると、起票してすぐ同じ PR で
閉じるだけの churn になる。このスキルでは、そうした項目は PR 本文の「課題」節に
title / detail をそのまま列挙することで監査可能性を担保し、新規 Issue は作らない。
このリポジトリの PR 本文規約は「`Closes #N` が無ければ `No-Issue: <理由>` を書け」
なので、対処項目が inbox 直接消化分だけの回では `No-Issue:` 側に倒す。Issue が
1 つでも混ざる回は、その Issue 分の `Closes #N` を書けば規約を満たす。

## 運用

- 発動キーワード: 「chores をまとめて」「wrapup を一掃」「inbox 消化」等(詳細は
  `SKILL.md` の `description`)。
- 自律度: triage 結果(対処対象の即対処/要判断の振り分けと理由、および除外一覧)を
  1 回だけ提示してユーザーの GO を取り、以降は項目ごとの確認を挟まず 1 つの
  chores PR にまとめる。GO 画面は open Issue 全件が対処対象・要判断・除外一覧の
  いずれかに 1 回ずつ現れていることを自己検査してから提示する(発見漏れゼロ)。
- 要判断に振り分けた項目は一切手を付けない。inbox 行は次回セッションの通常の Stop
  ゲートフローに委ね、Issue はそのまま open で残して最終報告に一覧を載せる。
- hook の `--add`/`--mark-filed`/`--check-dup` の契約・selftest・
  `~/.claude/settings.json` の登録は変更しない。inbox パスの解決は
  `--inbox-path` サブコマンドに委ね、書き込みは既存の `--mark-filed` 経由に限る。
