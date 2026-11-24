# ADR-1700 — デザインの正本は CSS の 1 つのまま、見て確かめる見本ページと、生成した機械可読の値と、1 枚だけの HTML が取り込む区間の pin を足す

- Status: Accepted
- Date: 2026-11-24
- Issue: No-Issue(Web の面を基準に従わせる作業の途中で、本人が「デザインシステムの形式化」を検討するよう求め、範囲を選んだ。2026-11-24)

## Context

Web の面の基準は、ADR-1677(基準・対象・書体)、ADR-1685(トークンの正本と、各面が複写を pin する配り方)、ADR-1687(部品)が裁定した。GOV.UK の 3 層(Styles・Components・Patterns)で見ると、Styles(トークン)と Components(部品の最初の版)は出来ている。足りないのは次の 3 つである。

1. **見て確かめる場所が無い。** 正本を変えると、どの面のどの部品がどう変わるかを、人が目で確かめる手段が無い(部品の見本ページは無かった)。
2. **機械が読める形が無い。** 正本は CSS だけで、`light-dark()` と `var()` を含むので、印刷物(Typst は `json(` を読める)や、値の一覧を読む道具は、そのままでは明暗を解いた色を読めない。
3. **1 枚だけの HTML が正本を取り込めない。** `design-pin` は、複写を別のファイルとして置く面を前提にする。claude.ai の Artifact の雛形のように、CSS を HTML の中に持つ面は、複写の先が無い。

## Decision

### D1: 部品と役割を 1 枚に並べた見本ページ `config/web/specimen.html` を置く。見本は正本を複写せず、実際に読み込んで描く

見本は、`variables.css`・`components.css`・`specimen.css`(配置だけ)を相対パスで読み込む。色の役割のすべてと、`components.css` のセレクタが使うクラスのすべてを並べる。値は持たない(色は `var(--役割)` で塗る)ので、正本を変えると見本の見た目が変わる。見本の撮影定義(`config/web/visual.toml`)を、明暗 × 幅 390px・1280px × 状態(初期・フォーカス・選択後)で持つので、正本の変更は、見本の Before | After | Diff として、人に見せられる(ADR-1698)。`web_specimen` が、読み込み・役割と部品の網羅・インラインの `style`/`script` と第三者の URL の不在・16 進の直書きと宣言の無い `var()` の不在を検査する。

先行例: GOV.UK Design System の、部品ごとの見本と使い方を載せる構成(https://design-system.service.gov.uk/components/ 、取得 2026-11-24。部品の見本ページがあることだけを読んだ)。ADR-1687 D1(部品は役割だけを参照する)。
差分: 異なる — GOV.UK は部品ごとの文書サイト。ここは、正本を読み込んで 1 枚に並べる静的な HTML で、検査が「載せ忘れ」を落とす。
軸: 表現不可能 — 単一正本 > 複写 + 同期。見本が値を持たないので、見本と正本の値がずれる状態が存在しない。部品を足して見本に載せ忘れる状態は、`web_specimen` が落とす。

### D2: 機械可読の値 `config/web/variables.json` は、CSS から生成する。手で書かない。ずれたら CI が落とす

原色(16 進)・役割(明暗を解いた 16 進)・尺度(書かれたままの値)を、決まった順で出す。生成は `cargo run -q -p repo-structure-check --example web_variables_json > config/web/variables.json`。`web_variables::check_tokens_json_tree` が、ツリーの内容と生成結果の一致を検査する。`@media` の中の再宣言(動きを 0ms にする規則)は、最初の宣言の値を載せる。解けない値は、欠けた欄でなく生成の失敗にする。

先行例: ADR-1685 の「外した候補」(DTCG の JSON を正本にして Style Dictionary で CSS を出す。当時は感触で外した)。W3C Design Tokens Community Group, Design Tokens Format Module 2025.10 https://www.designtokens.org/tr/2025.10/format/ (取得 2026-11-22。別の調査の記録から。「This specification is considered stable.」と、明暗などの文脈の差は別の Resolver Module と、書いてある)。Style Dictionary, DTCG https://styledictionary.com/info/dtcg/ (取得 2026-11-22。同じく別の調査の記録から。2025.10 には完全には対応せず、Resolver は未着手)。
差分: 一致 — CSS が正本。異なる — 感触で外した DTCG を、根拠に置き直した。DTCG を正本にすると、明暗(`light-dark()`)の解決が、安定した仕様の外(Resolver)と、未対応の道具に依存する。生成物は派生なので、形を DTCG に寄せる必要も、いまは無い(読み手は、印刷物と、claude.ai の Design System 型の入力で、配列形の一覧を読む)。
軸: 表現不可能 — 単一正本 > 複写 + 同期。`variables.json` は生成物で、CSS とずれたら落ちる。手で書いた値が正本と違う状態は、存在できない。

### D3: 1 枚だけの HTML は、正本を HTML の中の印を付けた区間に複写し、`design-pin` が区間の内容を固定する

`design-pin.toml` の `[[file]]` に、任意の `begin`・`end`(その語を含む行が、それぞれ 1 行だけ。両方か、どちらも書かない)を足す。区間は、`begin` の次の行から `end` の前の行まで。区間の内容の sha256 が、pin の `sha256`(正本のファイルの sha256)と一致しなければならない。区間の外は、固定に触れない。`begin` と `end` の欠け・重複・順序違い・片方だけの指定は、落とす。

先行例: ADR-1685 D1(pin 型。`commit`・`name`・`sha256`・`path`)。
差分: 一致 — pin の欄と、正本の取得の照合は、そのまま。異なる — 照合する対象が、ファイル全体でなく区間。
軸: 表現不可能 — pin 固定 > 浮動。区間が正本と違う状態(手で直した・別の版を貼った)を、CI が落とす。

## Alternatives considered

D1 では、Storybook などの部品の展示の道具を外した — 感触で外した(npm の依存の木を足したくない。実測していない)。D2 では、DTCG の JSON を正本にする案を外した(上の D2 の根拠)。claude.ai の Design System 型の Artifact は、見本と `variables.json` から派生物として作れる(その型の手順書は、配列形の `variables.json` を読み、`light-dark()` などの関数を含む値は読めないと書く。取得 2026-11-24)が、この ADR では作らない — 見本と `variables.json` が出来て、本人が見てから決める(未実装、意図あり)。

## Consequences

- 正本(`variables.css`・`components.css`)を変える PR は、`variables.json` を作り直し、見本の Before | After | Diff を見せる。
- 区間の pin は、`design-pin` の入力が増えるだけで、既存の pin(`begin`・`end` を持たない)は変わらない。各面の取り込み(今日のページなどの区間の追加)は、各面のリポジトリの後続の PR。
- ページの型の要件(描画した HTML への検査)は、この ADR の範囲外(未実装、意図あり)。
- 見本は、面ごとのテーマの上書き(accent の 4 つ)を持たない。上書きした面のコントラストは、これまでどおり各面が自分で計算する。

## 執行点

- `config/web/specimen.html`
- `config/web/specimen.css`
- `config/web/variables.json`
- `config/web/visual.toml`
- `crates/repo-structure-check/src/web_specimen.rs`
- `crates/repo-structure-check/src/web_variables.rs`
- `crates/repo-structure-check/examples/web_variables_json.rs`
- `crates/repo-structure-check/tests/tree.rs`
- `.github/actions/design-pin/check.sh`

## Verification

- `cargo test -p repo-structure-check`: 単体テスト 111 件とツリーの検査 25 件が成功(2026-11-24)。`web_specimen` の単体テストは、実物が通ることと、負例(部品・役割の載せ忘れ、インラインの `style`・`script`、第三者の URL、正本を読み込まない見本、16 進の直書き、宣言の無い変数)が落ちることを確かめる。`tokens_json` の単体テストは、全役割が明暗の 16 進で載ること、再宣言でなく最初の値を載せること、引用符の escape、決定的であること、解けない値が失敗になることを確かめる。ツリーの検査の 2 件は、読めずに通ってしまわないための番兵。
- `bash .github/actions/design-pin/check.sh --selftest`: 19 件が成功(区間の追加分 7 件を含む)。
- 見本を `visual_diff.py` で 12 セル(3 状態 × 明暗 × 幅 390px・1280px)撮り、暗・幅 1280px の初期を目で確かめた。選択肢が選択済みになっていた不整合(説明は「既定の選択を置かない」)を、撮って気づいて直した。
- 未実施: CI での実行(この PR の CI で確かめる)。`design-pin` の区間の指定を、実際の面の CI から `@main` で呼ぶこと(各面の後続の PR)。
