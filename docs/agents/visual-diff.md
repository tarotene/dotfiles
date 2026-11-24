# visual-diff — 見た目の Before | After | Diff を、固定の撮り方で撮り、本人に見せる

実装: `config/agents/skills/pr-description/scripts/visual_diff.py`(`uv run --script`。PEP 723 で `playwright==1.63.0`・`pillow==11.3.0`)
手順の正本: `config/agents/skills/pr-description/SKILL.md` §5b
決定: ADR-1698(見た目の変更は、基準の ref から取り出した Before と After を固定の行列で撮り、Diff つきの並べたページで本人に見せる)
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

## audit(ページの型の要件。ADR-0000)

| サブコマンド | 入力 | 出力 |
|---|---|---|
| `audit` | 撮影定義(各面の `types`)、`config/web/page-types.toml` | 面ごとの違反(要件 ID・説明・出たセル)、通った検査、人が見る要件(未検査)。違反があれば終了コード 1。`--json` で JSON も書く |

- 検査は 10 個(閉じた語彙 `AUDITS`)。`document`(C1)・`contrast`(C2)・`targets`(C3)・`overflow`(C4。幅 390px のセルだけ)・`third-party`(C9)・`keyboard`(C12)・`main-button`(F1)・`default-choice`(F2。初期状態だけ)・`confirm`(F4。開いたダイアログと、スクリプトの静的な検出)・`links`(D7)。
- 測定(`AUDIT_JS`)は、撮影と同じ行列の各セルで、描画した状態のページの中で走る。コントラストは、文字を持つ要素ごとに、祖先をたどって重ねた背景色から WCAG 2.2 の式で出す。`keyboard` は、撮影定義の状態ごとに、先頭の 4 ボタンにフォーカスして Enter・Space を押し、押されるかを見る(明・幅 1280px で、状態を開き直して)。
- 測定から違反を作る部分(`cell_findings`)は、ブラウザ無しで試験できる純粋関数。要件の読み込み(`load_types`)も、種別と検査の名前が閉語彙で、結びつかない検査を落とす。
- 機械で測れない要件(`walkthrough`・`visual`)は、出力の「未検査」に ID と種別で並ぶ。通ったことにしない。
- 限界: ラベルに結びついたラジオ・チェックボックスは、ラベルの大きさで測る。`keyboard` は、状態の操作で現れるボタンの全部ではなく、先頭の 4 つ。`audit` は CI では走らない(ブラウザが要る。`--selftest` は走る)。

## 限界

- 差が意味を持つのは、同じ機体で撮った Before と After の間だけ。機体をまたぐと、書体の有無で全画素が変わりうる(未確認)。
- 画素の差は、見た目が良いかを判定しない。本人が見る。許容差は既定 8/255(同じ機体の 2 回の撮影は画素単位で一致する)。
- 撮影できる面は、撮影定義を持つ面だけ。最初の撮影定義は裁定ページ(`reference/verdict-page/visual/`)。
- 遮断した第三者への要求は、本番の表示とずれる(読み込めない書体は、機体の書体に落ちる)。ADR-1677 D6 が第三者の書体を禁じているので、守られた面では差が出ない。
