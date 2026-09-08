# GitHub ネイティブ Stacked PR 機能の実態(調査記録)

ADR-0008 のルール 1 に従い、時間で腐る事実をここに隔離する。
`docs/claude/stacked-pr.md` の裁定(素の `--base` + `gh stack link`、
`init/submit/sync` は使わない)はここにリンクするだけで、本文には埋め込まない。
public preview が GA したときは、このファイルだけを差し替えればよい。

すべて **2026-09-09 時点**の一次確認。

## ステータス

- [Stacked pull requests are now in public preview — GitHub Changelog](https://github.blog/changelog/2026-07-30-stacked-pull-requests-are-now-in-public-preview/)
  (2026-07-30、取得 2026-09-09)。GA 告知は 2026-09-09 時点で確認できず。
- [github/roadmap#1218 — Pull request stacks [Public Preview]](https://github.com/github/roadmap/issues/1218)
  (作成 2026-01-29、更新 2026-09-03、`Shipped` ラベル、取得 2026-09-09)。
- [About stacked pull requests — GitHub Docs](https://docs.github.com/en/pull-requests/get-started/about-stacked-prs)
  (取得 2026-09-09): 「同一リポジトリの単一の線形チェーンのみ。cross-fork
  不可。GitHub Desktop 非対応。」中段 PR にも branch protection と
  default-branch 向け CI が適用される。

## CLI / API サーフェス

- 公式 CLI は本体ではなく拡張 [github/gh-stack](https://github.com/github/gh-stack)。
  当機に `v0.1.0` が既にインストール済み(最新は `v0.1.1` / 2026-09-02、
  取得 2026-09-09)。`gh` 本体は `2.99.0`。
- REST `/repos/{owner}/{repo}/stacks` は preview ヘッダ不要で 200 を返すことを
  実測(2026-09-09)。
- GraphQL は `PullRequest.stack` / `.stackEntry` が読めるが
  **mutation は 0 件(read-only)** —
  [Stacked pull requests APIs and webhooks](https://docs.github.com/en/pull-requests/reference/stacked-pull-requests-rest-and-graphql-apis)
  (取得 2026-09-09)。
- サーバサイド rebase のコミットは署名されない。署名必須リポでは GitHub CLI
  から rebase せよと公式が明記 —
  [Managing stacked pull requests](https://docs.github.com/en/pull-requests/how-tos/create-pull-requests/managing-stacked-pull-requests)
  (取得 2026-09-09)。

## `gh-stack` 拡張の既知の open issue(実害級、計 120 件 / 2026-09-09 時点)

- [#489](https://github.com/github/gh-stack/issues/489) — `gh stack init`
  が既存ブランチを trunk から再作成し、submit の force-push で履歴を破壊して
  PR を閉じる。
- [#354](https://github.com/github/gh-stack/issues/354) — `gh stack sync`
  が常に rebase し、レビュー履歴(changes since last view)を壊す。GitHub
  側が `topic: rebase alternative` ラベルを付けて open のまま。
- [#319](https://github.com/github/gh-stack/issues/319) —
  stale/missing `refs/pull/N/merge` で `pull_request` workflow が
  静かに走らなくなる(silent-green リスク)。

## このリポジトリ・関連リポジトリでの実測

- `tarotene/dotfiles`: squash-merge のみ許可
  (`allow_merge_commit: false`, `allow_rebase_merge: false`)、
  `delete_branch_on_merge: true`。ruleset `Ephemeral Initial` は
  `~DEFAULT_BRANCH` スコープで `deletion` / `non_fast_forward` /
  `pull_request`(承認 0)のみ。署名必須ルールは無い。
- `arkedge/sbir-aocs-report`: squash-merge のみ許可。
  `main-protection` ruleset は `deletion` / `non_fast_forward` /
  `pull_request` / `required_status_checks`。署名必須ルールは無い。
  dotfiles には `required_status_checks` が無く、ここには有る、という
  ruleset のぶれが 2026-09-09 まで存在した(#129)。同日、dotfiles の
  `Ephemeral Initial` にも `required_status_checks`(6 context)を追加して
  解消した。ぶれの棚卸し・再発防止の仕組みは別 Issue(#130)で追跡する。

## 再検証すべきこと(再検証済み、2026-09-09)

`docs/claude/pr-gate.md:383-387` の「stacked PR は ruleset 対象外」前提を、
`gh stack link` を実際に使った stack(#124)の merged PR で実測した:

| PR | base | 報告された check(計 6 件) |
|---|---|---|
| [#126](https://github.com/tarotene/dotfiles/pull/126) | `docs/adr-0008-documentation-artifact-selection`(非 default) | 全件 pass |
| [#127](https://github.com/tarotene/dotfiles/pull/127) | `feat/rebase-update-refs`(非 default) | 全件 pass |

GitHub Docs の「中段 PR にも default-branch 向け CI が適用される」は
矛盾していなかった — それは **CI が実行される**ことを述べており、
`pr-gate.md` の前提は **ruleset が適用される**かどうかを述べている。
両 workflow の `pull_request:` トリガーにブランチフィルタが無い
(`.github/workflows/ci.yml:9-13`, `.github/workflows/nix.yml:7-10`)ため、
CI は base を問わず走る。一方 ruleset `Ephemeral Initial` の条件は
`~DEFAULT_BRANCH` のままなので、非 default branch 向け PR には
`required_status_checks` が適用されない(`gh api rules/branches/<非 main>`
は `[]` を返す、2026-09-09 実測)。つまり前提は正しく、「対象外」と
「報告されない」を区別せず読める書き方だっただけだった。
