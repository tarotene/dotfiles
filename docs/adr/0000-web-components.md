# ADR-0000 — 共有の部品は正本の役割だけを参照し、主な操作を先に書いて、PC では右端・スマートフォンでは上に置く

- Status: Accepted
- Date: 2026-11-24
- Issue: No-Issue(ADR-1685 が後続に残した「部品」の最初の版と、本人が絵を見て裁定したボタンの置き方の実装。この ADR は、その結果のうち公開してよい規則だけを残す)

## Context

ADR-1685 は、トークンの正本 `config/web/variables.css` と、各面の複写の固定を裁定した。部品(ボタン・選択肢・保存の表示・警告・空の状態・表の組み替え)は、まだ持っていない。実機の審査(2026-11-24)では、同じ種類の部品が面ごとに別の作りで、操作部が小さい、押下・エラーの状態で固定色の文字が淡い色の上に載る、保存の成否が表示に出ない、といった問題が、複数の面で見つかった。

ボタンの置き方は、2 つの公的なデザインシステムで食い違う。デジタル庁デザインシステム(DADS)は、主な操作を右端、戻る操作を左端に置く。英国政府のデザインシステムは、主な操作を入力欄の左端に揃える。2026-11-24 に、同じ画面で置き方だけを変えた 3 つの案(DADS 型、英国型、スマートフォンの下端に固定する型)を、スマートフォンの幅(390px)と PC で並べた絵で比べ、本人が DADS 型を選んだ。

## Decision

### D1: 部品は `config/web/components.css` に置き、正本の役割と尺度だけを参照する

色は `var(--…)` の役割で書き、16 進を直書きしない。`var()` の名前は、すべて `variables.css` に宣言があるものに限る。面ごとのテーマが役割を上書きすれば、部品は書き換えずに追従する。最初の版の部品は、`.btn`(`.btn-main`・`.btn-sub`・`.btn-link`)、`.actions`、`.choices` と `.choice`、`.status`(保存の表示。`data-state` が `saving`・`saved`・`failed`)、`.alert-ok`・`.alert-ng`・`.alert-warn`、`.empty`、`.skip-link`、`.table-cards`(狭い画面で表を 1 行 1 枚に組み替える。列を隠さない)、`:focus-visible`。

先行例: ADR-1685 D2 の 2 層(原色・役割)。Material Design 3 の「component の token は、system の token を指す」(https://m3.material.io/foundations/design-tokens/overview 、取得 2026-11-23)。
差分: 一致 — 部品は役割を指し、値を持たない。M3 の component token の層は作らず、部品の CSS が役割を直接指す(部品の数が少ない間は、層を足さない)。
軸: 表現不可能 — 単一正本 > 複写 + 同期。部品が値を持たないので、部品と正本の値がずれる状態が存在しない。

### D2: ボタンは、HTML では主な操作を先に書き、PC(幅 40rem 以上)では「戻る」を左端・「次へ」を右端に離し、スマートフォンでは縦に積んで「次へ」を上に置く

`.actions` が `flex-direction: row-reverse` と `justify-content: space-between` で実現する。主な操作が先頭に書かれるので、スマートフォンの縦積みで上になり、読み上げとキーボードの順序でも先に来る。画面の下端に固定する帯は、基準に入れない(長い画面で内容を隠し、ソフトキーボードと重なりやすい)。操作部の大きさは `--target`(2.75rem = 44px)を下限にする。

先行例: DADS のボタンの使い方(https://design.digital.go.jp/dads/components/button/usage/ 、取得 2026-11-23。主な操作は 1 画面に原則 1 つ、操作部は 44 CSS px 以上)。
差分: 異なる — 英国型(主な操作を左端に揃える)と、スマートフォンの下端固定の型を、絵で比べて採らなかった(本人の裁定。2026-11-24)。
軸: 表現不可能 — 宣言 > 手続き。置き方を CSS の 1 か所に宣言し、各面が自分で並べ替える余地を作らない。

### D3: 部品の不変条件は、`repo-structure-check` の `web_components` が `cargo test` で検査する

16 進の色の直書き(値の側だけを読み、セレクタの id は色と取り違えない)、正本に宣言の無い `var()`、必須の部品の欠け、`.btn` の `min-height: var(--target)` の欠け、`.actions` の置き方(`row-reverse` と `space-between`)の欠け、`@media (prefers-color-scheme)` の二重書きを落とす。例外は持たない。

先行例: 同じ crate の `web_variables`(ADR-1685 D3)と `web_assets`(ADR-1677 D7)。
差分: 一致 — 同じ crate に、同じ型の検査を足す。
軸: 検出のみ — この検査が届くのは、部品の CSS 1 ファイルだけである。各面が部品を使う HTML(部品の綴りを使うか、独自の作りに戻していないか)と、ページの型(学習・演習・裁定フォーム・ダッシュボード)の要件の検査は、未実装(意図あり)。

## Alternatives considered

D2 では、英国型と、スマートフォンの下端固定の型を、絵で比べた(上の Context)。D1 では、部品ごとに独自の変数を足す案(部品の中で原色を直接指す)を外した — 面のテーマが追従できなくなる(分析)。

## Consequences

- 各面は、`design-pin.toml` に `components.css` を足して複写し、`<link>` で読む(ADR-1685 D1。面の取り込みは、後続)。
- 部品の使い方(上のマークアップの例)は、`components.css` の冒頭に書いた。
- ページの型の要件を出力 HTML の検査にすること、Artifact をその場で組むときの手順書、部品への component token の層は、この ADR の範囲外(未実装、意図あり)。

## 執行点

- `config/web/components.css`
- `crates/repo-structure-check/src/web_components.rs`
- `crates/repo-structure-check/tests/tree.rs`

## Verification

- `cargo test -p repo-structure-check`: 単体テスト 101 件と、ツリー全体の検査 21 件が成功(2026-11-24)。`web_components` の単体テスト 8 件は、正本の部品が通ることと、負例(16 進の直書き・宣言の無い変数・必須の部品の欠け・`.btn` の操作部の大きさの欠け・置き方の欠け・二重書き)が落ちること、id セレクタが色と取り違えられないことを確かめる。ツリー全体の検査のうち、`the_shared_components_use_only_the_design_variables` が D3 を、`the_components_check_reads_what_it_is_about` が部品を読めずに通ってしまわないための番兵を担う。
- 手元の `cargo clippy --all-targets --deny warnings` が成功。
- 未実施: 部品を実際の面に当てた描画の確認(明暗・幅 390px)。部品の値は、正本の役割の組が WCAG の式で検査済みであることに依存する。
- 未実施: CI と同じ clippy(nix の derivation)での確認。
