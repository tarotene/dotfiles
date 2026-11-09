# docs-index — `docs/README.md` の索引を、文書の H1 から生成する

実装: `crates/docs-index`(ライブラリと bin `docs-index`)
走る場所: 手元は `cargo run -p docs-index -- render`(書き直し)と `cargo run -p docs-index -- --check`(検査)。
CI は `Rust workspace` ジョブの `cargo test --workspace` が `crates/docs-index/tests/tree.rs` で検査する
Issue: #1450(親の tracking Issue は #1433)

## 動機

`docs/README.md` は、各文書へのリンクと、手で書いた要約の一覧だった(203 項目、820 行)。一覧は 2 つの腐り方をした。

- 文書を足して索引に載せ忘れる(18 件が未リンクだった。ADR は `adr-number-check` が網羅を見るが、ADR 以外は誰も見ない)。
- 要約が文書の内容とずれる(ADR-0035 の第 2 軸を「保守コスト最小」と書いたまま、決定は「還元性」に改まっていた)。

一覧は、指す実体(文書)が増えるたびに書き換えが要る、寿命の短い記述である(ADR-0033 の判定軸)。ADR-0033 は
「一覧が生成に見合う規模になったら、生成を採る道は塞いでいない」と記録した(`## Alternatives considered`)。
203 項目はその規模に達したので、索引の一覧を文書の H1 から生成する。

## 生成の規則

`docs/README.md` の中の、`<!-- BEGIN GENERATED: docs-index <id> -->` と `<!-- END GENERATED -->` の間だけを生成する。
外の文章は手で書いたまま残る。節は 4 つ。

| id | 読む場所 | 1 行の形 | 並び |
|---|---|---|---|
| `adr` | `docs/adr/*.md` | `- [ADR-<ファイル名の番号>](adr/<ファイル>) — <H1 から ADR-N の接頭辞を除いた題名>` | 番号の昇順。仮番号 `0000` は最後 |
| `root` | `docs/*.md`(`README.md` を除く) | ``- [`<ファイル>`](<ファイル>) — <H1>`` | ファイル名順 |
| `agents` | `docs/agents/*.md` | ``- [`agents/<ファイル>`](agents/<ファイル>) — <H1>`` | ファイル名順 |
| `claude` | `docs/claude/*.md` | 同上 | ファイル名順 |

- 1 行はリンクと H1 だけ。手で書いた要約は持たない。題名が索引で意味を持つように、H1 は「何の文書か」を言う。
- H1 が無い文書は、索引を作れないので失敗にする(失敗の文面に文書のパスを含む)。
- 索引の節を持たない置き場(`docs/<新しい dir>/*.md`)と、入れ子の文書(`docs/adr/x/y.md`)は、失敗にする。置き場を足すときは、
  `BLOCKS` に節を足し、README にマーカーを置く。
- ADR の `[ADR-<番号>]` の形は、`scripts/adr-number-check` の索引の網羅の検査(`\[ADR-<番号>\]`)が読む。
  番号は、ファイル名の番号の文字列のまま使う(`872` と `0033` が混在する)。

## ADR の仮番号と改番

ADR は仮番号 `0000-<slug>.md` で起草し、PR を作ったあとに `scripts/adr-number-check --fix <PR番号>` で改番する
(`adr-numbering.md`)。`--fix` は README の `ADR-0000` を含む行も書き換えるが、並びは直さない。**`--fix` のあとは
`cargo run -p docs-index -- render` を実行して、並びを直す**(実行しないと、`tree.rs` が落ちる)。

## 検査

| 検査 | 内容 |
|---|---|
| `tests/tree.rs` の `the_docs_index_matches_the_h1_of_every_document` | あるべき `docs/README.md` と現在の内容が一致する。落ちたら、最初に違う行と `render` の案内を出す |
| `tests/tree.rs` の `every_document_appears_exactly_once_in_the_generated_blocks` | 生成の節の中で、どの文書のリンクもちょうど 1 回出る |
| `src/lib.rs` のテスト | H1 の読み(コードの囲みの中を読まない)、ADR の並び(`0000` が最後)、マーカーの欠落、置き場の知らない文書 |

## 既存手段との関係

| 既存手段 | 見たこと(2026-11-09) | このリポジトリで採らない理由 |
|---|---|---|
| `mkdocs-literate-nav`、`mkdocs-gen-files`(ADR-0033 が確立解として挙げる) | どちらも MkDocs のプラグインで、サイトの nav を作る。保守は続いている(最終 push は 2026-11-06 と 2026-04-17) | MkDocs のサイトを持たない。コミットする README の節は作れない |
| `adr generate toc`(adr-tools、`npryce/adr-tools` の `src/_adr_generate_toc`) | ADR 専用で、固定の H1 つきの文書全体を標準出力に出す。リンクは `* [<H1>](<file>)`。最終 push は 2024-04-25 | README の中の節を置き換えられない。ADR 以外の 3 つの節を作れない。`[ADR-<番号>]` の形と仮番号の並びを作れない |

## ADR-0033 との関係

ADR-0033 の `nav-doc-exempt` マーカーは、生成物を通す設計だったが、`docs/README.md` には使わない。`github-audit` の
`nav_doc_scan` が走査するのはルートの README・CONTRIBUTING・AGENTS.md・CLAUDE.md で、`docs/README.md` は対象外
(`crates/github-audit/src/domains/navdoc.rs`)。対象外のファイルにマーカーを置くと、使われないマーカーの検出に当たる。

## 執行点

- `crates/docs-index/src/lib.rs`
- `crates/docs-index/src/main.rs`
- `crates/docs-index/tests/tree.rs`
- `.github/actions/stack-position/action.yml`
