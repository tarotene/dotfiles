# ADR-792 — 無料プランの非公開リポジトリは、トピック `plan-free-private` の宣言で、成り立たない前提検査だけを外す

- Status: Accepted
- Date: 2026-10-14
- Issue: No-Issue(`/grill-me` セッション中に発見・裁定。無料プランの組織に作った非公開リポジトリの初回 PR で、`PR title` の required check だけが赤くなった。ユーザーの依頼は「無料プラン用の charter skill を起動するようにするのを恒久対応にして、workflow 側で automerge 設定の監視だけやめる。Team に切り替えるときの手順も明らかにする」)

## Context

PR タイトル契約の検査(`.github/actions/pr-title`、ADR-0031)と `github-audit` は、リポジトリが
GitHub のネイティブ自動マージと rulesets を使えることを暗黙に前提にしている。無料プランの組織に
作った非公開リポジトリで検査を実走したところ(2026-10-14)、次が成り立たなかった。

| 前提 | 無料・非公開での実態 | 根拠 |
|---|---|---|
| 自動マージが有効(ADR-568 D2、`judge_settings` の `auto-merge-disabled`) | 設定の送信が黙って無視され `allow_auto_merge` は `false` のまま。公式文書は非公開リポジトリの自動マージを Pro・Team・Enterprise に限る | `github/docs` の `data/reusables/gated-features/auto-merge.md`(取得 2026-10-14)。設定スクリプトが `allow_auto_merge: true` を送って `false` を読み戻した |
| 生きた branch ruleset を取得できる(`rulesets-context-check`、`judge_rulesets`、`judge_titles`) | `GET /repos/{o}/{r}/rulesets` と `/rules/branches/{b}` が 403「Upgrade to GitHub Pro or make this repository public」 | 実走(取得 2026-10-14) |

PR タイトルの文法の手順は成功しており、赤いのは前提検査の手順だった。検査を赤のまま残すと
本当の失敗が埋もれ、検査全体を外すとタイトルの文法や squash のみの設定まで緩む。成り立たない
前提だけを外し、有料プランに切り替えたら元に戻せる必要がある。

別件として、`workflow-naming-check` は `ci.yml` と集約 job `CI passed`(ADR-591)を要求する。
これはプランと無関係で、`core` の種まきが `ci.yml` を播かないことによる(範囲外。wrap-up に流す)。

## Decision

### D1: 前提の成否を、リポジトリのトピック `plan-free-private` 1 つで宣言する

意味は「このリポジトリは非公開で、所有者のプランが無料のため、自動マージと rulesets が使えない」。
REST の `GET /repos/{owner}/{repo}` が `.topics` と `.private` を返すので、CI(`GITHUB_TOKEN`)の
`pr-merge-settings-check` も `rulesets-context-check` も、人の token で動く `github-audit`
(bash 版・Rust 版)も、**追加の取得なしで同じ値を読む**。workflow ファイルには宣言を持たない。
宣言を外すだけで、検査が元に戻る。

### D2: 宣言があるとき、成り立たない前提だけを外す

| 検査 | 宣言ありのとき |
|---|---|
| settings(`judge_settings`、`pr-merge-settings-check`) | `auto-merge-disabled` を要求しない。squash のみ・タイトル・メッセージは引き続き検査する |
| `rulesets-context-check` | live の取得をしない。宣言ファイルの context が head で報告されていることだけ検査する |
| rulesets(`judge_rulesets`) | live の ruleset が無いことを drift にしない(`not-applicable`)。宣言ファイルは Team 切替用に播いたままにする |
| titles(`judge_titles`) | required に載っていないこと、live の context と run の job 名の突合を判定しない。`pr-title.yml` の有無は引き続き検査する |

### D3: 宣言が実態と食い違ったら `plan-free-private-stale` で必ず検出する

宣言があるのに、リポジトリが公開になった、または `allow_auto_merge` が `true` になったとき、
settings ドメインが報告する。宣言ありで live の default-branch ruleset が存在するときは、rulesets
ドメインが同じコードを報告する。宣言は非公開を含意するので、公開リポジトリに残ったトピックは
rulesets・titles では通常判定に戻る。Team に切り替えたのにトピックを外し忘れる、という手作業の
漏れを、監査で必ず見つける。

### D4: `create-repo.sh` が所有者のプランを検出して宣言を播く

`gh api /orgs/{owner}`(組織)または `/users/{owner}`(自分のユーザー)の `.plan.name` が `free` で、
リポジトリが非公開のとき、`plan-free-private` を `gh repo edit --add-topic` で播く。`--plan
auto|free|paid`(既定 `auto`)で上書きでき、検出できなかったときは注意を出して `--plan free` での
再実行(作成は飛ばされる)を案内する。`--dry-run` は gh を呼ばないので、検出しない。

### D5: Team 切替の手順を `docs/agents/free-plan-declaration.md` に書く

宣言を外す、設定を適用して自動マージを有効にする、rulesets を適用する、監査で `ok` を確かめる、
の順。workflow ファイルの編集は要らない。組織のプラン変更だけが人手で、画面の手順は GitHub Docs
「Upgrading your account's plan」に拠る。

## 先行例

- GitHub, Auto-merge(`github/docs` の `data/reusables/gated-features/auto-merge.md`)。「Auto-merge for pull requests is available in public repositories with GitHub Free and GitHub Free for organizations, and in public and private repositories with GitHub Pro, GitHub Team, GitHub Enterprise Cloud, and GitHub Enterprise Server.」(取得 2026-10-14)
- GitHub, About rulesets。非公開リポジトリの rulesets は GitHub Team 以上。https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/about-rulesets (取得 2026-10-14)
- GitHub, Upgrading your account's plan。https://docs.github.com/en/billing/how-tos/manage-plan-and-licenses/upgrade-plan (取得 2026-10-14)
- このリポジトリの `naming-*`・`lifecycle-*` トピック(ADR-0014、ADR-0026): リポジトリの性質をトピックで宣言し、`github-audit` が宣言の欠落・過剰を検出する。本決定は分類(何であるか)でなく環境の制約(何が使えないか)を宣言する点で異なり、使えるようになったら外す必要があるため、逆向きの検査(D3)を足した。
- docs/adr/568-renovate-automerge-shared-preset.md D2: 自動マージを前提とする根拠。

## 却下した案

- **呼び出し側 workflow の入力で外す案。** CI からは読めるが、`github-audit` は workflow を読まないので、宣言を二重に持つ(複写+同期)。プランを切り替えるたびに workflow の編集も要る。
- **実行時にプランを API で引く案。** `GET /orgs/{o}` の `plan` は組織の管理権限を持つ token でないと返らない見込みで、`GITHUB_TOKEN` では読めないと見ている(未確認)。読めるなら、トピックの代わりに実行時の検出も可能になる。
- **検査全体を無効にする案。** 感触で外した。タイトルの文法や squash のみの設定まで緩むのを避けたかった。分析ではない。

## 見つからなかったもの

- プランに依存する前提を検出・宣言する、既存の枠組み(`github-audit`、GitHub Docs の rulesets・auto-merge の節を「plan」「free」「gated」で探した)。
- 組織のプランを変更したあと、自動マージの設定が反映されるまでの時間。

## 執行点

この ADR が定義する決定を執行する、今回変更・追加した実体:

- `scripts/github-audit`
- `crates/github-audit/src/model.rs`
- `crates/github-audit/src/domains/settings.rs`
- `crates/github-audit/src/domains/rulesets.rs`
- `crates/github-audit/src/domains/titles.rs`
- `crates/github-audit/src/audit.rs`
- `scripts/pr-merge-settings-check`
- `scripts/rulesets-context-check`
- `config/agents/skills/repo-charter/scripts/create-repo.sh`
- `docs/agents/free-plan-declaration.md`
