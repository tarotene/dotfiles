---
version: 1
slug: "specimen-html"
primary_target: "specimen.html"
related_targets: []
---

# Surface brief: デザインの見本(config/web/specimen.html)

## Scope
出発点の世界(デザインシステム)の見本。全面に通す世界ではなく、採る面だけが複写する(ADR-1729)。モード: Operate が主、Read が従。利用者は本人 1 人(スマートフォンと PC)。公開の面・団体の面の読み手は本人以外。

## Direction contract

THESIS: 点検済みの機体のように、決めたことが札として残り、いまの 1 件だけが黄色の枠で囲まれる。避けるのは、役所の書式のような味気なさ・AI が作った感じ(グラデーション・ガラス・生成りの地と飾り書体)。

OWN-WORLD: 航空機の外板のマーキング。鋼色の灰色(#e9ecee)の地に白いシートの面、墨色(#1b1f23)、安全黄(#f2c200)、橙の注意の丸と「!」。短い見出しと符号は太く(800)、数字・時刻・符号は等幅の読み出し。点線の枠が「まだ」、実線の札が「済み」、黄色の枠が「いま」。暗の設定では、鋼色の暗い地に明るい鋼色の文字、黄色はそのまま。

STORY: 本人が、この出発点を採った面で、毎日の作業(裁定・演習・状況の確認)を迷わず・速く・間違えずに進める。

FIRST VIEWPORT: 1 件ずつ進む裁定ページの 1 画面(1440x900)。上に 1 行の状態行、その下に全件のステップの列(決めた件は検査札、いまは黄色の枠、残りは破線)、中央に 1 件(ラベル・見出し・引用・文脈)、下に固定の 3x2 の選択肢の格子と、戻る(左端)・次へ(右端)。主操作の位置はどの件でも動かない。

FORM: 機体のマーキング(割り当て: 2 回目の手の 7 番目、seed:08b4eed1874051ed。本人が決定ページで選択)。

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
