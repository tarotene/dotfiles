# visual-diff — 見た目の Before | After | Diff を、固定の撮り方で撮り、本人に見せる

実装: `config/agents/skills/pr-description/scripts/visual_diff.py`(`uv run --script`。PEP 723 で `playwright==1.63.0`・`pillow==11.3.0`)
手順の正本: `config/agents/skills/pr-description/SKILL.md` §5b
決定: ADR-0000(見た目の変更は、基準の ref から取り出した Before と After を固定の行列で撮り、Diff つきの並べたページで本人に見せる)
走る場所: 手元は `visual_diff.py capture|diff|page`。CI は `--selftest`(ブラウザ無し)だけを `Escape-hatch dry-run` ジョブが走らせる。撮影を伴う `--selftest --browser` は手元。

## 動機

見た目の確認は、手で撮ると、変更の後に Before を撮り損ね、Firefox の Playwright の制約(明が効かない・幅が 0.75 倍)に当たり、初期状態だけを撮って
失敗・押下の状態の問題を拾えず、全ページを 1 枚に撮って差が小さすぎて読めなかった。`G_visual` は画像の有無しか見ないので、これらは検査をすり抜ける。

## 構成

| サブコマンド | 入力 | 出力 |
|---|---|---|
| `capture` | 撮影定義 `visual.toml`、`--ref`(基準の git の ref、または `WORKTREE`) | セル(面 × 状態 × 明暗 × 幅)ごとの PNG と `manifest.json`(遮断した第三者への要求・ページのエラー・横スクロール) |
| `diff` | Before と After の撮影ディレクトリ | 差があるセルの Before \| After \| Diff の 1 枚(Diff は赤が変わった画素)、高い画像の変更箇所の切り出し、`report.json` |
| `page` | `report.json` | 本人に見せる並べたページ `index.html`(差のあるセルを先頭、差なしは表) |

- 画面のファイルは `--ref` から `git archive` で取り出す。撮影定義と疑似(stub)は作業ツリーのものを使う。`prepare`(取り出したツリーの中で走る 1 行)で、`build.sh` などの生成を挟める。
- 配信は Playwright の `route` で、ソケットを使わない。第三者への要求は遮断して、`manifest.json` に残す。時刻は `page.clock` で固定する。
- 撮影定義のキーは閉じていて、知らないキーは誤り(`load_spec`)。`root` は、リポジトリの中の下位のディレクトリに限る。

## 限界

- 差が意味を持つのは、同じ機体で撮った Before と After の間だけ。機体をまたぐと、書体の有無で全画素が変わりうる(未確認)。
- 画素の差は、見た目が良いかを判定しない。本人が見る。許容差は既定 8/255(同じ機体の 2 回の撮影は画素単位で一致する)。
- 撮影できる面は、撮影定義を持つ面だけ。最初の撮影定義は裁定ページ(`reference/verdict-page/visual/`)。
- 遮断した第三者への要求は、本番の表示とずれる(読み込めない書体は、機体の書体に落ちる)。ADR-1677 D6 が第三者の書体を禁じているので、守られた面では差が出ない。
