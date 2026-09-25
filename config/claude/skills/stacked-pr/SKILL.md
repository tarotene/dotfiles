---
name: stacked-pr
description: セッション内で複数の PR を作るとき、依存関係を予測せず常に作成順の単一チェーン(stacked PR)に積む手順(ADR-0027、uncertainty-first stacking)。離脱は閉じたタグ `Independent-PR: <理由>` のみ。複数 PR・stacked PR・PR を積む・base を親ブランチに・同一セッションで PR を複数作る、計画・グリル中に見つかったスコープ外項目を stack に積む、といった文脈で使う。stack changes, multiple PRs in one session, base branch chain, といった英語の文脈でも使う。PR 同士の依存関係は Issue 同士の依存関係とは別問題であることに注意 — 1 つの Issue が複数段の stack になることもあり、複数の独立 Issue が 1 つの線形 stack になることもある。
---

依存する変更を 1 つの巨大な PR に詰めず、main 起点で並行させて衝突させることもなく、
**stacked PR**(base を親ブランチにした PR の線形チェーン)として積む。

## 1. 常時単一チェーンに積む(ADR-0027)

同一セッション・同一 worktree で 2 本目以降の PR を作るときは、依存の
有無を予測せず**常に**直前の段の head branch を base にする。かつての
判定条件(後続が先行 PR の成果物を参照するか / 同一ファイルの同じ節を
逐次編集するか)は「積むかどうか」の分岐としては廃止した — 機能的な
依存関係とコード競合ベースの依存関係は別物で、後者は実際に PR を作る
まで予測できない。この予測に依存の有無を委ねる設計は、2026-09-21 の
実測(`docs/adr/0027-uncertainty-first-stacking.md` Context)でセッション内
の判定が系統的に外れ、base 宣言と実体(物理的な直列ブランチ)が不整合に
なる事故を招いた。

判定対象は依頼された変更同士に限らない。計画・グリル・実装中に見つかった
依頼スコープ外の項目も、今のセッションで手を付けるなら既定でチェーンに
積む(段の追加は `AskUserQuestion` で「stack に積む / wrap-up inbox に
送る」の 2 択を聞く — こちらは何を「今」やるかのスコープ判断であり、
チェーンに積むかどうかの判断ではない)。

離脱(このチェーンに積まない独立 PR にする)は、PR 本文に閉じたタグ
**`Independent-PR: <理由>`**(`No-Issue:`/`No-Visual:`/`No-Attribution:` と
同じ「理由必須の閉じたタグ」家系)を書いたときだけ成立する。依存の有無を
先読みして自発的にチェーンから外れることはしない。

作成時は `config/claude/hooks/stack-base-guard.sh` が base の取り違えを
機械的に deny する(`docs/claude/stack-base-guard.md`)。完了時は
`pr-gate.sh` の `G_stack` が `gh stack link` の実行忘れを block する
(`docs/claude/pr-gate.md`)。§8 参照。

## 2. 分割の設計原則

旧判定条件((a) 後続が先行段の成果物を参照する / (b) 同一ファイルの同じ節を
逐次編集する)は、「積むかどうか」の判断からは退いたが、**何を 1 段に
まとめるか**の目安としては引き続き使う — 段が細かすぎる/粗すぎるかを
判断する参考にする。

- 各段は単体でマージしても壊れないこと(Google eng-practices の
  "Don't Break the Build")。ADR とその実装を別段にする場合、実装段が
  マージされるまで ADR 段はドラフトのままにしない — ADR 段自体が
  「決定を記録する」という完結した変更である必要がある。
- 段は小さすぎて意図が読めないところまでは削らない(同上、
  "not so small that its implications are difficult to understand")。
- **同じ行を 2 段以上が書き換える切り方はしない(追記なら可)**。
  同一ファイルの同じ行を複数段が編集し、かつ squash-merge を使うと、
  実際には衝突していないのに衝突が報告される "phantom conflict" が起きる
  (git-town の警告)。`rerere.enabled = true` は残りを吸収するが、
  切り方そのもので避けられるものは避ける。
- 推奨段数の権威的な上限は存在しない(GitHub Docs・Google eng-practices とも
  確認できず)。3〜4 段を超えたら「分割の軸が間違っているサイン」として
  疑い、まとめ直せないか一度考える。

## 3. 積む場所と手順

同一セッション・同一 worktree の中で積む(worktree は増やさない)。

1. 分割案(何段に分け、各段が何を含むか)を計画段階で先に提示する。承認後は
   全段の PR 作成まで確認を挟まず進む — 「PR を作成しますか?」は聞かない
   (global CLAUDE.md の完了定義と同じ規律)。計画・グリル中に見つかった
   スコープ外の追加提案段は、依頼由来の段と区別できるよう分割案に
   「追加提案」と明示する(ユーザーが plan 修正でこの段を外せば
   wrap-up inbox 行きになる)。承認後の実装中に見つかった項目は
   `AskUserQuestion` で「stack に積む / wrap-up inbox に送る」の 2 択を
   聞く(「同一 PR に混ぜる」はレビュー負荷が事実上ゼロの微小修正に限る
   例外で、既定の選択肢には出さない)。
2. 最下段を `main` から切って実装 → commit。
3. 2 段目以降は、直前の段のブランチから切って実装 → commit
   (`git switch -c <branch>`。`git worktree add` ではない — 同一 worktree
   内でブランチを切り替えるだけ)。
4. 各段ごとに `git push -u origin <branch>` →
   `gh pr create --base <直前の段のブランチ>`(最下段だけ `--base main`)。
   全段を作り終えるまで止まらない。
5. 全段の PR ができたら `gh stack link <PR番号1> <PR番号2> ... <PR番号N>`
   (最下段から順)で GitHub 上の stack にまとめる(§5 参照)。**この実行は
   完了の一部** — `pr-gate.sh` の `G_stack` が未リンクのまま終わろうとする
   のを block する(ADR-0027、§8 参照)。

## 4. 下位段への修正が入ったときの追従

レビューで下位段に修正が入ったら、**その場で全上位段を rebase する**。
先延ばしにしない。

```bash
git switch <下位段のブランチ>
# 修正を追加コミットする(可能なら amend より追加コミットを優先し、
# force-push の範囲を絞る)
git switch <最上位段のブランチ>
git rebase <下位段のブランチ>   # rebase.updateRefs=true (home/modules/git.nix)
                                  # により中間段のブランチ ref も一括で追従する
git push --force-with-lease origin <段のブランチ>   # 影響を受けた各段で
```

`rebase.updateRefs` は machine-wide に有効化済み(`home/modules/git.nix`、
`docs/git-sync.md` 参照)。これが無いと最上位段だけ rebase され、中間段の
ブランチ ref が古いコミットを指したまま置いていかれる。

中間段(bottom でも tip でもない段)は `nix.yml` の重いジョブ
(`rust workspace` / `build vega` / `build arcturus` / `build altair`)が
既定で skip される(ADR-468)。修正した段を rebase で上位段に追従させれば
tip の合成木で改めて検査されるが、rebase せずにその段を単独でマージする
場合は `ci:full` ラベルを貼って heavy を手動で走らせる。

### 最下段の base(main)自体が進んで衝突したとき

最下段の作業中に `main` が他の変更で先に進み、最下段の PR が `main` と
衝突することがある(同一 worktree で長時間セッションを回していると
頻発する)。GitHub は `pull_request` イベントの CI を、マージ先との自動
マージが計算できない(`mergeable: CONFLICTING`)PR では走らせない
(WebFetch で `cache-nix-action` の入力名を確認したのと同じ調べ方の徹底を、
運用面のトラブルシュートにも適用する)。「CI がずっと pending/未実行の
まま」に見えたら、待つ前に `gh pr view <N> --json mergeable` で
`CONFLICTING` になっていないかを確認する — `pending` を漫然と待っても
CI は永遠に発火しない。

対処は §4 と同じ「その場で rebase、先延ばしにしない」だが、**最下段から
順に**行う(最下段が `main` と衝突しているなら、上位段を rebase しても
土台が古いまま揺れているだけで解決しない)。

この節の手順はどのステップから始める場合も、**まず `git fetch origin
main` を打ってから** `origin/main` を参照する。ローカルの
remote-tracking ref(`origin/main`)は明示的に fetch するまで更新
されない — 同一セッション中に main がリモートでさらに進んだ後、古い
`origin/main` に対して rebase すると、GitHub 側が計算する実際の
`mergeable` は解消されないまま(見かけ上は衝突が消えたように見えて)
push してしまう。

```bash
git fetch origin main
git switch <最下段のブランチ>
git rebase origin/main
# コンフリクト解消 → git add <file> → git rebase --continue
nix flake check --all-systems --no-build   # このリポジトリでの回帰確認
git push --force-with-lease origin <最下段のブランチ>
```

続けて 2 段目以降を rebase する際、単純に `git rebase <直下の段>` とは
**書かない**。最下段を rebase すると最下段の commit SHA が変わるため、
上位段の履歴にはまだ「古い SHA の最下段コミット」が残っている。ここで
素の `git rebase <直下の段>` を打つと、git は「新しい直下の段に含まれない
コミット」を古い最下段コミットもろとも再生しようとし、**既に解決した
はずの衝突がもう一度(無意味に)出る**。`--onto` で「古い最下段コミットは
飛ばして、直下の段より後のコミットだけを新しい直下の段の上に積み直す」と
明示する:

```bash
git switch <2段目のブランチ>
git rebase --onto <最下段のブランチ> <rebase 前の最下段コミットの SHA> <2段目のブランチ>
# 2段目自身の変更が新しい最下段と実質衝突する場合のみコンフリクトが出る
# (最下段の rebase 由来の衝突は再現しない)
nix flake check --all-systems --no-build
git push --force-with-lease origin <2段目のブランチ>
```

3 段目以降も同じ要領で、直下の段のブランチ名と「rebase 前の直下の段の
コミット SHA」(`git rebase --onto` 実行前に `git log --oneline -1
<直下の段>` などで控えておく)を差し替えて繰り返す。各段を rebase する
たびに `nix flake check --all-systems --no-build`(このリポジトリの回帰
確認コマンド、他リポジトリではそれぞれの高速検証コマンドに読み替える)を
挟み、全段 push し終えてから `gh pr checks <N> --watch` で CI を待つ
(`mergeable` が `MERGEABLE` に変わったことも合わせて確認する)。

## 5. GitHub ネイティブの stack 機能

2026-07-30 に GitHub 本体へ public preview で入った Stacked pull requests
機能を使う。**素の `gh pr create --base <親ブランチ>` が基本形**で、
全段の PR を作った後に `gh stack link` で GitHub 上の stack オブジェクトに
後付けでまとめる。

`gh stack init` / `add` / `submit` / `sync` の**ローカル追跡系コマンドは
使わない**。ローカルのブランチ構造を stack 拡張に握らせると、既知の
バグ(既存ブランチを trunk から再作成して force-push で履歴を破壊する等)を
踏むリスクがある。`link` はローカル追跡を作らず GitHub 上でリンクするだけ
なので、このリスクを避けられる。

preview の実際のステータス、既知の issue、拡張のバージョンといった
**時間で腐る事実**は `docs/stacked-pr-github-native.md`(調査記録)を参照。
この SKILL.md には裁定とその理由だけを書く(ADR-0008 のルール 1)。

## 6. Issue リンクの書き分け

各段の PR 本文の 1 行目(`Closes #N` / `No-Issue:`)は、**その段で完了する
Issue** の `Closes` を書く。その段では完了しない場合は
`No-Issue: #N の stack 第 k 段(#N は最終段で閉じる)` と書く。

stack 全体が 1 つの Issue に対応する場合も、複数の独立 Issue が 1 つの
stack に積まれる場合も、この書き分けは変わらない — PR の依存関係と
Issue の依存関係は別問題である。

## 7. 本文の `Stack:` 行

`pr-description` スキルの本文スケルトンの 1 行目の隣に、次の形式で
1 行を追加する:

```
Stack: <段番号>/<総段数> (base: #<親PR番号>)
```

最下段は `Stack: 1/3 (base: main)` のように親を `main` と書く。この行は
`pr-gate.sh` の検査対象ではない(人間とレビュアーのための注記)。

## 8. 機械強制(ADR-0027)

§1 の常時単一チェーンは指示文だけに頼らず、両端で機械強制する
(2026-09-21、`docs/adr/0027-uncertainty-first-stacking.md`)。

- **作成時**: `config/claude/hooks/stack-base-guard.sh`(PreToolUse)が
  `gh pr create` / `gh pr edit --base` を検査する。HEAD が他の open PR の
  コミットを祖先として含むのに base が違えば deny する(`Independent-PR:`
  タグでも抜けられない — 物理的必然のため)。セッション内 2 本目以降で
  チェーン外のブランチから PR を作ろうとした場合は、`Independent-PR:
  <理由>` が無ければ deny する。
- **完了時**: `pr-gate.sh` の `G_stack`(Stop)が、chain size 2 以上の
  stacked PR が GitHub 上の stack(`gh stack link`)にリンクされていなけ
  れば block する。`gh-stack` 拡張不在・API 取得不能は advisory に降格
  する。

設計根拠の詳細は `docs/claude/stack-base-guard.md` と
`docs/claude/pr-gate.md` を参照。

## 9. 追記

事例やアンチパターンは今後 `cases.md` に切り出す。追記時のサニタイズ規則は
`skill-gardening` を参照。
