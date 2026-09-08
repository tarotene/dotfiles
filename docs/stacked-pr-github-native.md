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
  dotfiles には `required_status_checks` が無く、ここには有る —
  ruleset のぶれの実例(別 Issue で追跡)。

## 再検証すべきこと

`docs/claude/pr-gate.md:383-387` の「stacked PR は ruleset 対象外」前提は、
GitHub Docs の「中段 PR にも branch protection と default-branch 向け CI が
適用される」という記述と噛み合わない可能性がある。`gh stack link` を実際に
使ったケースで実測し直す(別 Issue で追跡)。
