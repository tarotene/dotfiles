---
name: stacked-pr
description: PR 同士に依存関係があるとき(先行 PR の成果物を後続が参照する、または同一ファイルの同じ節を逐次編集する)、main 起点で並行させず stacked PR として積む手順。依存する変更・stacked PR・PR を積む・base を親ブランチに・ADR を分割・同一ファイルを逐次編集、といった文脈で使う。stack changes, dependent pull requests, base branch chain, といった英語の文脈でも使う。PR 同士の依存関係は Issue 同士の依存関係とは別問題であることに注意 — 1 つの Issue が複数段の stack になることもあり、複数の独立 Issue が 1 つの線形 stack になることもある。
---

依存する変更を 1 つの巨大な PR に詰めず、main 起点で並行させて衝突させることもなく、
**stacked PR**(base を親ブランチにした PR の線形チェーン)として積む。

## 1. 判定条件

次のどちらかを満たせば stack する。

- (a) 後続の変更が先行 PR の成果物(ADR の決定、関数、設定キー、スキル本体など)を
  参照する
- (b) 同一ファイルの同じ節を逐次編集する

どちらも満たさない独立した変更は、stack せず別の PR にする(git-town:
"independent work should use separate top-level branches")。

## 2. 分割の設計原則

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
   (global CLAUDE.md の完了定義と同じ規律)。
2. 最下段を `main` から切って実装 → commit。
3. 2 段目以降は、直前の段のブランチから切って実装 → commit
   (`git switch -c <branch>`。`git worktree add` ではない — 同一 worktree
   内でブランチを切り替えるだけ)。
4. 各段ごとに `git push -u origin <branch>` →
   `gh pr create --base <直前の段のブランチ>`(最下段だけ `--base main`)。
   全段を作り終えるまで止まらない。
5. 全段の PR ができたら `gh stack link <PR番号1> <PR番号2> ... <PR番号N>`
   (最下段から順)で GitHub 上の stack にまとめる(§5 参照)。

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

## 8. なぜ pr-gate.sh を触らないか

`pr-gate.sh` は既に PR の `baseRefName` を見て動作し(`G_link` に stacked
advisory、`G_CI` に quiesce フォールバックを持つ)、stacked PR で完全に
沈黙するわけではない。「base が親のコミットを含むのに default branch」
のような取り違えを機械的に block する案もあるが、今回は指示文とスキルの
運用を先に確立し、実際に取り違えが起きてから block 化を検討する
(判断できる場合だけ踏み込む、という既存の pr-gate の設計原則に倣う)。

## 9. 追記

事例やアンチパターンは今後 `cases.md` に切り出す。追記時のサニタイズ規則は
`skill-gardening` を参照。
