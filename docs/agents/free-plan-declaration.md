# free-plan-declaration — 無料プランの非公開リポジトリを宣言 1 つで前提検査から除外する

設計判断の記録: `docs/adr/792-free-plan-declaration.md`
宣言: リポジトリのトピック `plan-free-private`
参照側: `crates/github-audit`(settings・rulesets・titles ドメイン)、
`crates/pr-contract-check`(`pr-merge-settings-check`、`rulesets-context-check`)
付与側: `config/agents/skills/repo-charter/scripts/create-repo.sh`(`--plan`)

## 何のための宣言か

PR タイトル契約の検査(`.github/actions/pr-title`)と `github-audit` は、リポジトリが
自動マージと rulesets を使えることを暗黙の前提としている。無料プランの組織(または
ユーザー)の**非公開**リポジトリでは、どちらも使えない(2026-10-14 に実測と原文で確認)。

| 前提 | 無料・非公開での実態 | 根拠 |
|---|---|---|
| 自動マージが有効 | 設定の送信が黙って無視され `allow_auto_merge` は `false` のまま。公式文書は非公開リポジトリの自動マージを Pro・Team・Enterprise に限る | `github/docs` の `data/reusables/gated-features/auto-merge.md`(取得 2026-10-14) |
| live の branch ruleset を取得できる | `GET /repos/{owner}/{repo}/rulesets` と `/rules/branches/{branch}` が 403「Upgrade to GitHub Pro or make this repository public」 | 実測(取得 2026-10-14) |

前提が成り立たない環境で検査を失敗のまま残すと、本当の失敗が埋もれる。かといって検査全体を
無効にすると、タイトルの文法や squash のみの設定まで緩んでしまう。そこで、**成り立たない前提だけ**を
宣言 1 つで除外する。

## 宣言があると何が変わるか

| 検査 | 宣言ありのとき |
|---|---|
| `pr-merge-settings-check`・`github-audit` の settings | `auto-merge-disabled` を報告しない。squash のみ・タイトル・メッセージの設定は引き続き検査する |
| `rulesets-context-check` | live の branch ruleset を取得しない。宣言ファイル(`.github/rulesets/quality.json`)の context が head で報告されていることだけ検査する |
| `github-audit` の rulesets | live の ruleset がないことを drift にしない(`not-applicable`)。宣言ファイルは Team 切替用に配置したままにする |
| `github-audit` の titles | 「PR title が required に載っていない」「live の context と run の job 名の突合」を判定しない。`pr-title.yml` の存在は引き続き検査する |

## 宣言が嘘になったことを検出する(`plan-free-private-stale`)

宣言がある状態でリポジトリが**公開**になったり**自動マージが有効**になったりすると、
settings ドメインが `plan-free-private-stale` を報告する(rulesets ドメインは、宣言ありの状態で
live の ruleset が存在するときに同じコードを報告する)。Team に切り替えたのにトピックを
外し忘れる手作業の漏れを監査で必ず検出する。宣言は非公開を含意するため、公開
リポジトリに残った場合は通常判定に戻る。

## 宣言の付け方

- **新規作成:** `create-repo.sh` が所有者のプランを `gh api /orgs/{owner}`(組織)または
  `/users/{owner}`(ユーザー、自分のときだけ見える)の `.plan.name` で検出し、非公開かつ
  `free` のとき `plan-free-private` を付与する。検出できないときは警告を出す。無料プランの
  非公開リポジトリであれば `--plan free` を付けて同じコマンドを再実行する(作成処理はスキップされる)。
- **既存のリポジトリ:** `gh repo edit <owner>/<repo> --add-topic plan-free-private`。

## 無料プランの非公開リポジトリで守ること

- Renovate の自動マージ(ADR-568 D2、GitHub ネイティブの自動マージに依存)を有効にしない。
- required check は強制されない(rulesets がない)。マージ前に CI の結果を人が確認する。
- `.github/rulesets/*.json` は削除しない。Team に切り替えたときにそのまま適用する。

## Team に切り替えるとき(宣言を外して、検査を元に戻す)

workflow ファイルの編集は不要である。宣言(トピック)を外せば、検査が自動的に元に戻る。

1. **人手:** 組織のプランを Team に変更する。GitHub の右上のプロフィール画像から Settings、
   Access 節の Organizations で組織を選び、組織の Settings の Access 節の
   「Billing and licensing」で「Upgrade to Team」を押す。月払いか年払いを選び、支払い
   方法を入力して「Upgrade my organization's account」を押す。同じ画面の
   「Current plan」が Team になっていることで完了を確認する。
   根拠: GitHub Docs「Upgrading your account's plan」
   (https://docs.github.com/en/billing/how-tos/manage-plan-and-licenses/upgrade-plan 、
   取得 2026-10-14)。画面の表記は取得時の文書の記述であり、変わっていれば実行時に確認する。
2. 宣言を外す。
   ```bash
   gh repo edit <owner>/<repo> --remove-topic plan-free-private
   ```
3. 設定を適用して自動マージを有効にし、設定値を読み戻す。
   ```bash
   ~/.claude/skills/repo-governance-common/scripts/apply-repo-settings.sh --owner <owner> --repo <repo>
   gh api /repos/<owner>/<repo> --jq .allow_auto_merge   # true になること
   ```
   プランの変更が反映されるまでに時間がかかる場合は、時間を置いて再実行する(反映までの
   時間は未確認)。
4. rulesets を適用する。宣言(`.github/rulesets/*.json`)は default branch から読まれる。
   まず `--unverified-contexts` で作成し、CI が一度走ったあとに `--reconcile` で context を
   検証して更新する。
   ```bash
   apply-rulesets.sh <owner>/<repo> --unverified-contexts
   apply-rulesets.sh <owner>/<repo> --reconcile
   ```
5. 監査で確認する。settings と rulesets と titles が `ok` を返し、`plan-free-private-stale`
   が出ないこと。`github-audit` は `GITHUB_AUDIT_OWNER`(既定は個人アカウント)が所有する
   リポジトリを見るので、組織のリポジトリでは組織名を渡す。
   ```bash
   GITHUB_AUDIT_OWNER=<owner> github-audit settings rulesets titles --json | jq '.[] | select(.repo=="<repo>")'
   ```

## Team を解約して無料に戻すとき

宣言を付け直す(`gh repo edit <owner>/<repo> --add-topic plan-free-private`)。付け忘れると、
settings ドメインが `auto-merge-disabled` を報告するので、監査で必ず検出できる。live の
ruleset が残っていると rulesets ドメインが `plan-free-private-stale` を報告するので、
解約後に消えていることを確認する。

## 未確認

- 組織のプランを変えたあと、自動マージの設定が反映されるまでの時間。
- `GITHUB_TOKEN` で `GET /orgs/{owner}` の `plan` が読み取れるか。読み取れるなら、トピックの代わりに
  実行時の検出も可能になる。現状は、`github-audit` を人間の token で実行する運用と、REST で
  読み取れるトピックによる宣言で足りている。
