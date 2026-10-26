# ADR-805 — 日本語が正本のリポジトリは、トピック `lang-ja` の宣言で、README/CONTRIBUTING の CJK 検査だけを外す

- Status: Accepted
- Date: 2026-10-16
- Issue: No-Issue(`/grill-me` セッション中に発見・裁定。日本語話者だけが読むリポジトリに charter を適用したところ、`readme-cjk-present` と `contributing-cjk-present` が出た。ユーザーの裁定は「日本語 README は正本として認める。dotfiles 側で宣言を足す」)
- Amends: ADR-0016 Decision 4、ADR-0017 Decision 3

## Context

ADR-0016 Decision 4 は、README と CONTRIBUTING を全リポジトリで英語の原文 1 本と定め、ADR-0017
Decision 3 は、CJK が 5 文字以上あることをそれだけで drift とした。根拠は、個人アカウントの規模で
全リポジトリの翻訳を保守する主体がいない、という運用上の事実だった(ADR-0016 の Alternatives)。

この前提が成り立たないリポジトリがある。読者が日本語話者に限られ(団体の運営など)、英語の原文を
読める人が実質的にいない場合である。英語に直しても、読者はそれを読めない。日本語を別ファイルに
分けると、読者が読む文書と監査される文書が別になり、二重管理になる。検査を受け入れ続けると、同じ
2 件の drift が毎回出て、他の drift が埋もれる。

CJK 検査以外の charter の検査は、このリポジトリでも成り立つ。見出しのリテラル(英語)、節の順序、
目的文と description の一致、導線文書に書かないもの(ADR-0033)、ルート文書の allowlist である。外すのは
CJK 検査だけでよい。

## Decision

### D1: 正本言語が日本語であることを、リポジトリのトピック `lang-ja` 1 つで宣言する

意味は「README.md と CONTRIBUTING.md の正本言語は日本語である」。`github-audit` は、宣言があるとき
`readme-cjk-present` と `contributing-cjk-present` を出さない。見出しのリテラル、節の順序、目的文の一致、
nav-doc の検査は、宣言があっても変わらない。

トピックは `repositoryTopics`(GraphQL)で、`naming-*`・`lifecycle-*`・`plan-free-private` と同じ読み方をする。
workflow ファイルには宣言を持たない。宣言を外すだけで検査が元に戻る。

### D2: 宣言が実態と食い違ったら `lang-ja-stale` で必ず検出する

宣言があるのに、README に CJK が閾値(5 文字)未満しかないとき、charters ドメインが `lang-ja-stale` を
報告する。英語に書き直したのにトピックを外し忘れる、という手作業の漏れを、監査で必ず見つける。
CONTRIBUTING.md は stale の判定に使わない(短く、CJK が無くても不自然でないため)。

### D3: 検査を外す範囲は CJK だけ。per-repo CI の言語混在検査には触れない

`check-language-mixing.sh`(per-repo CI)の対象は `LANG_MIX_PATHS` の宣言で絞る(ADR-0016 の
2026-10-01 の Amendment)。本決定はこの機構を変えない。

### D4: `repo-charter` スキルに、日本語が正本の場合の手順を足す

charter インタビューの時点で、README の正本言語を日本語にするかを確かめ、そうなら `lang-ja` を付ける
(`gh repo edit --add-topic lang-ja`)。

## 先行例

- このリポジトリの ADR-792(無料プランの宣言)。環境や性質をトピック 1 つで宣言し、`-stale` で嘘を検出する型。本決定は同じ型を charters ドメインの CJK 検査に使う。
- ADR-0016 Decision 4 の根拠(W3C Authorized Translations、Kubernetes・MDN のローカライズ方針)。これらは翻訳を保守する主体がいる大規模プロジェクトの方針で、読者が一つの言語に限られる小さな団体のリポジトリには当てはまらない。
- ADR-0014・ADR-0026 の `naming-*`・`lifecycle-*` トピック。性質の宣言を監査が検出する型。

## 却下した案

- **README/CONTRIBUTING を英語にして、日本語を README.ja.md に分ける案。** ADR-0016 が既に許している形だが、読者が読む文書と監査される文書が別になる。英語の原文との複写+同期が生じる。
- **検査を受け入れ続ける案。** 感触で外した(他の drift が埋もれる感触)。分析ではない。
- **CJK の閾値を上げる案。** 日本語の文書は閾値をどこに置いても超えるので、検査が実質的に無効になり、宣言のような明示的な表明が残らない。

## 見つからなかったもの

- 日本語話者だけが読む小規模リポジトリの README の言語を、機械検査と両立させる既存の枠組み。この ADR は、ADR-0016 の根拠(翻訳の保守主体が個人規模にいないこと)が当てはまらない場合に限る例外を、宣言として足すだけで、追加の文献調査はしていない。

## 執行点

この ADR が定義する決定を執行する、今回変更・追加した実体:

- `scripts/github-audit`
- `crates/github-audit/src/domains/charters.rs`
- `crates/github-audit/src/audit.rs`
- `crates/github-audit/tests/judges.rs`
- `docs/agents/lang-ja-declaration.md`
- `config/agents/skills/repo-charter/SKILL.md`
