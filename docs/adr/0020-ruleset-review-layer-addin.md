# ADR-0020 — ruleset baseline のコア層/レビュー層 2 層化

- Status: Accepted
- Date: 2026-09-19
- Issue: No-Issue(grill-me セッション中に発見・裁定)
- Amends: ADR-0015 の rulesets ドメイン(`github-audit` の
  `BASELINE_RULE_TYPES`)を部分修正する。ADR-0015 の他の Decision
  (統合 CLI 構成・LLM ノードの分離)はそのまま有効。

## Context

`github-audit` の rulesets ドメインは、全自作リポ共通の単一 baseline
(`deletion`/`non_fast_forward`/`required_signatures`/
`required_linear_history`/`required_status_checks`/`pull_request`/
`copilot_code_review` の 7 rule type、および `pull_request` の
`required_review_thread_resolution`/`allowed_merge_methods` パラメータ)を
要求していた。この中の「Copilot code review 自動リクエスト + 会話 resolve
必須」は、開発初期(未リリース)のリポジトリでは開発速度を落とし
ノイズになる、という問題意識がユーザーから出た。PR 必須・force-push
禁止・squash-only 等の基本保護は初期リポでも維持したい一方、レビュー
往復の強制だけは外せるようにしたい。

## 一次情報(取得日 2026-09-19)

- GitHub Docs "About rulesets"(rule layering 節)
  <https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/about-rulesets> —
  "if multiple rulesets target the same branch or tag in a repository, the
  rules in each of these rulesets are aggregated. If the same rule is
  defined in different ways across the aggregated rulesets, the most
  restrictive version of the rule applies." boolean パラメータ
  (`required_review_thread_resolution`)の衝突を名指しで解決する明文例は
  無いが、同ページの review-count の実例は同種のパラメータ衝突に
  most-restrictive が適用されることを実演しており、一般則からの自然な
  帰結として採用する。
- GitHub REST API "REST API endpoints for rules"
  <https://docs.github.com/en/rest/repos/rules> — `copilot_code_review`
  rule type のスキーマ。パラメータは `review_on_push` /
  `review_draft_pull_requests`(いずれも boolean)。
- GitHub Changelog "Copilot code review: Independent repository rule for
  automatic reviews"(2025-09-10 公開)
  <https://github.blog/changelog/2025-09-10-copilot-code-review-independent-repository-rule-for-automatic-reviews/> —
  `copilot_code_review` が単独 rule type として独立した経緯。
- CNCF Graduation Criteria
  <https://github.com/cncf/toc/blob/main/process/graduation_criteria.md> —
  プロジェクト成熟度を graduated/incubating/sandbox の宣言フェーズで
  管理する先行例(棄却案の比較対象、下記)。

## Decision

1. **baseline を 2 層に分ける。** コア層(常に必須)とレビュー層
   (アドイン、opt-in 検出)。コア層は次の 6 rule type:

   ```
   deletion, non_fast_forward, required_signatures, required_linear_history,
   required_status_checks, pull_request
   ```

   レビュー層は `copilot_code_review` rule type と
   `pull_request.required_review_thread_resolution` の 2 要素。

2. **レビュー層は宣言ではなく存在で判定する(アドイン方式)。** リポジトリの
   「開発フェイズ」を宣言させる台帳(topics・Release 有無等)は導入しない。
   union に `copilot_code_review` があるか、いずれかの `pull_request` rule
   の `required_review_thread_resolution` が true なら「レビュー層に
   opt-in 済み」とみなす。

3. **片方でもあれば完備を要求する。** 2 要素のうち一方だけが入っている
   状態は `review_layer=partial-drift` として drift 化する
   (`missing` に `review_layer.copilot_code_review` /
   `review_layer.required_review_thread_resolution` を積む)。中途半端な
   導入(例: Copilot は呼ぶが未 resolve のままマージできる)を許さない。

4. **未導入は情報行として報告し、drift にしない。** `review_layer=absent`
   として報告する。`required_status_checks` の check-name 収集
   (`scripts/github-audit` の既存実装)と同じ「収集するが裁かない」設計を
   踏襲する。

5. **適用側(`*-repo-governance` スキル)も 2 層に分ける。** レビュー層を
   独立ファイル `rulesets/review.json` に切り出し、`apply-rulesets.sh` の
   既定はコア 3 ファイル(Security/Quality/Workflow)のみ適用する。
   `--with-review` で明示的にレビュー層も適用、`--remove-review` で
   既存リポからレビュー層を剥離する。

## Alternatives considered

- **Release 有無で自動的にフェーズ判定する** — 一見自然だが、dotfiles
  自身が ADR-0004 で semver release しない方針であるため、この方式では
  dotfiles が永遠に「初期」扱いになる。判定シグナルとしてリポジトリ
  横断で一貫しない。棄却。
- **GitHub topics でフェーズを宣言する(CNCF 型)** — ADR-0014 の naming
  class と同様に `stage: incubating` 的な topic を権威記録にする案。
  CNCF は組織規模のガバナンスプロセス(TOC 投票・卒業レビュー)を前提に
  この宣言コストを正当化しているが、単独開発者規模ではフェーズの
  付与・更新・陳腐化管理のコストが便益に見合わない。新しい語彙(フェーズ
  分類)と ADR をもう一段必要とする点でも過剰。棄却。ただし将来
  複数人運用に移行する場合は再検討の余地がある。
- **既存の overrides.tsv による rulesets ドメイン丸ごと exempt** —
  既存の逃げ道だが、粒度が粗く「レビュー層だけ外す」ができない。
  `deletion`/`non_fast_forward` 等の基本保護まで一緒に免除されてしまう。
  棄却(2 層化後も overrides.tsv 自体は他の用途で存続する)。

## Consequences

- `scripts/github-audit` の `BASELINE_RULE_TYPES` からは `copilot_code_review`
  を外し、`judge_rulesets()` がレビュー層を別枠で判定する。
  `docs/github-audit.md` の baseline 節を本 ADR にリンクする形へ更新する。
- `rust-repo-governance` / `typst-repo-governance` / `astro-site-governance`
  の 3 スキルに `rulesets/review.json` を新設し、`workflow.json` から
  `copilot_code_review` rule と thread resolution 要求を外す。
- 本 ADR の時点でレビュー層を(旧 baseline 経由で)持つ既存リポジトリは、
  再監査で `review_layer=complete` と報告され続ける — 剥離するかどうかは
  リポジトリごとの人間判断(`github-audit-triage`)。
- baseline をさらに変更する場合は、この ADR を supersede する新しい ADR を
  起こす(ADR-0008 の規約)。

## Verification

- `github-audit --selftest` — レビュー層 3 態(absent/complete/
  partial-drift)を fixture で確認。
- `shellcheck -S error scripts/github-audit` および 3 スキルの
  `apply-rulesets.sh` が通る。
- 実アカウントに対する read-only 監査実行で、レビュー層を持つ/持たない
  既存リポジトリの双方が期待通りの verdict/review_layer になることを
  確認する。
