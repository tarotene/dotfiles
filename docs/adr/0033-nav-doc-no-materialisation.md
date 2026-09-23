# ADR-0033 — 導線文書は実体を複製しない(nav-doc no-materialisation)

- Status: Accepted
- Date: 2026-09-23
- Issue: No-Issue(grill-me セッション中に発見・裁定)
- Amends: ADR-0016(Decision 5 の適用範囲を「導線文書同士の複製」から
  「導線文書が実体を複製すること」全般へ拡張する。ADR-0016 の他の
  Decision はそのまま有効)

## Context

別の private リポジトリ(具体名は publish-guard の denylist 方針により
伏せる)の README・AGENTS.md 系文書を調査した結果、実体が他所にある
情報(ディレクトリ構成図・コンテンツ一覧・他ファイルの中身の転記)を
大量に複製していた。実測で他ファイル転記 18
箇所・構成図 1 箇所・手書き一覧 23 箇所。同期は人力のみ(CI/hook による
検査ゼロ)で、6 件の drift が実際に発生していた(ワークフロー一覧の
欠落、構成図からの複数ディレクトリの欠落、ADR 索引の欠落、YAML スキーマ
転記の実体との不一致など)。加えて `AGENTS.md` に「`README.md` は同じ
標準を human-readable に要約せよ、両者を同一コミットで更新せよ」という
同期義務が明文で存在し、これが実際に発火して session 中にも新たな重複が
2 件生まれた。

ADR-0016 Decision 5 は「AI 向け正本は AGENTS.md 1 本、README/CONTRIBUTING
を参照する側に置き内容を複製しない」と定めたが、これは**導線文書同士の
複製**(AGENTS.md ↔ README)のみを対象にしており、**導線文書が実体
(ファイルシステムの構造・他ファイルの中身)を複製すること**は規約の
空白のままだった。その結果、dotfiles 自身も `AGENTS.md` の
`## Project Structure` 節に約 300 行の手書きディレクトリツリーを抱えて
おり、ADR 一覧が `AGENTS.md` と `docs/README.md` の 2 箇所に重複していた
(本 ADR の適用対象。段5 Consequences 参照)。

## Decision

1. **適用対象を「導線文書」に限定する。** ルート文書 allowlist
   (README.md / CONTRIBUTING.md / AGENTS.md / CLAUDE.md、ADR-0016
   Decision 3)と、各ディレクトリの `<dir>/README.md`。`docs/adr/**`
   (accepted 後 immutable な記録)と `docs/claude/**`(1 hook/skill の
   設計根拠を書く living document、ADR-0008 決定 3)はコンテンツの性質上
   対象外。
2. **判定軸は情報の寿命(変更頻度)にする。** 「この記述は、指す実体が
   変わるたびに書き換えが要るか」。ファイル追加・リネーム・設定値変更・
   コンテンツ追加で壊れる記述(ディレクトリツリー、ファイル一覧、他
   ファイルの内容の転記)は導線文書に書かない。目的・方針・命名規約・
   判定基準・1 行ポインタは寿命が長く、引き続き書いてよい。
3. **決定論チェックはパス密度とツリー、転記の判定は LLM ノードに
   切り出す。** ADR-0015 Decision 4 の基準(決定的に取得できる事実か、
   意味判断は LLM ノードへ切り出せるか)を満たすのは前二者のみ。
   - `nav-doc-tree-fence`: フェンスコードブロック内に `├`/`└`/`│` の
     いずれかが出現したら drift。
   - `nav-doc-path-inventory`: ATX 見出しで区切った節ごとに、パス様
     トークン(空白を含まず、`http(s)://` で始まらず、`/` を含むか
     既知拡張子で終わる inline code / fence 先頭トークン)をユニーク
     カウントし、`NAV_DOC_PATH_THRESHOLD`(既定 4)以上なら drift。
   - 他ファイルの転記(内容が意味的に一致するが字面は変えてある場合を
     含む)は決定論で判定できないため、`github-audit-triage` の LLM
     ノードが機械フラグの立ったリポジトリの導線文書を読んで起草時に
     併せて拾う。
4. **退避先は実体の隣に置く。** ツリー・一覧が運んでいた「このディレク
   トリは何か」という役割説明は、各 `<dir>/README.md` の冒頭 1 文へ
   移す。`<dir>/README.md` 自身は導線文書なので一覧までは書けないが、
   自分自身についての 1〜3 行は書ける。
5. **逃げ道はファイル内の閉じたマーカー 1 種にし、チェック名と理由を
   両方必須にする。** `<!-- nav-doc-exempt: <check> — <理由> -->`
   (`<check>` は `path-inventory` または `tree-fence`)を直後のブロック
   の直前に置くと、名指しされたチェックのみ判定から除外される。チェック
   名または理由を欠くマーカーは `nav-doc-exempt-malformed` として drift
   にする。マーカーを外しても drift しない(既に不要になった)exemption
   は `nav-doc-exempt-unused` として報告する。
6. **横断監査は既存の charters ドメインを拡張する。** 新規ドメインを
   作らない(ADR-0015 Decision 4 のドメイン追加基準に触れないため、
   ADR は 1 本で足りる)。

## Alternatives considered

- **一覧を自動生成し、CI で再生成漏れ(drift)を検査する。** Sphinx の
  `toctree` `:glob:` オプションや mkdocs-gen-files + mkdocs-literate-nav
  <https://mkdocstrings.github.io/recipes/> がエコシステムの確立解で
  ある。棄却理由: 本 ADR が対象にする一覧は生成器を書く価値がないほど
  小さく(数行〜十数行)、生成器自体が新たな保守対象になる。ただし
  `nav-doc-exempt` マーカーは生成物を通す設計なので、将来リポジトリの
  一覧が生成に見合う規模になった場合にこの解を採る道は塞いでいない。
- **明示アンカー + git SHA + AST フィンガープリントで参照先を追跡する。**
  Fiberplane の drift documentation linter(Laurynas Keturakis, 2026-03-25
  <https://fiberplane.com/blog/drift-documentation-linter/>、ベンダー
  ブログにつき一次の裏取りなし)がこの方式を採る。棄却理由: 横断監査
  (`github-audit`)は GraphQL 越しに README/CONTRIBUTING/AGENTS.md の
  テキストを取得するのみで `git ls-files` を持たず、この方式は載らない。
  かつ既存ドキュメント全体への一括アンカー付与という大きな移行コストを
  要する。
- **markdown linter の既存ルールをそのまま使う。** markdownlint
  <https://github.com/DavidAnson/markdownlint/blob/main/doc/Rules.md>・
  remark-lint <https://github.com/remarkjs/remark-lint> を調査したが、
  節内のパス密度を見るルールは存在しない(リンク系ルールは MD011/
  MD034/MD051/MD059 のみで密度閾値を持たない)。独自チェックを書く。

## Consequences

- `docs/claude/repo-charter.md` の `repo-charter` スキルが本 ADR の
  規約正本を持つ(§2 README の禁止事項、`<dir>/README.md` の扱い)。
- `github-audit` の charters ドメインに `nav-doc-tree-fence` /
  `nav-doc-path-inventory` / `nav-doc-exempt-malformed` /
  `nav-doc-exempt-unused` の 4 トークンが追加される(ADR-0015 実装)。
- `github-audit-triage` の LLM ノードが、機械フラグの立ったリポジトリで
  他ファイルの転記も併せて起草する。
- **本 ADR は適用範囲を導線文書のみに限定する。** arc42 §5 Building
  block view <https://docs.arc42.org/section-5/> や Diátaxis, Reference
  <https://diataxis.fr/reference/> は逆に「ドキュメント構造をソース構造
  に写せ」と推奨するが、これらの対象はアーキテクチャ文書・リファレンス
  文書であり、本 ADR が `docs/adr/**` と `docs/claude/**` を対象外に
  しているのはこの反証への応答である。
- dotfiles 自身の `AGENTS.md` の `## Project Structure` 節(約 300 行の
  ツリー)と ADR 一覧(`docs/README.md` と重複)は本 ADR の drift 第一号
  になる。本 ADR の実装 PR には含めず、`github-audit charters` が実際に
  `nav-doc-*` を報告してから `github-audit-triage` の一括起草で解体する
  (Issue で追跡)。
- 本 ADR を変更する場合は、これを supersede する新しい ADR を起こす
  (ADR-0008 の規約)。

## Verification

- `github-audit --selftest` — 以下 7 fixture を追加し ok/drifted の
  分岐を確認: 罫線なし構成図(パス密度で検出)/ 罫線ありツリー
  (tree-fence で検出)/ 散文の索引(パス密度で検出)/ 正当な 1〜2 パス
  言及(通る)/ 正しい exempt マーカー(検査対象から除外)/ チェック名
  または理由を欠くマーカー(`nav-doc-exempt-malformed`)/ 外しても
  drift しないマーカー(`nav-doc-exempt-unused`)。
- `github-audit charters --json` を実アカウントに対して実行し、dotfiles
  自身を含む複数リポジトリで `nav-doc-*` トークンが report されることを
  確認する。
- `shellcheck -S error scripts/github-audit`。
- `nix flake check`。
