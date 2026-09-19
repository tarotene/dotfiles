# ADR-0017 — CONTRIBUTING.md の固定スキーマと「Issue litmus」語彙の廃止

- Status: Accepted
- Date: 2026-09-19
- Issue: No-Issue(grill-me セッション中に発見・裁定)
- Supersedes: ADR-0013 Decision 1 の Issue litmus 項、ADR-0016 Decision 2 を
  部分 supersede する。ADR-0016 の他の Decision(README 全節固定スキーマ・
  ルート allowlist・言語正本・AGENTS.md/CLAUDE.md ルーティング・skills
  ルーティング)はそのまま有効。

## Context

ADR-0016 は「Issue litmus は CONTRIBUTING.md へ移設する」とだけ決め、
CONTRIBUTING.md **自体**の節構成には一次情報の裏付けを与えなかった
(README 側は standard-readme 等の一次情報で全節固定スキーマを根拠づけた
のと非対称)。結果として実際に播かれた CONTRIBUTING.md
(`# Contributing` → `## Issue litmus` → `判定問:`/`採用例:`/`棄却例:`)は、
ADR-0016 Decision 4(README/CONTRIBUTING は英語正本 1 本)に**自ら違反**
する日英混在ファイルになった。混在の発生源は `repo-charter` SKILL.md §3 の
テンプレそのものが日本語ラベルだったこと、および `scripts/github-audit`
の selftest fixture が既に全英語(`Judging question:`)でテンプレと
食い違っていたことの 2 つ。

加えて、`scripts/github-audit` の言語混在判定 `is_language_mixed` は
「CJK 文字数 ≥20 かつ Latin 文字数 ≥20」で初めて drift とする対称
ヒューリスティックで、日本語ラベル 9 文字程度の軽微な混在はすり抜けた
(実際に drift として検出されなかった)。

## 一次情報(取得日 2026-09-19、個別調査で確認)

- GitHub 公式 "Setting guidelines for repository contributors"
  <https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/setting-guidelines-for-repository-contributors>
  — CONTRIBUTING.md の内容は "could include"(任意列挙)であり、必須節構成
  を定義しない。逐語で挙げるのは "Steps for creating good issues or pull
  requests" と "Links to external documentation … or a code of conduct"。
  配置は `.github/` → repo ルート → `docs/` の優先順位。
- GitHub Open Source Guides "Starting a Project"
  <https://opensource.guide/starting-a-project/> — CONTRIBUTING が答える
  べき項目として bug 報告・機能提案・環境構築とテスト・受け入れる貢献の
  種類・ロードマップ・連絡方法を列挙。初期段階のプロジェクトは簡素な
  CONTRIBUTING でよいと明言。
- GitHub Open Source Guides "Best Practices for Maintainers"
  <https://opensource.guide/best-practices/> — "The types of contributions
  you'll accept" の明文化を推奨。単独/ボランティアメンテナへの唯一の
  直接言及: "If maintaining your project is part-time or purely
  volunteered, be honest about how much time you have."
- nayafia/contributing-template(Nadia Asparouhova, CC0, 2016 年頃)
  <https://github.com/nayafia/contributing-template> — 40 の OSS の実例
  調査に基づくチェックリスト。Introduction / Ground Rules / Your First
  Contribution / Getting started / How to report a bug / How to suggest a
  feature / Code review process / Community / (Bonus) の節構成。
- Mozilla Science Lab "Wrangling Web Contributions: How to Build a
  CONTRIBUTING.md"
  <https://mozillascience.github.io/working-open-workshop/contributing/>
  (Working Open Workshop, 2016 年頃) — "CONTRIBUTING.md should be in your
  root directory, think of it as a anchor for your project"。
- 実例(取得日 2026-09-19): atom/atom の CONTRIBUTING.md
  <https://github.com/atom/atom/blob/master/CONTRIBUTING.md> は全文を
  1 ファイルに収める型。facebook/react
  <https://legacy.reactjs.org/docs/how-to-contribute.html> と
  kubernetes/community
  <https://github.com/kubernetes/community/blob/master/contributors/guide/README.md>
  は「リポジトリ内 CONTRIBUTING.md は中央ガイドへのポインタ」型 —
  大規模・複数リポジトリのプロジェクトではこちらが支配的。
- **確認できなかったもの**: standard-readme
  <https://github.com/RichardLitt/standard-readme/blob/main/spec.md> に
  相当する、CONTRIBUTING.md の節構成を MUST/SHOULD で規定した lint 可能な
  spec は、探した範囲(GitHub 公式・Web 全般)に存在しない。

## Decision

1. **CONTRIBUTING.md を固定スキーマにする。** `# Contributing` → 前文
   1 文(README への参照。環境構築/テストは README `## Development` が
   正本なので重複させない)→ `## Issues`(必須)→ `## Pull requests`
   (必須)→ `## Expectations`(任意、末尾)。見出しリテラルは英語で固定し、
   機械検査のアンカーにする(README と同じ規範)。この 2 節核は GitHub 公式
   ドキュメントの逐語 "Steps for creating good issues or pull requests" に
   直接対応する。`## Expectations` は opensource.guide の単独メンテナ
   言及("be honest about how much time you have")に対応する唯一の一次
   情報根拠であり、必須ではなく任意節にとどめる。
2. **「Issue litmus」という見出し名の自作語彙を廃止する。** 判定問 1 文 +
   採用例・棄却例という**内容の形**(ADR-0013 由来)は有用なので
   `## Issues` 節の冒頭に維持するが、見出し名としての "Issue litmus" は
   一次情報のどこにも先例がなく、GitHub 標準の "Issues" という語彙に
   単に併走するだけの独自命名だった。この内容形式自体(AI エージェントが
   コントリビュータになる状況での受け入れ判定)は、調査した一次情報の
   いずれにも言及がない空白地帯であり、ADR-0013/ADR-0016 由来の形式を
   語彙だけ変えて継承する。
3. **README/CONTRIBUTING の言語検査を非対称化する。** ADR-0016 Decision 4
   はこの 2 ファイルを「英語正本 1 本」と既に決めているので、判定は
   「両スクリプトが閾値を超えて混在しているか」ではなく「CJK が
   存在すること自体」を drift とする。閾値は 5 文字
   (`CJK_PRESENCE_THRESHOLD`)とし、旧来の対称混在ヒューリスティック
   (`LANG_MIX_THRESHOLD=20`、両スクリプトとも 20 文字以上で初めて drift)
   がすり抜けた今回の実害(日本語ラベル 9 文字)を確実に捕捉する。
4. **`github-audit` の charters ドメインを新スキーマに合わせて更新する。**
   `contributing-no-litmus` を廃止し、README と対称な
   `contributing-schema-incomplete-or-out-of-order` /
   `contributing-stray-heading:<names>` に置き換える(旧 `## Issue litmus`
   見出しの残存は、許可見出し集合に含まれないため自然に stray heading と
   して検出される — 専用チェックは不要)。`is_language_mixed` は
   `is_cjk_present` に置き換え、drift キーも `readme-cjk-present` /
   `contributing-cjk-present` に改名する(旧キー名はどのコンシューマにも
   参照されていないことを確認済み)。

## Alternatives considered

- **CONTRIBUTING.md を Atom 型(全文単一ファイル、Ground
  Rules/Community/Recognition まで含むフルセット)にする** — 単独メンテナ
  + AI エージェント環境では人間コミュニティ向け節(初心者導線・
  表彰・複数チャネル案内)が空集合になる。個人の全リポジトリへ横断適用
  するには過剰。棄却。
- **React/Kubernetes 型のポインタ(中央ガイドへの参照 1 行)にする** —
  GitHub の Issue/PR 作成画面の自動リンクは各リポジトリの
  CONTRIBUTING.md 自体を表示するため、ポインタ自体は各リポジトリに要る
  (GitHub Docs の配置規則)。中央ガイドの置き場所という新しい設計問題を
  増やすだけで、今回の対称スケールには見合わない。棄却(ただし将来
  リポジトリ数がさらに増えれば再検討の余地はある)。
- **「Issue litmus」語彙をそのまま維持する** — README 側は一次情報のいずれ
  にも先例のない語彙("In:"/"Out:" ラベル等)を ADR-0016 で退けたのに、
  CONTRIBUTING 側だけ同種の自作語彙を残すのは一貫しない。GitHub 標準語彙
  ("Issues") に揃える。棄却。
- **言語検査を対称のまま閾値だけ下げる(例: 両スクリプトとも 5 文字)** —
  自作見出しラベルのような「本文は英語、ラベルだけ日本語」というパターン
  では Latin 文字数は元々多い(閾値を余裕で超える)ため、対称のままでも
  検出はできる。しかし ADR-0016 Decision 4 の「英語正本 1 本」という要求は
  そもそも Latin との共起を前提にしておらず、意味論として非対称
  (CJK 存在それ自体が drift)の方が正確。採用。

## Consequences

- `scripts/github-audit` の charters ドメイン、`repo-charter` SKILL.md §3、
  dotfiles 自身の CONTRIBUTING.md が本 ADR の実装物。
- 本 ADR の時点で CONTRIBUTING.md を持つ全リポジトリは、新スキーマ
  (`## Issues`/`## Pull requests`)に未適合のため `drifted` と再報告
  される。これは ADR-0016 の Consequences が既に想定した「文書正典を
  変更するたびに全リポジトリが drifted に戻る」パターンの再発であり、
  監査自体の不具合ではない。適合化は各リポジトリを次に触るタイミングで
  順次行う(dotfiles 自身は本 ADR と同じ変更セットで適合済み)。
- 文書正典をさらに変更する場合は、この ADR を supersede する新しい ADR を
  起こす(ADR-0008 の規約)。

## Verification

- `github-audit --selftest` — 新スキーマの見出し集合・順序・任意節、
  旧 `## Issue litmus` 見出し残存の stray-heading 検出、CJK 非対称判定の
  各分岐を fixture で確認(`pj-ambiguous` フィクスチャが退役語彙の残存
  検出の回帰ケース)。
- `shellcheck -S error scripts/github-audit` が通る。
- dotfiles 自身の CONTRIBUTING.md を `github-audit` の内部関数
  (`doc_headings`/`is_cjk_present`)へ直接渡し、見出し集合が
  `Issues`/`Pull requests`/`Expectations` の順で揃い、CJK が存在しない
  ことを確認。
- `nix flake check` — 3 ホストとも green。
