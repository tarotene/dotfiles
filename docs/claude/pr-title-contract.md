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
| required check | PR の現在のタイトル + merge 設定の前提(下記「前提設定の自己防衛」) | CI red(squash merge をブロック) | `pr-title.yml` |
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
連結の曖昧さがない。checkout する `scripts/pr-title-check` の ref は
`github.repository == 'tarotene/dotfiles'` の場合だけ `main` ではなく
自身の head SHA にする(checker 自体を変更する自己適用 PR が「main に
まだ乗っていない」ために fail する鶏卵問題の実測込みの回避、Stage 3 で
実機確認)。

`workflow_call` 経由で他リポジトリが呼ぶ場合、GitHub の仕様上 context 名は
`<呼び出し側 **job** の name> / <呼び出される job の name>` の連結になる
(telepath#243 の Actions API 実測、取得 2026-09-26。GitHub Community
Discussion #46752 <https://github.com/orgs/community/discussions/46752>
は「呼び出し側 workflow の name」説を裏付ける記述を持たず、discussion
自体も未解決 — この説は誤読で、ADR-0031 D2 追補が誤って採用し、#337 の
播きで required check が恒久的に「Expected」のまま merge を止める事故を
起こした。ADR-0031 の 2026-09-26 Amendment 参照)。呼び出し workflow
テンプレート(`config/claude/skills/repo-governance-common/templates/
.github/workflows/pr-title.yml`、rust/typst/astro-site の 3 skill は
symlink で共有)は `check` job に `name: PR Title` を固定するため、連結名は
常に `"PR Title / PR title"` になる。「最初の利用リポジトリへの展開時に
実機で確認する」という手動手順は、`.github/workflows/pr-title.yml`
(reusable workflow)の最終 step `scripts/pr-title-context-check` による
実行時の自己照合に置き換えた — run のたびに自分自身の check 名を Actions
API から実測し、有効な branch ruleset の required_status_checks と比較
して不一致なら CI を red にする。

## revert の扱い

GitHub UI の revert ボタンが作る `Revert "..."` というタイトルは文法に
非適合であり、CI は red になる。特例として通す実装はしない — `revert:`
type に改題してから merge する運用とする(例:
`revert: 署名検証の早期 return を戻す`)。

## titles ドメインが見るもの / 見ないもの

- 見る: (a) `pr-title.yml` を呼び出す workflow がリポジトリに存在するか、
  (b) その required check context(`PR title` または `PR Title / PR
  title` の完全一致)が ruleset に登録されているか、(c) pr-title.yml の
  最新 run が実際に報告した job 名と (b) の context が一致するか
  (`pr-title-context-mismatch`、ADR-0031 2026-09-26 Amendment — #337 の
  事故で (b) だけでは検出できなかった盲点を埋める)。
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

## 前提設定の自己防衛

タイトル適合の判定は「squash merge した結果、PR タイトルがそのまま
`main` の commit subject になる」という merge 設定(`squash_merge_
commit_title=PR_TITLE` / `squash_merge_commit_message=BLANK` / squash-only)
の上に乗った proxy であり、この設定自体が drift すると契約全体が無声で
バイパスされる。`github-audit settings` ドメインはこの drift を検出できる
が、監査は手動実行のため「検出」であって「防止」ではない — 実際に
PRIVATE リポジトリ 2 件(実名は ADR-0034 により省略)が drift し、うち
1 リポは直近 25 PR すべてで契約が無声にバイパスされていた実例がある
(ADR-0031 Amendment 参照)。

これを required check(`pr-title.yml`)に同居させ、drift した状態での
merge 自体を CI red で止める。判定ロジックは新設せず、`scripts/
pr-merge-settings-check` が `scripts/github-audit` の `judge_settings()`
を re-source して再利用する(判定の単一正本は `github-audit` 側のまま)。
API 取得に失敗した場合は fail-closed(red)にする — client guard
(`pr-title-guard.sh`)の fail-open とは逆で、契約の前提が確認できない
まま merge を通さないことを優先する。

## スコープ外(意図的)

- Issue タイトルの書式強制 — 「変更」ではなく「世界の状態」を記述する
  別ジャンルであり、squash commit と無関係(ADR-0031 D6)。
- ブランチ上の commit メッセージの書式強制 — squash で `main` の履歴
  からは破棄される(同上)。実測(2026-09-23/24、squash 運用 4 リポの
  直近 25 PR・ブランチ commit 計 109 件)では非適合 1 件のみ(適合率
  99.1%)で、その 1 件も上記の merge 設定 drift を経由してしか
  `main` に漏れなかった(ADR-0031 Amendment 参照)。
- 他 19 リポジトリへの播き・merged PR のバックフィル — 本 ADR で方針は
  確定するが、実施は sub-issue へ切り出し後続セッションが行う
  (ADR-0031 Consequences)。
