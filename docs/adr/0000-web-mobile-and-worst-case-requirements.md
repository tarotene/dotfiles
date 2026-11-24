# ADR-0000 — ページの型の要件に、最悪のデータでの描画・ホバーの囲い・入力欄の文字の大きさ・拡大の禁止を足す

- Status: Accepted
- Date: 2026-11-24
- Issue: No-Issue(ADR-1701 の続き。Web の面を基準に従わせる作業のうち、隣の skill との突き合わせで見つかった、要件の穴を埋める)

## Context

ADR-1701 は、ページの型の要件を `config/web/page-types.toml` に 38 件起こし、描画した HTML で測れる 10 件を `visual_diff.py audit` で検査する形にした。

同じ配布元(Emil Kowalski, `emilkowalski/skills`)の 3 つの skill(`break-ui`・`mobile-native`・`learn-taste`)を全文読み(取得 2026-11-24)、38 件の要件と突き合わせた。すでに覆っているもの(操作部の大きさ C3、幅 390px の横スクロール C4、`prefers-reduced-motion` C7、ネイティブ要素 F9)を除いて、足りない観点が 3 つ見つかった。いずれも、実際の面で実在するかを、`grep` と CSS の走査で確かめた。

1. **最悪のデータでの描画**(`break-ui`): 今の要件に、データの極端さ(長い名前・長いメール・0 件/1 件/多数の件数・欠けた項目)を見る項目が無い。ADR-1701 の Context の実機の審査では、面ごとに、表の列が丸ごと消える・末尾が `nowrap` と省略で切れる・内訳が `title` 属性だけでタッチでは出ない、という指摘が出ていて、この型に当たる。
2. **ホバーの効かせ方**(`mobile-native` §1): `:hover` の見た目を `@media (hover: hover)` で囲まないと、スマートフォンで 1 度タップした要素のホバー表示が残り続ける。自分が保守する面の HTML・CSS・Svelte のファイルを走査した範囲で、囲いのあるものは 0 ファイル、囲いの無い `:hover` は 5 ファイルにあった(Python や Rust のコードが生成するページは、走査していない)。
3. **入力欄の文字の大きさ**(`mobile-native` §4): iOS Safari は、文字が 16px 未満の入力欄にフォーカスすると、ページを拡大して戻さない(skill の記述。iPhone の実機では確かめていない=未確認)。同じ範囲を走査して、16px 未満の入力欄の規則が 3 か所あった。あわせて、拡大を禁じる `<meta name="viewport">`(`user-scalable=no`・`maximum-scale`)は、skill が禁じる書き方で、同じ範囲では 0 件だった(予防の項目)。

## Decision

### D1: C15 を足す。実在しうる最悪のデータでも、重なり・欠け・横スクロールが出ず、省略した文字は全文を読む手段がある(種別 `visual`)

人が撮影した画面を見て判断する要件として足す。機械で測る検査(`audit`)にしない。最悪のデータを面ごとの撮影定義に載せる仕組み(データの境界で差し替える fixture)が、まだ無いので、測れないことを「通った」にしない(ADR-1701 D3)。`break-ui` の手順(切り替えつきの fixture を作り、壊れ方を報告する)は、この ADR では取り込まない(未実装、意図あり)。

先行例: Emil Kowalski, `skills/break-ui/SKILL.md` と `CATALOG.md`(最悪のデータは、実在しうる値か、スキーマの限界の値にする。省略は、全文を読む手段と対にする。省略の仕方は、切り詰め・折り返し・中間の省略・行数の制限・切り詰めないを、欄ごとに選ぶ) https://github.com/emilkowalski/skills/blob/main/skills/break-ui/SKILL.md (取得 2026-11-24)
差分: 異なる — 上流は、検査でなく、画面を壊して報告する手順。ここでは、要件の 1 行にして、撮影した画面を人が見る側に置く。
軸: 検出のみ — 最悪のデータの描画を、書けない形にはできない。fixture が無く、機械で測れないため。

### D2: C16・C17 を足す。描画した HTML を `audit` で測る(新しい検査 `hover`・`input-size`)

- C16: `:hover` を含む規則は、`@media (hover: hover)` の中に書く。測定は、描画したページの `document.styleSheets` を、入れ子の規則(`@media`・`@supports`・CSS の入れ子)まで読み、囲いの外にある `:hover` の規則を数える(読めないシートは数えない)。幅によらず測る。
- C17: `input`・`select`・`textarea`(チェックボックス・ラジオ・ボタン類・`hidden` を除く)の計算後の `font-size` が 16px 未満のものを数える。iOS のページの拡大は、小さい画面の挙動なので、幅 390px のセルだけで測る。

どちらも、`cell_findings`(測定から違反を作る、ブラウザ無しで試験できる純粋関数)に置き、検査の名前は閉語彙 `AUDITS` に足す。

先行例: Emil Kowalski, `skills/mobile-native/SKILL.md` §1(「ホバーは、`@media (hover: hover) and (pointer: fine)` で囲む」)と §4(「入力欄の文字は 16px 以上」) https://github.com/emilkowalski/skills/blob/main/skills/mobile-native/SKILL.md (取得 2026-11-24)
差分: 異なる — 上流は、コードを読んで直す手順。ここでは、描画した状態のページの測定として、検査の名前つきで測る。測定が見るのは `(hover: hover)` の有無だけで、`(pointer: fine)` の併記は要求しない(`(hover: hover) and (pointer: fine)` の書き方も通る)。上流が `(pointer: fine)` を足すのは、粗い入力なのにホバーできると名乗る一部の端末を除くためで、その端末の扱いは、この検査の範囲外(未確認)。
軸: 検出のみ — 検査は、囲いの無い `:hover`・小さい入力欄を作れなくはしない。入力欄の既定の大きさを共有の部品(`components.css`)に持たせて、書けない形にする案は、続く部品の段で検討する(未実装、意図あり)。

`hover` と `pointer` のメディア特性は、Baseline の widely である(web-platform-dx の公開 API `https://api.webstatus.dev/v1/features/interaction`、`high_date` 2021-06-11、取得 2026-11-24)。`hover` 単独・`pointer` 単独の feature_id での確認はしていない(`interaction` が両方を含む項目)。

### D3: C1 を拡張する。viewport は、拡大を禁じない(`user-scalable=no`・`user-scalable=0`・2 未満の `maximum-scale` を書かない)

検査は、既存の `document` に足す(新しい検査の名前は足さない)。閾値の 2 は、200% の拡大を妨げないことを基準にした、この検査の判断で、WCAG の条文は、この ADR を書いた時点で参照していない(未確認)。`mobile-native` は、`user-scalable=no`・`maximum-scale=1` を、アクセシビリティの失敗として禁じる(Hard Rule 4)。

先行例: Emil Kowalski, `skills/mobile-native/SKILL.md` Hard Rule 4(「拡大を禁じない。入力欄の文字の大きさを直す」) (取得 2026-11-24)。
差分: 一致 — 禁じる書き方は同じ。閾値(2 未満)だけ、この検査の判断で足した。
軸: 検出のみ。

## 採らないもの

- `learn-taste`(設計の訂正を、理由つきの規則として `DESIGN.md` に書く skill): 要件(`page-types.toml`)と ADR、本人のフィードバックの Issue 化という既存の置き場と重なり、新しい置き場を増やす。「繰り返した訂正は、文章でなく検査にする」という考え方は、すでに ADR-543 の昇格型が担う(分析)。
- `mobile-native` のうち、タップ時の灰色の光・タップの遅延・引っ張り更新・ノッチ・ステータスバーの色: 走査した範囲で該当が見つからないか、実機でしか確かめられない。入れない(分析。実機は未確認)。
- `interactive-widget` を含む viewport の既定値の推奨: Baseline の状態を確かめていない(未確認)ので、要件にしない。

## Consequences

- 実物への適用(2026-11-24): 見本ページは、新しい検査を含めて違反 0 件。裁定ページ(本リポジトリの `verdict-page/template.html`)は、C16 が 1 件(`button:hover`)、C17 が 13 件(`input#run-check` の 15px ほか)を、新しく拾った。裁定ページの違反(ADR-1701 の 8 件と合わせて、C12・C3・C4)を直すのは、各面を従わせる段(未実装、意図あり)。
- 他のリポジトリの面は、この検査の範囲の外(各面が、ページの型を宣言し、`audit` を走らせる段で見つかる)。
- 要件は 38 件から 41 件になる。ADR-1701 の本文の件数は、その時点の記録として変えない(ADR は追記のみ)。

## 執行点

- `config/web/page-types.toml`
- `config/agents/skills/pr-description/scripts/visual_diff.py`

## Verification

- `uv run --script config/agents/skills/pr-description/scripts/visual_diff.py --selftest`(ブラウザ無し): 実物の `page-types.toml`(4 つの型・41 の要件)の読み込み、C15 が `visual`・C16・C17 が `audit` であること、`cell_findings` の試験(基準を外れた測定が、`keyboard` 以外の 11 の検査すべてで違反を出す、`user-scalable=no`・`user-scalable=0`・2 未満の `maximum-scale` は違反、`maximum-scale=5`・`user-scalable=yes` は違反にしない、入力欄の文字の大きさは幅 390px のセルだけで見る)が成功する。
- `--selftest --browser`: わざと悪いページ(囲いの無い `button:hover`・12px の `textarea` を足した)が 12 の検査のすべてで落ち、良いページが 12 の検査のすべてを通る。
- 実物への適用: `visual_diff.py audit --spec config/web/visual.toml`(見本ページ)は違反 0 件(通った検査 8 件)。`audit --spec config/agents/skills/inbox-adjudication/reference/verdict-page/visual/visual.toml`(裁定ページ)は、C16 と C17 を含む違反 10 件。
