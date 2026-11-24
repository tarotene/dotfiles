# ADR-0000 — ページの構造を、課題の性質の軸から選ぶ規則にし、すべての HTML に構造の宣言を要求する

- Status: Accepted
- Date: 2026-11-25
- Issue: No-Issue(ADR-1708 の原則の層の続き。構造を選ぶ規則・構造パターンの表・宣言の必須化・要件 F10 を足す)

## Context

ADR-1708 は、要件を原則の層に結びつけた。ページの構造(縦に 1 枚並べるか、1 件ずつ進むか)を選ぶ規則は、まだ無い。構造は、面ごとに、作る人の習慣で決まり、選んだ理由も、退けた案も、記録されない。ADR-1710 は、メール裁定ページを、触って選んだ結果として、ステップ型にした。この選択は、規則に書かれていないので、次の面を作るときに、再び習慣で決まる。

一次情報は、構造が「原則だけ」では決まらず、「原則と、課題の性質」から決まることを示す。同じ文書が、1 件ずつ進む構造の適所と、避ける場面を、どちらも書く。

- 1 件ずつ分けるのが助かるのは、手順が互いに依存しないとき: 「Staged disclosure is useful when you can divide a task into distinct steps that have little interaction.」依存して行き来するときは、逆に「It is problematic when the steps are interdependent and users must alternate between them.」(NN/g「Progressive Disclosure」。`nn-progressive`・`nn-progressive-interdep`)
- 初心者・まれな手順には向く: 「Use wizards for novice users or infrequent processes (e.g., configuration or setup).」繰り返し使う熟練者には向かない: 「wizards can quickly become annoying and overly controlling if they have to be used over and over again」(NN/g「Wizards」。`nn-wizards-novice`・`nn-wizards-overused`)
- 1 ページに 1 つが出発点で、内部向けの反復する利用者は統合してよい: 「User research will tell you when you can merge pages together. For example, if you’re designing an internal service for government users who need to repeat and switch between tasks quickly.」(GOV.UK Service Manual「Structuring forms」。`gov-one`・`gov-merge`)

つまり、課題の性質(項目の独立性・比較の要否・反復頻度・順序)の値から、構造の候補が決まる。この規則が、どこにも書かれていなかった。

## Decision

### D1: 課題の性質を 7 つの軸の閉語彙で表し、軸の値から構造パターンの候補を出す表を、`config/web/structures.toml` に置く

軸: 項目間の独立性(`independent`・`interdependent`)、比較・相互参照の要否(`none`・`needed`)、反復頻度・熟練度(`rare`・`daily`)、1 件あたりの情報量(`small`・`large`)、取り消しの可否(`undoable`・`irreversible`)、件数(`few`・`many`)、順序の自由度(`fixed`・`free`)。パターンは `step`(ステップ型)・`single-column`(縦 1 枚)・`document`(読み物・見本)の 3 つ。`fits` は、パターンが許す軸の値(空の表 = 何でも許す)で、`step` は、独立・比較なし・順序固定のときの既定(`default = true`)。既定は強制でなく、外れるときは理由を書く。`measure` は、描画したページで測れる約束(`step` は、ページ全体が縦にスクロールしない)。

この表は、原則の出典を原文で再読したパターンだけを載せた初期の表で、仮説である。`list-detail`・`task-list`・`dashboard` などは、出典を原文で再読したときに足す(未実装、意図あり)。

先行例: ADR-1703 の対応表 `config/web/selection.toml` と、その検査 `crates/repo-structure-check/src/web_selection.rs`(閉語彙の表と、別の表への参照整合) (取得 2026-11-25)
差分: 一致 — 閉じたキーと語彙、参照整合の型をそのまま使った。
本命: なし — 規則を表にしたいという目的から、既存の対応表と同型に決まった
対抗馬: 規則を文章(規約の散文)だけで書く。決定木を実行する新しい道具を書く
外した候補: 決定木を実行する道具 — 感触で外した(軸が 7 つ・パターンが 3 つの規模では、表と検査で足りると感じただけで、道具の案は比べていない)
既存手段: config/web/structures.toml — 拡張: config/web/selection.toml と crates/repo-structure-check/src/web_selection.rs の型
軸: 表現不可能 — 閉語彙 > 自由記述 + 事後 lint。存在しない軸・値・構造を、書けない形にする。

### D2: リポジトリ内のすべての HTML は、`<head>` に構造と軸を宣言する。宣言の無い HTML は、検査が落とす

宣言は、`<meta name="page-structure" content="step">`、`<meta name="page-axes" content="independence=independent;comparison=none;...">`(宣言する軸だけでよい)、外れるときは `<meta name="page-structure-deviation" content="<理由>">`。整合の規則は、(1) 構造・軸・値が語彙にある、(2) 宣言した構造の `fits` に挙げた軸は、すべて宣言する、(3) 軸の値が `fits` の外にあるときは理由を書く、(4) 軸の値が既定の構造に合う(既定の `fits` の軸をすべて宣言し、値が許す範囲内)のに、別の構造を選ぶときも理由を書く。検査は `repo-structure-check` の `web_structures` で、`web_assets` が走査する `*.html`・`*.htm` のすべてに当てる。宣言を免除する一覧は持たない。

先行例: ADR-1701 `docs/adr/1701-web-page-type-requirements.md`(ページの型を撮影定義 `visual.toml` の `types` で宣言する。省略は `common` 扱いで、宣言の無い面は落ちない) (取得 2026-11-25)
差分: 異なる — 型の宣言は撮影定義の側にあり、撮影定義を持たない面は検査の外になる。構造は「宣言の無いページを作れない」ことが目的なので、HTML 自身に置き、すべての HTML を走査して落とす。
本命: なし — 目的(宣言の必須化)から決まった
対抗馬: 撮影定義 `visual.toml` に `structure` キーを足す(ADR-1701 と同じ置き場)。`<html data-structure="…">` の属性での宣言
外した候補: `data-*` 属性 — 感触で外した(meta のほうが、`<head>` に置けて読み取りやすいと感じただけで、比べてはいない)
既存手段: crates/repo-structure-check/src/web_structures.rs — 拡張: crates/repo-structure-check/src/web_assets.rs の HTML の走査(`read_files`)と、`web_selection.rs` の型
軸: 表現不可能 — 宣言の無い HTML を、CI を通る形では足せなくする(宣言 > 手続き)。ただし、宣言が実際の描画と合っているかは、D3 の測れる範囲だけ。

### D3: 要件 F10 を共通の型に足し、`audit` が、宣言と描画を突き合わせる

F10 は、「ページの構造と軸を宣言し、構造は語彙にあって、軸の値と整合する。構造 `step` を宣言したページは、描画した画面全体が縦にスクロールしない」(`check = "audit"`、`audit = "structure"`)。要件は 42 件になる。`visual_diff.py audit` は、描画したページの `<meta name="page-structure">` を読み、宣言が無い・語彙に無い・`measure = "viewport-fit"` なのにページの高さが画面を超える、を違反にする。軸の整合は、`web_structures` が検査するので、`audit` には書かない(2 か所に同じ照合を書かない)。

先行例: ADR-1704 D2(`hover`・`input-size` の検査を、描画した状態の測定として足す。測定から違反を作る部分は、ブラウザ無しで試験できる純粋関数 `cell_findings`) (取得 2026-11-25)
差分: 一致 — 測定を `AUDIT_JS` に足し、違反を `cell_findings` に作る型をそのまま使った。
軸: 検出のみ — 宣言を、描画の 1 つの約束(縦にスクロールしない)に突き合わせるだけで、構造そのものの良し悪しは測れない。

### D4: 構造に効く原則を、原則の表(`principles.toml`)に足す

`gov-merge`・`nn-wizards-novice`・`nn-wizards-overused`・`nn-progressive-interdep` を、`scope = "structure"` で足した。引用は、原文を取得して逐語で照合した(ADR-1708 D3 と同じ)。`scope = "structure"` の原則は、どれかのパターンか、どれかの要件が指す。どちらも指さない原則は、検査が落とす。

先行例: ADR-1708 D1〜D3(原則の表と、引用の原文照合) (取得 2026-11-25)
差分: 一致 — 同じ表・同じ検査に、構造に効く原則の行を足した。
軸: 表現不可能 — 単一正本 > 複写 + 同期。パターンが原則を ID で指し、本文を複写しない。

### D5: 既知の 3 つの HTML の宣言は、現状の構造の記述であり、構造の選択ではない

裁定ページの雛形は、ADR-1710 で選んだ `step`(7 つの軸をすべて宣言)。見本ページは `document`(読み物。軸の宣言は要らない)。モデル同等性マップは `single-column`(独立性 = `interdependent`、比較 = `needed`、順序 = `free`。軸の値が既定に合わないので、理由は要らない)。後の 2 つは、現状の構造を書いただけで、練習用の画面で構造を選び直したものではない。構造を変える面は、`docs/agents/personal-web-app.md` の工程(軸の値を書く → パターンを選び、退けた案と理由を書く → 練習用の画面で本人が選ぶ → 検査)に従う。

軸: 検出のみ — 軸の値の判断は、宣言した人の判断で、機械は語彙と整合だけを見る。

## Alternatives considered

- **規則を、規約の文章だけで書く**: 守られなくても、機械が気づかない。構造は、書いた後でなく、設計の時点で決まるので、宣言を要求する検査が要る。採らない。
- **撮影定義に構造を宣言させる**: 撮影定義を持たない HTML が検査の外に残る。HTML 自身の宣言を、すべての HTML に要求するほうが、宣言の無いページを作れなくできる。採らない。
- **パターンを最初から 6 つ以上にする(一覧と詳細・タスク一覧・ダッシュボードを含む)**: 原則の出典を、原文で再読していない。出典の無いパターンを、表に載せない。採らない(足すのは、続く作業)。
- **宣言を免除する一覧を持つ**: 例外を許すと、宣言の無いページが、一覧に載る形で増える。今は、すべての HTML が宣言できるので、一覧を持たない。

## Consequences

- 要件は 42 件になる。F10 は、`common` の型なので、撮影定義を持つ面(裁定ページの雛形・見本ページ)で `audit` が測る(どちらも違反 0 件)。
- ページ(HTML)を足すとき、構造の宣言が要る。軸の値が既定(`step`)に合うのに別の構造を選ぶと、理由が要る。
- 画面を作る・変える計画に、構造の欄を求める設計時点の検査は、続く ADR で足す(未実装、意図あり)。
- 表は、初期の仮説である。構造を選んだ実例(裁定ページ)が増えたら、軸の値と `fits` を見直す。

## 執行点

- `config/web/structures.toml`
- `config/web/principles.toml`
- `config/web/page-types.toml`
- `config/web/specimen.html`
- `crates/model-equivalence/page.html`
- `crates/repo-structure-check/src/web_structures.rs`
- `crates/repo-structure-check/src/lib.rs`
- `crates/repo-structure-check/tests/tree.rs`
- `config/agents/skills/pr-description/scripts/visual_diff.py`

## Verification

- `cargo test -p repo-structure-check -p model-equivalence`: `web_structures` の単体試験(表の形・`fits` と原則の実在・死んだ構造の原則・宣言の有無・宣言の語彙・軸の整合・理由が要る 2 つの場合・属性の読み方)、ツリーへの適用(`the_structures_table_is_closed_and_every_html_declares_its_structure`)、番兵(`the_structures_check_reads_what_it_is_about`)が成功する。
- `uv run --script config/agents/skills/pr-description/scripts/visual_diff.py --selftest`(ブラウザ無しと `--browser`): 実物の `page-types.toml` の 42 件、構造の検査(宣言が無い・語彙に無い・`step` で縦にスクロールする、は違反。`document` は縦に長くても違反にしない。語彙を渡さないときは宣言が在ることだけを見る)、わざと悪いページが 13 の検査のすべてで落ち、良いページが 13 の検査のすべてを通ること。
- `visual_diff.py audit`(2026-11-25): 裁定ページ(違反 0 件、通った検査 12 件。F10 を含む)、見本ページ(違反 0 件、通った検査 9 件。F10 を含む)。
- 外部の引用の照合(2026-11-25): 追記した 4 件の `quote` が、取得した原文にそのまま含まれること。
