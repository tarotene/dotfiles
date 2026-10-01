# ADR-640 — 技術スタック標準の API ドキュメントのビルドを全リポジトリで必須にする

- Status: Accepted
- Date: 2026-10-01
- Issue: No-Issue(`/grill-me` セッション中に発見・裁定。「Rust なら rustdoc、
  Python なら … と、スタックに応じた標準ドキュメントのビルドを全リポジトリに
  強制し、ビルド後の扱いもベストプラクティスを調べて audit スキル周辺に撒きたい」
  という依頼から始まった)

## Context

owner `tarotene` のリポジトリ群には、スタック標準の API ドキュメント生成
(rustdoc / Sphinx / TypeDoc)を CI で検査するものが無い。doc コメントの
未解決リンクや壊れた doctest は、レビューで気付かない限り main に入る。
一方で、既に ADR-591 が「required check は集約 job `CI passed` 1 本 +
`PR title`」を確立し、ADR-0015 が「`github-audit` が drift を検出し、
`github-audit-triage` が一括 PR 化する」ループを確立している。今回の課題は
新しい強制機構を作ることではなく、この既存の経路に docs ビルドを載せること。

調査と fixture での実測(いずれも取得 2026-10-01)で分かったこと:

- **rustdoc**: 既定で warn の lint(`broken_intra_doc_links` /
  `invalid_html_tags` / `bare_urls` など)は `RUSTDOCFLAGS="-D warnings"` で
  すべて error になる(rust-lang, The rustdoc book "Lints",
  <https://doc.rust-lang.org/rustdoc/lints.html>)。tokio-rs/tokio の
  `.github/workflows/ci.yml` の docs job は nightly で `--cfg docsrs
  --cfg tokio_unstable -Dwarnings` を使い、`cargo test --doc` と
  cargo-semver-checks を別 job で回している。docs.rs は crates.io の
  リリースを自動でビルドする(docs.rs, "Builds",
  <https://docs.rs/about/builds>)。
- **Python**: PyPA は doc ツールを推奨しない。"Creating documentation"
  チュートリアルは「パッケージングと無関係」として削除され、Sphinx 自身の
  チュートリアルへ誘導している(PyPA, Python Packaging User Guide,
  <https://packaging.python.org/en/latest/tutorials/creating-documentation/>)。
  Sphinx は `sphinx-build -W` で警告をエラーにでき、`-n`(nitpicky)で未解決
  参照を警告にする(Sphinx, sphinx-build,
  <https://www.sphinx-doc.org/en/master/man/sphinx-build.html>)。Material for
  MkDocs は 9.7.0(2025-11-11)で保守モードに入り、後継 Zensical への移行が
  案内されている(squidfunk/mkdocs-material releases)。
- **TypeScript**: TypeDoc は `--treatWarningsAsErrors` を持つ(TypeDoc,
  Options: Validation, <https://typedoc.org/documents/Options.Validation.html>)。
  TypeDoc 0.28.20 の peerDependencies は `typescript 5.0.x … 6.0.x` で、
  TypeScript 7.0.2(npm の `latest`)を入れたリポジトリでは起動時に
  `TypeError` で落ちることを実測した。
- **Sphinx の doctest**: `sphinx.ext.doctest` は autodoc 対象モジュールを自動で
  import しないため、docstring の `>>>` が `NameError` で落ちる(実測)。
  `doctest_global_setup` でパッケージを import すると通る。
- **Pages の可視性**: GitHub Free では Pages は public repo のみ。Pro では
  private repo でも使えるが、公開されるサイトは誰でも閲覧できる。閲覧制限は
  GitHub Enterprise Cloud の組織所有リポジトリ限定(GitHub Docs, "GitHub's
  plans" / "Changing the visibility of your GitHub Pages site",
  <https://docs.github.com/en/pages/getting-started-with-github-pages/changing-the-visibility-of-your-github-pages-site>)。

## Decision

### D1: docs ジョブは `ci.yml` に置き、`CI passed` の needs に含めて必須化する

ruleset の required 集合 `["CI passed", "PR title"]` は変えない(ADR-591
D1)。`ci.yml` の全 job は `ci-passed.needs` に含まれるという不変条件を
既存の `workflows` ドメインが検出するので、docs ジョブの必須化はその仕組みに
乗る。軸: 表現不可能(required の正本は `needs` 配列の単一正本)。

### D2: ビルドの実体は composite action `docs-<stack>` に置く

`tarotene/dotfiles/.github/actions/docs-{rust,python,typescript}` を
`@main` で参照する。ADR-591 D3(pr-title)と同じ理由で reusable workflow に
しない — reusable は `<caller job name> / <reusable job name>` の連結名を作り、
required check の context 不一致事故(#337)の型になる。strict の基準
(`-D warnings` など)は action が単一正本で、呼び出し側は緩められない。
`@main` 参照は変更が全リポジトリの CI へ即時に届くため、`tests/docs-
fixtures` の正常系・壊した系を dotfiles の CI(`docs-actions` job)で固定する。

### D3: 適用判定は「マニフェスト → ツール」の閉表で行う

| マニフェスト | 条件 | ツール | strict 呼び出し |
|---|---|---|---|
| `Cargo.toml` | — | rustdoc | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace` + `cargo test --doc --workspace` |
| `pyproject.toml` | — | Sphinx + autodoc | `sphinx-build -W -n` + `sphinx-build -W -b doctest` |
| `package.json` | `exports` / `main` / `types` のいずれかを持つライブラリ | TypeDoc | `typedoc --treatWarningsAsErrors` |

表に載らないスタック(Astro サイト・Nix・Go・Typst・GAS・設定のみ)は `n/a`
で、drift に数えない。新しいスタックは表に行を足して有効にする。除外は既存の
`overrides.tsv` を docs ドメイン単位で使う。軸: 表現不可能(閉語彙)。

### D4: 合格条件は「警告をエラー扱い」まで。カバレッジは要求しない

`missing_docs` / `interrogate --fail-under` のようなカバレッジ lint は、既存
リポジトリへの後追い負担が大きい割に、壊れた doc を防ぐ効果は strict ビルドが
既に担う。各リポジトリの lint 設定に任せる。Rust は stable で済む範囲に留める
(tokio のように nightly + `--document-private-items` は要求しない)。
`[package.metadata.docs.rs]` と `--cfg docsrs` を再現したい crate 向けに、
nightly を使う `docsrs: 'true'` を opt-in で用意する。軸: 検出のみ。

### D5: Python は Sphinx + autodoc。設定が無ければ sphinx-apidoc で最小構成を生成する

`docs/conf.py` があればそれを使い、無ければ `sphinx-apidoc --full
--ext-doctest` で生成して `doctest_global_setup` を足す(Context の実測)。
Python 用の governance スキルは新設しない — 撥き口は triage だけで足りる。
MkDocs + mkdocstrings(`--strict`)を対抗馬としたが、定番テーマが保守モードに
入っており、Sphinx の方が strict 化の一次情報が揃う。pdoc は警告をエラー化する
フラグを一次情報で確認できなかったため、感触で外した。軸: 検出のみ。

### D6: TypeDoc と TypeScript は action が隔離ディレクトリに pin して入れる

リポジトリ自身の `node_modules` の TypeScript を使うと、strict の成否が
リポジトリの TS バージョンに依存し、7 系では TypeDoc が起動すらできない
(Context)。`$RUNNER_TEMP` に `typedoc@0.28.20` と `typescript@6.0.3` を
`--save-exact` で入れる(軸: 表現不可能 — pin 固定 > 浮動)。entry point は
TypeDoc が `package.json` の exports / main から推論する(実測)。

### D7: 公開は「PUBLIC かつ docs.rs 対象外」だけ Pages、PRIVATE は artifact のみ

- PUBLIC かつ docs.rs / pkg.go.dev の対象外: main への push で Pages へ
  deploy する(テンプレート `docs-pages.yml`、PR ゲートとは別 workflow)。
- crates.io 公開済み crate: docs.rs に任せる(重複して自前 deploy しない)。
- PRIVATE: workflow artifact(7 日保持)だけに留める。個人アカウントでは
  private repo の Pages も公開されるため、Pages が有効なら `github-audit` の
  docs ドメイン(後続の段)が `private-pages-enabled` として drift 報告する。

軸: 検出のみ — Pages の有効化は GitHub 側の設定で、宣言から強制できない。

### D8: 後段検査は doctest を必須に含め、外部リンク検査は cron で回す

doctest は action 内で常に実行する(`cargo test --doc --workspace`、
`sphinx-build -b doctest`)。lib ターゲットが無い workspace では
`cargo test --doc` が失敗するため、lib の有無で分岐する。外部リンクは
lychee を週次 cron で回し(`docs-linkcheck.yml`)、壊れていれば Issue に
起票または追記する。外部サイトの状態は PR 時点で決まらないため必須にしない。
内部リンクは strict ビルドが担う。

### D9: 監査は新しい `docs` ドメインに置き、`needs` の網羅は既存の `workflows` に任せる

`github-audit` に `docs` ドメインを追加する(閉表による適用判定、`ci.yml` に
`docs-<stack>` を `uses:` するジョブの有無、PRIVATE の Pages 有効)。`needs:`
の網羅検査は `judge_workflows()` が既に担うので二重に実装しない。軸: 還元。
`docs` ドメインの実装は、この ADR と同じ stacked PR チェーンの、action・
テンプレート・fixture を着地させる段より後の段に置く。

### D10: 既存リポジトリへの展開は `github-audit-triage` の一括 PR で行う

ADR-0015 / ADR-591 D8 と同じ経路。新規リポジトリは `*-repo-governance` の
テンプレートが最初から docs ジョブを含む。新しい展開機構は作らない。

## Alternatives considered

- **reusable workflow(`workflow_call`)で中央化**: 当初の案。ADR-591 D3 の
  連結名問題と caller→callee の権限縮小を再導入するため不採用(D2)。
- **各リポジトリへジョブ本文を複写**: strict の基準がリポジトリごとに漂流し、
  判定が YAML 内のコマンド文字列のヒューリスティックになる。感触で外した。
- **MkDocs + mkdocstrings**: 定番テーマの Material が保守モード。不採用(D5)。
- **TypeScript は TypeDoc 以外**(api-extractor + api-documenter): 設定が重く
  単一 action で閉じにくい。不採用(D6)。
- **PRIVATE リポジトリも Pages へ**: 個人アカウントでは公開されるため不採用(D7)。
- **外部リンク検査を必須チェックにする**: 外部サイトの一時的な不調で PR が
  赤くなる。不採用(D8)。

## Consequences

- 全リポジトリの `ci.yml` に `docs` job が増え、`CI passed` の `needs` に入る。
  既存の doc 警告があるリポジトリは、展開 PR でその修正を含める必要がある。
- `@main` 参照のため、この action の変更は全リポジトリに即時に効く。dotfiles
  の `docs-actions` job が防波堤になる。
- ツール本体の pin(Sphinx 9.1.0、TypeDoc 0.28.20、TypeScript 6.0.3)は
  Renovate の `github-actions` マネージャの対象外で、追従は手動(各 action の
  `*_PIN` コメントを目印にする)。composite action 内の action の SHA pin は
  標準の `github-actions` マネージャが追う。
- 同じ stacked PR チェーンの後続の段で、`github-audit` の `docs` ドメインと、
  `github-audit-triage` の docs 起草手順、`*-repo-governance` テンプレートへの
  `docs` job 追加を入れる。

## 執行点

- .github/actions/docs-rust/action.yml
- .github/actions/docs-python/action.yml
- .github/actions/docs-typescript/action.yml
- .github/workflows/ci.yml
- tests/docs-fixtures/README.md
- config/claude/skills/repo-governance-common/templates/.github/workflows/docs-pages.yml
- config/claude/skills/repo-governance-common/templates/.github/workflows/docs-linkcheck.yml

## Verification

- dotfiles の CI の `API docs` job(dotfiles 自身の Cargo workspace に
  `docs-rust` を適用)と `Docs actions (fixtures)` job(正常な fixture が成功し、
  未解決リンクを入れた fixture が失敗することを 3 スタックで固定)が green に
  なること。この ADR を導入する PR 自身がその自己適用の実測になる。
- `docs-rust` の `docsrs: 'true'` 経路(nightly)は `Docs actions (fixtures)`
  job でのみ実行される(ローカルに nightly が無いため)。
