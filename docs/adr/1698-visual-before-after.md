# ADR-1698 — 見た目の変更は、基準の ref から取り出した Before と After を固定の行列で撮り、Diff つきの並べたページで本人に見せる

- Status: Accepted
- Date: 2026-11-24
- Issue: No-Issue(Web の面の見た目を基準に従わせる作業を、順に進める前に、見た目の確認の手順を先に固定する。本人の依頼)

## Context

`pr-description` スキルは、見た目に影響する変更に Before / After の証跡を求める。機械が見るのは `G_visual` の「画像か `No-Visual:` がある」だけで、Before と After が対で揃うかは見ない(同スキル §7)。撮り方は散文で、セッションごとに手で組む。2026-11-24 の実測で、手で撮ると次の 4 つが起きた。

- Playwright MCP の Firefox は、`emulateMedia` の明の指定が効かず(常に暗)、`setViewportSize` の幅が 0.75 倍になり、`file://` を拒否する(`docs/agents/personal-web-app.md` §7)。
- 初期状態だけを撮ると、押下・エラー・解答後の状態の問題を拾えない。固定色の文字が淡い色の上に載る問題は、初期状態の実測では見えなかった。
- 全ページを 1 枚に撮ると、変更箇所が小さく、本人が差を読めない(高さ 6000px のページで、変更は 500px の幅の 1 行だった)。
- 変更を入れたあとに Before を撮ろうとすると、変更前の画面はもう手元に無い。

本人は、「見た目の Diff が分かるように、Before / After を本人に見せる」手順を、先に固定することを求めた(2026-11-24)。見せ方は、Before | After | Diff を横に並べた非公開のページを出してリンクを知らせ、止まらずに進める形を選んだ。

## Decision

### D1: 撮るのは、基準の ref から取り出したファイルの Before と、作業ツリーの After。行列は 明・暗 × 幅 390px・1280px × 撮影定義の状態

Before は、基準の ref(stacked PR では直前の段のブランチ)から `git archive` で取り出したファイルで撮り、作業ツリーからは撮れない。撮影定義(`visual.toml`)と、`window.claude` などの疑似(stub)は、Before・After とも現在の作業ツリーのものを使う(疑似の違いを画面の差に混ぜない)。状態は撮影定義が宣言し、初期・データあり・押下・失敗・空を、面ごとに持つ。画面に差し込むデータは合成データだけで、実データを撮影定義に置かない。第三者への要求は遮断する(撮影を決定的にするため。ADR-1677 D6 と同じ線)。

先行例: `config/agents/skills/pr-description/SKILL.md` §3〜§5・§7(`--attach`、Before は変更前に撮る、`G_visual` は 3 択の OR で対を検査しない)(取得 2026-11-24)。
差分: 異なる — 既存は撮り方の散文と、画像の有無だけの機械検査。ここは撮影・差分・見せ方を 1 本のスクリプトに寄せ、Before を作業ツリーから撮れない形にして、変更後に Before を撮り損ねる状態を作れなくする。
軸: 表現不可能 — 単一正本 > 複写 + 同期。撮り方がセッションごとの手組みで複写される状態を 1 つのスクリプトに寄せ、Before の取り出し元を ref に限る。

### D2: 実装は、PEP 723 の Python(Playwright の Chromium + Pillow)で書く

本命: なし — 候補を、公式のバインディングの有無と、既存の実績から絞った。結論から出発したのではない。
対抗馬: `@playwright/test` の `toHaveScreenshot`(同じ評価軸「基準の ref から作った Before と After を、固定の行列で比べ、Before | After | Diff を出せるか」。比較は pixelmatch で、`--update-snapshots` で基準画像を作る。出力される差分画像の名前と置き場は、取得した文書に記述なし。npm の依存とテスト定義の新設が要る)。Rust + CDP の crate(同じ評価軸。公式のバインディングが無い。保守状況は未確認)。
外した候補: reg-cli(reg-suit) — 感触で外した(npm の依存を足したくない。README は読んでいない)。Percy・Chromatic などの有償サービス — 本人の画面を第三者に送るので外した(分析。ADR-1677 D6 の「第三者へ送らない」と同じ線)。ImageMagick の `compare` — 開発の機体に無かった(分析)。
既存手段: `config/agents/skills/pr-description/scripts/visual_diff.py` — 自前 — 却下: `toHaveScreenshot`(上記。基準の ref からの取り出しと、本人に見せる並べたページの出力が、そのままでは得られない。ただし、出力形式の記述が取得した文書に無いだけで、無いとは断定しない)。
先行例: PEP 723 と `playwright==1.63.0` のピンを使う手作業の Playwright スクリプト(別の個人の記録のリポジトリにある。同じ版に揃えた)。Playwright, Visual comparisons https://playwright.dev/docs/test-snapshots (取得 2026-11-24。「Playwright Test uses the pixelmatch library.」)。
差分: 一致 — 手で動かす重いスクリプトは PEP 723 で依存を宣言する。差が出るのは用途だけで、Playwright を画面の操作でなく撮影に使う。
実測: 2026-11-24、sandbox の中で Chromium が起動し、`--selftest --browser` が通った(`playwright==1.63.0` は機体の `chromium-1243` を使えた)。同じ入力で 2 回撮った画像は画素単位で一致し、明と暗が描き分けられ、幅は要求どおり(Firefox の 0.75 倍にならない)。
軸: 還元 — 既存の PEP 723 + Playwright の実績で足り、Rust の CDP crate や npm の依存の木を足さない。

### D3: 本人への見せ方は、非公開の Artifact に並べたページと、PR への添付。リンクを出したあとは止まらずに進む

セルごとに Before | After | Diff(赤が変わった画素)の 1 枚を出し、高い画像には変更箇所の切り出し(最大 3 か所。縦横とも変更の範囲に絞る)を足す。差が無いセルは「差なし」と表にまとめる(見た目を変えない整理の証跡になる)。並べたページは `visual_diff.py page` が作り、画像と一緒に `publish_files` で公開する。リンクを出したあとは、返事を待たずに PR まで進め、NG は PR 上で受ける。

先行例: `config/agents/skills/pr-description/SKILL.md` §3(`gh --attach`)・§5a(リポジトリ外の Artifact は、描画を確認してよいかを本人に 1 回聞く)(取得 2026-11-24)。
差分: 異なる — 既存は PR 添付が主で、本人に見せる経路が無い。本人が 2026-11-24 に「見せる」と裁定し、見せ方(並べたページ・止まらない)を選んだ。§5a の描画の確認の許可は、この裁定が与える。
軸: 検出のみ — 見せたかどうかは機械で強制できない(PR 本文に Artifact のリンクと差なしの一覧を書く規律だけ)。同じ漏れが再発したら、`G_visual` に対の検査を足す昇格を検討する(ADR-543)。

## Alternatives considered

D1 では、変更後に作業ツリーを退避して Before を撮る案(`git shelve`)を外した — 撮り忘れの余地が残り、ref からの取り出しなら、そもそも作業ツリーを動かさずに済む(分析)。D3 では、PR への添付だけにする案を外した — 本人が PR を開くまで見られず、リンクをチャットに出す案のほうが、本人の判断を待たずに進められる(本人の裁定)。

## Consequences

- 画面の変更を持つ PR は、撮影定義を面のデータの隣に置く。最初の撮影定義は、裁定ページ(`reference/verdict-page/visual/`)。
- 差が意味を持つのは、同じ機体で撮った Before と After の間だけである。書体の有無とレンダリングの違いで、機体をまたぐと全画素が変わりうる(未確認。同じ機体の 2 回の撮影が一致することだけを確かめた)。
- `G_visual` は変えない(画像・コードブロック・`No-Visual:` の 3 択)。ペア性は、`pr-description` §7 の意味的完成の基準と、本人に見せるリンクで担う。
- ページの型の要件を、描画した HTML への検査にすること(`audit`)は、この ADR の範囲外(未実装、意図あり)。

## 執行点

- `config/agents/skills/pr-description/scripts/visual_diff.py`
- `config/agents/skills/pr-description/SKILL.md`
- `config/agents/skills/inbox-adjudication/reference/verdict-page/visual/visual.toml`
- `config/agents/skills/inbox-adjudication/reference/verdict-page/visual/stub.js`
- `.github/workflows/ci.yml`

## Verification

- `uv run --script config/agents/skills/pr-description/scripts/visual_diff.py --selftest`: 画素の差・許容差・高さの違い・セルの分類・切り出し・並べたページ・撮影定義の誤り 6 種の拒否・ref からの取り出し(編集前の内容)が成功(2026-11-24)。
- `--selftest --browser`: 明暗の描き分け・幅・2 回の撮影の一致が成功(2026-11-24、sandbox の中)。
- 試運転: 裁定ページの保存表示の修正(PR #1689)を題材に、修正の直前の ref と作業ツリーを、3 状態 × 明暗 × 幅 390px・1280px の 12 セルで撮った。差があったのは保存失敗の 4 セルだけで、Before は書き込みが失敗しても「保存済み」を出し、After は「保存できませんでした」と「もう一度保存」を出した。保存成功と初期の 8 セルは差なしだった。並べたページを本人に出した。
- 未実施: CI での `--selftest` の実行(この PR の CI で確かめる)。他の面の撮影定義。
