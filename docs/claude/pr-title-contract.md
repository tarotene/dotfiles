# pr-title-contract — PR タイトルを commit-message 契約として機械強制する

設計判断の記録: `docs/adr/0031-pr-title-as-commit-message-contract.md`
checker(単一ソース): `scripts/pr-title-check`
client guard: `config/claude/hooks/pr-title-guard.sh`
サーバ側 required check: `.github/workflows/pr-title.yml`(reusable workflow)
audit: `scripts/github-audit` の `settings` ドメイン拡張 + 新設 `titles` ドメイン
Issue: tarotene/dotfiles#325

squash-only 運用(`allow_squash_merge=true`、`squash_merge_commit_title=
PR_TITLE`)では PR タイトルがそのまま `main` の commit subject になる。
non-conventional な commit が `main` に混入する経路をこの 1 点に絞り、
client 側の作成時 deny・サーバ側の CI red・監査側の drift 検出という
三層で機械強制する。

## 三層構造

| 層 | 何を見るか | 強制の形 | 実装 |
|---|---|---|---|
| client guard | ローカルで打つ `gh pr create`/`gh pr edit --title` | PreToolUse deny(作成前に止める) | `pr-title-guard.sh` |
| required check | PR の現在のタイトル | CI red(squash merge をブロック) | `pr-title.yml` |
| audit | 「仕組みが存在するか」(呼び出し workflow・required check context) | drift 報告(`github-audit titles`) | `github-audit` |

3 層とも `scripts/pr-title-check` の 1 つの正規表現を最終的な判定根拠に
する。文法を変えるときはこのファイルだけを直せばよい。

## 文法

```
type(scope)?!?: subject
```

- `type`: `feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert`
- `scope`: 省略可。`[a-z0-9._/-]+` のカンマ区切り(例: `(alacritty,herdr)`)
- `!`: breaking change 標識、省略可
- `subject`: 非空、言語自由、長さ無制限、末尾 `.`/`。` 禁止

根拠と実測データは ADR-0031 の D1 を参照。

## client guard の範囲

`tarotene/*` のリポジトリでのみ発火する(owner を `--repo`/`-R` または
`git remote get-url origin` から解決。解決不能なら fail-open)。
`home/modules/claude.nix` の hooks は common 層で会社ホストにも配備される
ため、会社リポの別慣行と衝突しないようスコープする(ADR-0031 D4)。

一時的に無効化したいときは `PR_TITLE_GUARD_ALLOW=1` を立てる(`No-Issue:`
のような本文タグ型 escape hatch ではない — 理由を本文に残す恒久的な決定
ではなく、単発の緊急対応向けの一時解除のため)。

## required check の context 名

dotfiles 自身は `pr-title.yml` に直接 `pull_request` トリガーを持たせて
自己適用する — job 名がそのまま required check context `PR title` になり、
連結の曖昧さがない。

`workflow_call` 経由で他リポジトリが呼ぶ場合、GitHub の仕様上 context 名は
`<呼び出し側 workflow の name> / <呼び出される job の name>` の連結になる
(GitHub Community Discussion #46752, 取得 2026-09-22:
<https://github.com/orgs/community/discussions/46752>)。Stage 5/6 の
呼び出し workflow テンプレートは各利用リポジトリの `workflow` レベル
`name:` を固定した上でこの連結形を required check として登録する。実際の
文字列は最初の利用リポジトリへの展開時に実機で確認する。

## revert の扱い

GitHub UI の revert ボタンが作る `Revert "..."` というタイトルは文法に
非適合であり、CI は red になる。特例として通す実装はしない — `revert:`
type に改題してから merge する運用とする(例:
`revert: 署名検証の早期 return を戻す`)。

## titles ドメインが見るもの / 見ないもの

- 見る: (a) `pr-title.yml` を呼び出す workflow がリポジトリに存在するか、
  (b) その required check context(`PR title`)が ruleset に登録されて
  いるか。
- 見ない: open PR 個々のタイトルの適合。CI(required check)と client
  guard がその役割を担う。audit まで実測すると三層の判定が重複し、
  「audit は drift と言うが CI は green」のような不整合が起きうる。
- CI を持たないリポジトリ(workflows なし)は `not-applicable`
  (`renovate` ドメインと同じ扱い、ADR-0020 の grandfathering 型)。

## settings ドメインの拡張

`squashMergeCommitTitle == "PR_TITLE"` / `squashMergeCommitMessage ==
"BLANK"` を検査する(missing token: `squash-title-not-pr-title` /
`squash-message-not-blank`)。適用側は
`config/claude/skills/*-repo-governance/scripts/apply-repo-settings.sh`
に既に同じ値がある — audit 側に監査項目を追加するだけで、新しい宣言値を
発明しない。

## スコープ外(意図的)

- Issue タイトルの書式強制 — 「変更」ではなく「世界の状態」を記述する
  別ジャンルであり、squash commit と無関係(ADR-0031 D6)。
- ブランチ上の commit メッセージの書式強制 — squash で `main` の履歴
  からは破棄される(同上)。
- 他 19 リポジトリへの播き・merged PR のバックフィル — 本 ADR で方針は
  確定するが、実施は sub-issue へ切り出し後続セッションが行う
  (ADR-0031 Consequences)。
