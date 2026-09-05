---
name: pr-description
description: PRの本文(Description)を標準スケルトンで書き、見た目に影響する変更にはBefore/After証跡(画像またはコード対比)を必ず添える習慣。PR作成・PR本文・Description作成・pull request description・PRを作って・スクリーンショット添付・Before/After・gh pr create・gh pr edit・--attach・視覚証跡、といった文脈で使う。attach before/after images to a PR, PR body skeleton, No-Visual escape hatch、といった英語の文脈でも使う。
---

PRの本文は、リポジトリごとにまちまちな自由記述ではなく、次の標準スケルトンで書く。リポジトリ固有の `PULL_REQUEST_TEMPLATE.md` がある場合は、その見出し構造に従いつつ、下記の内容要素を漏れなく埋める(見出しは譲るが、内容要素は譲らない)。

## 1. 本文スケルトン

```
Closes #N / No-Issue: <理由>

## 課題
解決したい問題を簡潔に(経緯の全列挙はしない)

## 解決策
採用案 + 棄却した代替案とのトレードオフ

## Before / After
画像(--attach)またはコードブロック対比。なければ No-Visual: <理由>

## 検証
LLM が実施・確認済みのこと(コマンドと結果)

## 要確認
人間にしかできない残作業のみ。なければセクションごと省略
```

1行目(`Closes #N` / `No-Issue:`)は既存の pr-gate(`G_link`)が機械的に強制する。`## Before / After` の証跡有無は `G_visual` が機械的に強制する(いずれもこのリポジトリの `config/claude/hooks/pr-gate.sh`)。それ以外(課題・解決策・検証・要確認の中身)はゲートの検査対象ではなく、この本文スケルトンが唯一の規律。

## 2. Visual の範囲

「見た目に影響する変更」は GUI/Web の画面変化に限らない。ターミナル/TUI の見た目(プロンプト、statusline、サイドバー、コマンドのレンダリング結果)も含む広義で判断する。テキストの差分だけで変化が十分伝わる場合(設定ファイルの数値変更など)は、画像の代わりに `## Before / After` 見出し配下へ fenced code block で対比を書いてよい。

## 3. `gh --attach` で画像を貼る(gh 2.99.0 以降)

`gh pr create|edit|comment`(および `gh issue create|edit|comment`)に repeatable な `--attach <path>` フラグが使える。`--attach './after.png#Alt text'` の `#` 以降が alt テキストになる。本文中に同じローカルパスの画像記法が既にあれば、その位置がアップロード後の URL に書き換わる。無ければ本文末尾に追記される。

制約: 画像・動画のみ(画像は10MB上限)、対象リポジトリへの write 権限が必要、GitHub.com / GitHub Enterprise Cloud のみ(GHES 非対応)。プライベートリポジトリの添付 URL は閲覧に認証が必要(public リポジトリでは無関係)。

```
gh pr create --title "..." --body "..." \
  --attach './before.png#Before' --attach './after.png#After'
```

## 4. Before の取り方

Before は変更を当てる**前**に撮る。変更後に「そういえば」と気づいても、変更前の状態はもう手元に無い。次のいずれかで確保する:

- 変更に着手する前に、対象コマンド/画面を先に撮っておく
- 変更前の状態が別 worktree(main のチェックアウト)に残っているなら、そちらで撮る
- 作業中の変更を一時的に退避してから撮る(このリポジトリでは `git shelve` / `git unshelve`)
- 変更前の SHA を一時 checkout して撮る

新規追加などBeforeが存在しない正当なケースでは、After単独でよい。その場合は本文に「Before: なし(<理由>)」と明記する(省略ではなく明記。ゲートは検査しないが、レビュアーが「撮り忘れ」と「元から無い」を区別できるようにする)。

## 5. 面ごとの撮り方

| 見た目の面 | 手段 |
|---|---|
| Web / HTML(ブラウザで見る画面) | Playwright でスクリーンショット |
| ターミナル出力の見た目(プロンプト、statusline、コマンド出力の色/レイアウト) | ANSI 出力を画像化するツール(`cmd \| freeze -l txt -o out.svg` のように、パイプで受けた出力を画像化できるものを選ぶ — 変更前に保存しておいた出力から後で画像化できるのが利点。出力形式は SVG か WebP を使う。charm-freeze はホストによっては PNG エンコーダがクラッシュすることがある) |
| 対話的 GUI(実機の画面操作が要るもの) | LLM では撮れないため、ユーザーにスクリーンショットを依頼し、`## 要確認` に計上する |

LLM で撮り切れる範囲は極力撮り切り、「要確認」は人間にしかできない残作業だけに絞る。

## 6. 完成の定義(二層)

- **意味的完成**(この本文の完成条件): ビジュアルが絡む変更では、Before と After の**両方**の証跡を載せる。コードブロック対比でも同様に両方を含める。Before が存在しない正当ケースのみ、After単独 + 「Before: なし(<理由>)」の明記で足りるとする。
- **機械的下限**(`G_visual` が実際に検査する条件、3択のOR): (a) アップロード済み画像(ローカルパスのままの画像記法は数えない)、(b) `## Before / After` 見出し配下の fenced code block、(c) `No-Visual: <理由>`。この3つのいずれかを満たせば `G_visual` は通る。

`G_visual` を通ることは必要条件であって、完成の定義そのものではない。ペア性(Before/Afterが揃っているか)はゲートが検査しないので、上の意味的完成の基準を自分で満たす。

事例は `cases.md` を参照。追記時のサニタイズ規則は `skill-gardening` を参照。
