# ADR-0000 — 部品と振る舞いは、ブラウザ標準 → 共有の部品 → 外部ライブラリの順に選び、対応表を閉じた語彙のデータにして検査する

- Status: Accepted
- Date: 2026-11-24
- Issue: No-Issue(ADR-1677・ADR-1685・ADR-1687・ADR-1701 の続き。Web の面を基準に従わせる作業のうち、部品と振る舞いの選び方の裁定)

## Context

ADR-1687 では、共有部品を CSS として配置し、見た目の役割のみを参照させた。課題として残っているのは、**何が必要なときに何を使うか**を決める場所がないことである。面ごとに独立して手作業で作り直す選択がなされている。ADR-1701 の Context が挙げたパターン(保存の成否が利用者に返らない、ショートカットキーがボタン標準のキー操作を奪う、読み込み中・失敗・空の区別がない)は、どれも同じ振る舞いを面ごとに手書きした結果である。

調べた事実(2026-11-24):

- **Pick UI Library**(Emil Kowalski, `emilkowalski/skills` の `skills/pick-ui-library/SKILL.md`。MIT、最終更新 2026-11-13)は、「作業の種類 → 使うライブラリ 1 つ」の対応表であり、手順は ①作業の種類を特定する(名指しされたライブラリではなく) ②導入済みのものを先に確認する ③1 つに絞って推薦する(選択肢の一覧は出さない) ④一覧になければ、対象外だと明言する。`disable-model-invocation: true` であり、自動発火はしない。一覧の大半は React 前提(base-ui・Sonner・recharts・zustand・next-themes など)。
- 保守の表示: 一覧の 12 件(base-ui・Sonner・recharts・motion・NumberFlow・shiki・zustand・dnd kit・Virtuoso・cmdk・Vaul・next-themes)のうち、README を全文読んだのは Vaul と base-ui のみである。Vaul(引き出し)は README に「This repo is unmaintained」と記載されている(最終 push は 2025-10-03)。ほかの 11 件は GitHub の `isArchived` が偽で、最終 push は 2025-10-29〜2026-11-24 である(README の保守表示は、base-ui 以外未確認)。
- 自分が保守する面のうち、React が実際に動作する面はない。操作のある面は素の HTML と JavaScript で、フレームワークを持つのは 1 面(Svelte)のみである(各リポジトリの `package.json` とテンプレートを読んだ範囲。合奏団の公開サイトはローカルになく未確認)。
- 一覧のうち、現在の面で用途があるのはタブとグラフのみで、どちらも手書き(SVG)。トースト・テーマ切り替え・確認ダイアログは、決定済みの要件(F3・C8・F4)と方向性が逆である。ダイアログ・メニュー・ドラッグ・長いリストは、どの面にもない。

## Decision

### D1: 部品と振る舞いは「ブラウザ標準 → 共有の部品(`config/web/`) → 外部ライブラリ」の順に選び、選び方を `config/web/selection.toml` に、閉じた語彙のデータとして置く

選び方(`way`)は 5 つ: `platform`(ブラウザ標準の要素)・`shared`(共有部品)・`external`(外部ライブラリ)・`handwritten`(手書きのまま。使う面が少ない)・`not-used`(使わない)。行ごとに保持できるキーが決まっている。

- `platform` の行は、`baseline = "widely"` を持つ。Baseline が widely でない機能(2026-11-24 時点の `popover` 属性は newly)は、標準の行に指定できない。
- `shared` の行は、`config/web/` の下にある実在するファイルを持つ。
- `external` の行は、バージョン(`pin`)と、同梱するファイルの `sha256` を持つ。
- `not-used` の行は、理由が `requirement` であれば、`page-types.toml` に実在する要件の ID を持つ。`no-surface`(該当する面がない)の場合は持たない。
- `handwritten` の行は、`reason = "few-surfaces"` を持つ。利用する面が増えた段階で、共有化するかを裁定し直す(ADR-1660 D6 の「2 つ目の app が出たら裁定する」と同型)。

初期の行は、`<dialog>`・`<details>`・ネイティブのラジオ(`platform`。いずれも widely)、ボタン・選択肢・保存表示・警告・空の状態・狭い画面の表(`shared`。`components.css`)、タブ・グラフ(`handwritten`)、トースト・テーマ切替・確認ダイアログ(`not-used`。要件に反する)、メニュー・引き出し・コマンドメニューなど(`not-used`。該当する面がない)。表にない作業が生じた場合は、行を追加して裁定を残す。

先行例: Emil Kowalski, `pick-ui-library/SKILL.md`(手順 ②「入っているものを先に確かめる」・③「1 つに絞る」・④「一覧の外は、外だと明言する」) https://github.com/emilkowalski/skills/blob/main/skills/pick-ui-library/SKILL.md (取得 2026-11-24)
差分: 異なる — 上流の一覧は React 前提で、ブラウザ標準を先頭に置かない。自分の面は React が 0 で、ネイティブの要素で APG の要件の大半を満たせるので、標準を先頭にする。上流は散文の手順と一覧で、守ったかを機械で検査しない。ここは、語彙を閉じたデータにし、行の整合を `cargo test` が検査する。
軸: 表現不可能 — 閉語彙 > 自由記述 + 事後 lint。選び方の語彙・行ごとのキー・参照先(要件の ID、共有のファイル)の実在を、書けない形にした。

### D2: 振る舞いの共有部品を、`config/web/` に素の JavaScript として置く(未実装、意図あり。次の PR)

保存表示の状態切り替え(保存中 → 保存済み。書き込み成功を確認してから「保存済み」にし、失敗時は項目の横に再保存操作を配置)、ショートカットキーが操作部の標準キー操作を奪わない仕組み、および読み込み中・失敗・空状態の出し分けを、1 か所の実装に集約する。配布方法は ADR-1685 の pin 型(複数ファイルの面は `[[file]]`、1 枚のみの HTML は ADR-1700 の区間 pin)を踏襲し、機構自体は変更しない。

本 ADR には D2 の決定のみを記述する。実装と執行点は、続く PR が `## Amendment` として追加する（執行点には現存する実体のみを記述できるため）。

先行例: 本リポジトリ `docs/adr/1687-web-components.md`（共有部品を CSS として配置し、役割のみを参照させる）。
差分: 異なる — 1687 の部品は CSS のみで振る舞いを持たない。JavaScript を伴う共有部品は初となるため、外部 URL や実行時 style の注入（`style-src 'self'` と衝突する）を禁止する検査を、実装の PR に含める。
軸: 表現不可能 — 単一正本 > 複写 + 同期。保存状態（`saving|saved|failed`）を 1 か所に集約し、面ごとの複写によって状態が食い違う（成功を先に出す形態）事態を、pin の `sha256` で作成不能にする。

### D3: Pick UI Library の skill そのものと、一覧の外部ライブラリは、今は入れない。規則(順序の考え方)だけを D1 の表に取り込み、React の面ができた時に再訪する

再訪の条件: 公開ページで実際に動く React（または別のコンポーネントフレームワーク）の面ができた時点。その際は `external` の行を追加し、バージョンおよび同梱ファイルの `sha256` を pin して、`style-src 'self'` の下で動くかをライブラリごとに実測する（今回は組み込む面が無く、未実測）。

先行例: 同上（`pick-ui-library/SKILL.md` および同リポジトリの README https://github.com/emilkowalski/skills、取得 2026-11-24）。
差分: 異なる — 一覧のうち用途があるのはタブとグラフのみであり、手書き SVG や標準要素で十分対応できる。トースト、テーマ切り替え、確認ダイアログは決定済みの要件と向きが逆である。skill の役割（事前に検証して 1 つに決める）は、D1 の表と検査が担う。
軸: 還元 — 一覧の価値は手順であり、その手順は自前の表が担う。skill の本体は、React の面が無い間は機能しない。

### D4: ADR-1677 D6 の「第三者からの書体・スタイル」に、スクリプトを含めると明確にする

ADR-1677 D6 の文言では書体とスタイルのみを禁止している。一方、ページの型の要件 C9（ADR-1701）および検査 `visual_diff.py audit` の `third-party`（サードパーティの書体・スタイル・スクリプト・画像を遮断してカウントする）は、最初からスクリプトも含んでいる。文言と運用に乖離があったため、運用に合わせて文言を修正する。`web_assets` が `<script src>` の外部ホスト、`modulepreload`、スクリプトの `import("…")`、`from "…"` の外部参照先を拒否し、外部スクリプトが必要な場合は同梱して `sha256` を pin する。

ADR-1677 の本文は変更しない（ADR は追記のみ）。本 D4 によって D6 の解釈を定める。

先行例: 本リポジトリ `config/web/page-types.toml` の C9 と `docs/adr/1677-web-design-baseline.md` の D6。
差分: 一致 — C9 ですでにスクリプトまで含めて運用している。新たな制約の追加ではなく、文言を実際の運用に合わせる。ただし、claude.ai の Artifact が CDN からスクリプトを読み込む例（図の描画ライブラリなど）は、本検査の対象範囲（本リポジトリの HTML・CSS）外であり、各面の例外として対象の面側で理由とともに記録する。
軸: 表現不可能 — 文言と運用の乖離を解消し、同一要件（C9）を検証する 2 つの検査（`web_assets` および `audit`）が同じ範囲を対象とするよう揃える。

## Alternatives considered

- D1: 対応表を散文の規約（`personal-web-app.md` の節）のみとする案を外した — 行の整合（共有ファイルの実在、要件 ID の実在、標準行の Baseline）を機械的に検証できないため（分析）。
- D2: Web Components(カスタム要素)で書く案を外した — 感触で外した(宣言的に見えるが、既存の面は関数の呼び出しで足りる。検討の記録もなく、実測していない)。TypeScript で書いて出力する案は、1 枚だけの HTML の区間 pin に出力された JavaScript をそのまま複写できる点で比較対象となる(未実測)。
- D3: React の面を新設して base-ui・recharts を使う案を外した — 面ごとにビルドや依存関係が増え、実行時に style を注入するライブラリは `style-src 'self'` と衝突する可能性がある（分析。実測していない）。Vaul は README での保守終了の表示により外した（分析）。Sonner・next-themes は要件 F3・C8 と方向性が逆のため外した（分析）。motion・NumberFlow・Cobe・torph・Leva・Satori は用途となる面がないため外した（分析）。

## Consequences

- 面を作成・修正する人（と Claude）は、手書きする前に `config/web/selection.toml` を参照する。表にない作業は、行を追加して裁定を残す。
- `popover` 属性が Baseline の widely に入ったら（newly から 30 か月後。low_date 2025-01-27 からの算出で 2027-07 ごろ。推論）、メニュー・ポップオーバーの行を `platform` に変更できる。それまでは、標準の行にできないことを検査で保証する。
- D2 の実装までは、保存の表示の状態切り替えは各面での手書きのまま（未実装、意図あり）。
- 各面が表の `shared` の部品を実際に使っているかの検査（出力した HTML の検査）は、この ADR の範囲外とする（未実装、意図あり）。

## 執行点

- `config/web/selection.toml`
- `crates/repo-structure-check/src/web_selection.rs`
- `crates/repo-structure-check/src/web_assets.rs`
- `crates/repo-structure-check/tests/tree.rs`

## Verification

- `cargo test -p repo-structure-check`: 対応表の検査（すべての選択肢を持つ表が通る、実物の表が通る、語彙外の選択肢・Widely でない `platform`・実在しない共有ファイル・版と `sha256` のない `external`・実在しない要件 ID・重複した `id` などがそれぞれエラーになる）と、`web_assets` の拡張（外部 `<script src>` の 3 種類の記述、`modulepreload`、スクリプトの `import("…")`・`from "…"` を拒否し、自サイトのスクリプト・インラインスクリプトは通す）が成功する。
- Baseline の値は web-platform-dx の公開 API（https://api.webstatus.dev/v1/features/dialog、`/details`、`/input-radio`、`/popover`。取得 2026-11-24）から取得した: `dialog` は widely（high_date 2024-09-14）、`details` は widely（2022-07-15）、`input-radio` は widely（2018-01-29）、`popover` は newly（low_date 2025-01-27、high_date は取得資料に記述なし）。
