# ADR-1708 — ページの型の要件を、原則の表に結びつける。要件が原則に結びつかなければ、検査が落とす

- Status: Accepted
- Date: 2026-11-25
- Issue: No-Issue(ADR-1701 の続き。要件が、どの原則から起こされたかを、検査できる形にする。ページの構造を選ぶ規則を設計の時点で守らせる作業の、最初の層)

## Context

ADR-1701 は、ページの型の要件を `config/web/page-types.toml` に 41 件起こした。要件の出典は、ファイルの先頭のコメント(「WCAG 2.2・NN/g のヒューリスティック・デジタル庁デザインシステム・GOV.UK などの公開の基準から起こした」)に、名前が挙がるだけである。どの要件が、どの原則から来たかの対応は、どこにも書かれていない。

その結果、原則が要件へ落とし込まれる段階で消失する恐れがある。ADR-1677 D5 では要件の例として「1 画面に 1 つの判断」を挙げたが、`page-types.toml` では裁定フォームの F1 が「1 画面の主操作は 1 つ」となり、「1 画面に 1 つの判断」という原則自体は要件に残らなかった。原則を追加・修正する際に依存する要件を把握できず、要件を追加する際にも原則と照合したかを検証する手段がない。

ページの構造(1 件ずつ進むか、縦 1 枚か、一覧と詳細か)を、課題の性質から選ぶ規則を置くには、その手前に、原則の層が要る。この ADR は、その最初の層だけを決める。構造を選ぶ規則・構造パターンの表・構造の宣言を求める要件・計画の検査は、続く ADR で扱う(未実装、意図あり)。

## Decision

### D1: 原則は、`config/web/principles.toml` に、閉じた語彙の表として置く

各原則は、`id`・`title`・`kind`(`principle` = 外部の原則・規格・公式の指針 / `decision` = このリポジトリの決定)・`scope`(`structure` = ページの構造を選ぶ規則にも効く / `general` = 個々の要件の根拠)・`source`・`retrieved`(原文を読んだ日)・`quote`(逐語の引用)を、すべて持つ。キーは閉じていて、知らないキー・語彙の外の `kind`・`scope`・重複した `id`・空の欄・`YYYY-MM-DD` でない日付は、検査で落ちる。

先行例: ADR-1703 の対応表 `config/web/selection.toml` と、その検査 `crates/repo-structure-check/src/web_selection.rs`(閉語彙の表と、別ファイルの ID への参照整合を落とす) (取得 2026-11-25)
差分: 一致 — 閉じたキーと語彙、`id` の形と重複、参照整合を落とす型をそのまま使った。
軸: 表現不可能 — 閉語彙 > 自由記述 + 事後 lint。出典と逐語の引用を持たない原則を、書けない形にした。

### D2: 各要件は、`principles = [...]` で、原則を 1 つ以上指す。実在しない原則を指すこと、原則に結びつかない要件、どの要件にも使われない一般の原則を、検査が落とす

`page-types.toml` の 41 件すべてに、`principles` を足した。束ね方は、要件ごとの元の出典(WCAG の達成基準、ヒューリスティックの番号、各公式の指針、このリポジトリの決定)から起こした。`scope = "general"` の原則が、どの要件にも指されていなければ、死んだ原則として落ちる。`scope = "structure"` の原則は、続く ADR の構造パターンの表が指すので、この検査の対象外にする。

`visual_diff.py` の `load_types` は、`principles` の形(原則の ID の、空でない配列。重複なし)を見て、`audit` の違反の出力に、要件の ID に続けて原則の ID を添える。実在の照合は、`repo-structure-check` の `web_principles` が行う(2 か所に同じ照合を書かない)。

先行例: ADR-1703 D1 の `not-used` の行の `req`(`page-types.toml` に実在する要件の ID を指す)。ADR-1701 D1(`load_types` が、種別と検査の名前の語彙を閉じ、結びつかない検査を落とす) (取得 2026-11-25)
差分: 異なる — 先行例は、表から要件への参照(`req`)を検査する。ここは、要件から原則への参照を検査し、さらに逆向き(どの要件にも使われない原則)も落とす。
軸: 表現不可能 — 単一正本 > 複写 + 同期。原則の本文を要件に複写せず、ID で参照させ、結びつかない要件と死んだ原則を、書けない形にした。

### D3: 外部の原則の引用は、原文を取り直して、逐語で照合したものだけを載せる。決定の引用は、出典のファイルに含まれることを、検査が照合する

`kind = "principle"` の `quote` は、載せるときに、取得した原文(HTML または PDF のテキスト)の中に、そのまま(空白だけを畳んで)含まれることを確かめた。取得は 2026-11-25。W3C・NN/g・GOV.UK・デジタル庁・GitHub Primer・WAI-ARIA APG・Roediger & Karpicke(2006)・Mogavi ら(2022)の 33 件を載せた。`kind = "decision"` の `quote` は、`source` のリポジトリ内のファイルに含まれることを、`web_principles` が毎回照合する(ADR-1677 D6・ADR-1703 D4・ADR-1704 D2 の 4 件)。

取得を試みず、載せなかった原典がある(Shneiderman の黄金律、Soderstrom & Bjork 2015、Dunlosky ら 2013、Butler ら 2007、Few 2007、Calm Technology Institute)。それらを出典に挙げていた要件(L3・D1・D6 など)は、取得できた別の原則(ヒューリスティック 8)に結びつけた。出典として弱くなった要件がある点は、続く作業で、原典を再読して足す(未実装、意図あり)。外部の引用は、ネットワークを使わない検査では、原文と照合できない。

先行例: `config/agents/AGENTS.md` の「WebFetch は取得したページを小さなモデルで要約して返し、要約には原文に無い断定が混じることがある」とする規律(要約だけで得た記述を、一次資料の記述として成果物に載せない)と、`docs/adr/1677-web-design-baseline.md` の D6 の先行例の欄(「以前の読み取りの出典で、今回は再読していない」と、再読の有無を明記する形) (取得 2026-11-25)
差分: 異なる — 先行例は、再読していないことを書き添える運用。ここは、再読していない引用を、表に載せない。
軸: 検出のみ — 外部の引用が原文のままかは、載せる時点の手順でしか確かめられない(検査はネットワークを使わない)。決定の引用だけは、機械で照合できる。

## Alternatives considered

- **要件の出典を現行のままファイルの先頭コメントに書く**: 要件と原則の対応は人が読まないと分からず、結びつかない要件や使われていない原則も検査できない。採用しない。
- **Google Labs の `DESIGN.md`(散文と機械可読のトークンを 1 つのファイルに置く形式)**: README の要旨までしか読んでおらず、仕様の全文は読んでいない。版表記が alpha であること、要件との結びつきの検査が無いことだけを見て外した(感触で外した)。
- **原則を `page-types.toml` の要件に、逐語の引用ごと複写する**: 同じ原則を指す要件が複数あり(例: 1.4.1 色の使用は C5・L7・D3)、引用が複写されて食い違う。単一正本に反する。採らない。

## Consequences

- 要件は 41 件のまま、すべてが原則を 1 つ以上指す。原則は 37 件(外部の原則 33、決定 4。`scope = "structure"` は 4)。
- 要件を足すときは、先に原則を `principles.toml` に足す(引用を原文で照合する)か、既存の原則を指す。原則が足りない要件は置けない。
- 続く ADR で、構造を選ぶ規則(軸 → 構造パターン)と、構造の宣言を求める要件を足す。そのときに足す `scope = "structure"` の原則は、構造パターンの表が指す。

## 執行点

- `config/web/principles.toml`
- `config/web/page-types.toml`
- `crates/repo-structure-check/src/web_principles.rs`
- `crates/repo-structure-check/src/lib.rs`
- `crates/repo-structure-check/tests/tree.rs`
- `config/agents/skills/pr-description/scripts/visual_diff.py`

## Verification

- `cargo test -p repo-structure-check`: `web_principles` の単体試験(正常な表が通る、知らないキー・語彙の外・必須の欠け・`id` の重複・日付の形・出典の種別・決定の引用の不一致・実在しない原則・原則を持たない要件・死んだ一般の原則・空の表、が落ちる)と、ツリーへの適用(`the_principles_table_is_closed_and_every_requirement_is_grounded`)、番兵(`the_principles_check_reads_what_it_is_about`)が成功する。
- `uv run --script config/agents/skills/pr-description/scripts/visual_diff.py --selftest`(ブラウザ無し): 実物の `page-types.toml` のすべての要件が `principles` を持つこと、`principles` の欠け・空・ID の形・重複が落ちることが成功する。
- 外部の引用の照合(2026-11-25): 取得した 16 の出典ページの原文から、HTML のタグを除いたテキスト(PDF は読み順のテキスト)に、33 件の `quote` がそのまま含まれること。WCAG の `source` の fragment が、原文の `id` に実在すること。
