# ADR-503 — required status check の正本を対象リポジトリ自身の宣言に一本化する

- Status: Accepted
- Date: 2026-09-26
- Issue: No-Issue(`/grill-me` セッション中に発見・裁定)

## Context

tarotene の private リポジトリの 1 つで、PR が実行されない workflow の
required check で恒久的に BLOCKED になっていることが `/grill-me`
セッション中に見つかった。Quality ruleset の `required_status_checks` が
`Format check`/`Host (clippy + test + smoke)`/`MSRV (1.88)`/
`Firmware (cross-compile nRF52840-DK)`/`Tools ( CLI clippy + tests)` を
要求していたが、当該リポジトリの CI は `test` という単一ジョブしか持たず、
これらの context を一度も報告しない。

原因は `github-rulesets-apply rust <owner>/<repo>` が
`rust-repo-governance` skill の `rulesets/quality.json`(参照実装
telepath の job 名を直書きしたテンプレート)をそのまま対象リポジトリへ
PUT していたこと。共有コア(`_rulesets-apply-core.sh`)は PUT/POST の
前に「context は対象リポジトリの workflow job 名と一致させてください」と
印字するだけで、実際に一致するかは一切検査していなかった。required
context の正本(テンプレート)が対象リポジトリの**外**にあり、対象
リポジトリの CI workflow(リポジトリの**内**)と同じ PR で編集される
保証が構造的に無かった。

横断調査(2026-09-26、21 リポジトリ)で同型の drift が他に、public な
telepath 自身を含む複数のリポジトリで見つかった。telepath のケースは
`--cli-crate` を空文字列で置換した結果 context が
`Tools ( CLI clippy + tests)`(ダブルスペース)になっていた——プレース
ホルダの置換漏れという別の事故クラスも同じ構造的欠陥から生じている。

さらに、required context を追加するタイミングと、対象リポジトリの
既存 open PR の head で新しい workflow が実際に走っているタイミングが
ズレる時間差クラスの事故(tarotene/bleep#32)も同一の到達範囲として
扱う——required 化した瞬間に開いていた PR は、rebase して再度 CI を
走らせない限り同じ理由で BLOCKED になる。

## Decision

required_status_checks の正本を、対象リポジトリ自身の
`.github/rulesets/{security,quality,workflow}[,review].json` に一本化する
(D1)。apply はどのリポジトリに対しても同じ汎用スクリプトで済ませ
(D4「還元」)、PUT/POST 直前に宣言の context を「検証対象コミットが
実際に報告する job 名」(Actions API 実測、YAML は静的パースしない — D3)
と突合し、報告不能な context があれば拒否する(D5)。検証対象コミットは
既定で対象 ref に対する最新 PR の head SHA(D14:
default branch の squash commit 自体には `pull_request` トリガーの run が
存在しないため)。

毎 PR の required check(既存の reusable `pr-title.yml` の最終 step、
D2)で、呼び出し元リポジトリの宣言と live な branch ruleset の両方を、
その PR の head が実際に報告する job 名と突合する——新しい required
context を作らないことで、bleep#32 型の時間差事故を再発させない。

`gh api` による ruleset の直接書換は `crates/rulesets-write-guard`
(Rust、D7)が deny する——ADR-0024 は新規 hook を既定で Rust とし、
bash 例外は「既存の巨大 bash 資産(`scripts/github-audit`)を source して
判定ロジックを再利用する」ときに限る。この guard は `gh` コマンド
文字列の解析だけで完結し、その例外に該当しない。

`github-audit` の rulesets ドメインに、宣言の欠落・drift・報告不能
context を機械判定する 3 種の finding を追加する(D8)。GitHub App による
中央 reconciler(宣言 → live の自動反映)は将来拡張として設計だけ残し、
今回は実装しない(D9:個人アカウントには account-level secret が無く、
各リポジトリが merge 時に自己 apply する形は App の秘密鍵を全リポジトリに
複製することになる。手動 apply + audit 検出という、より安い既存の手段が
同じ不変条件を保てる)。

先行例・各判断の対比は本 PR のスタック内(段1〜3)の各 PR 本文と、
セッションの `## 先行例との対比` 節に記録する
(precedent-grounding/selection-grounding スキル)。

## Consequences

- required context の正本と、それを満たす CI workflow が同じリポジトリ・
  同じ PR で編集可能になる——テンプレート側の変更が対象リポジトリの
  required check に無断で影響しなくなる。
- 型引数(rust/typst/astro/core/dotfiles)を持つ 4 層の apply スクリプトが
  1 本に統合され、型の手渡し誤りという事故の直接経路が消える。
- `gh api` での ruleset 直接書換が既定で deny されるため、宣言を経ない
  変更は明示的な bypass を要する(`RULESETS_WRITE_GUARD_BYPASS=1`)。
- 宣言 → live の反映は依然手動(`apply-rulesets.sh --reconcile`)——
  audit は drift を検出するが、自動修復はしない(将来拡張で対応)。
- 既存の `rulesets-declaration-missing` を持つリポジトリ(dotfiles 自身の
  播き先のうち未移行の 11 リポジトリ)が新たに audit の drifted 対象として
  可視化される——#337 の播き(github-audit-triage)で順次解消する。

## 執行点

- `scripts/apply-rulesets.sh` — 汎用 apply(宣言読み取り + context 検証 +
  PUT/POST、`--from-dir`/`--verify-sha`/`--reconcile`/
  `--unverified-contexts`/`--delete-ruleset`)
- `scripts/github-rulesets-apply` — 型引数を持たない複数リポジトリループ
- `scripts/rulesets-context-check` — 毎 PR の required check 本体
  (`.github/workflows/pr-title.yml` の最終 step から呼ばれる)
- `scripts/github-audit` — `fetch_head_sha_job_names`/
  `fetch_latest_pr_head_job_names`、`judge_rulesets` の
  declaration-missing/drift/unreportable-context 判定
- `crates/rulesets-write-guard/` — `gh api` ruleset 書込みの PreToolUse deny
- `.github/workflows/pr-title.yml` — `rulesets-context-check` の呼び出し
- `.github/rulesets/security.json` — dotfiles 自身の宣言
  (`quality.json`/`workflow.json` も同ディレクトリに同居)
- `config/agents/skills/rust-repo-governance/scripts/copy-files.sh` —
  宣言の播種 + placeholder 検証 + Firmware context の自動除去
- `config/agents/skills/repo-governance-common/scripts/copy-files.sh` —
  `core` 型(該当エコシステムが無いリポジトリ)向けの新規播種スクリプト
- `home/modules/claude.nix` — `rulesets-write-guard` の hook 登録、
  governance skill 3 種への `.github/rulesets/{security,workflow,review}.json`
  オーバーレイのパス変更

## Amendment (2026-10-27 — 毎 PR の required check は待たず、全 workflow の job 名と静的に突き合わせる, #1014・#1065)

D2 の毎 PR の required check(`rulesets-context-check`)は、宣言と live の context を「その
head が実際に報告する job 名」(Actions API の実測、D3)と突き合わせるため、実測の job 名が
揃うまで最大 1200 秒待っていた。2 つの問題が出た。

- #1014: required の集約 job `CI passed` は、`needs:` で束ねた重い job が全部終わるまで自身の
  check-run も報告しない。`Build vega` が Nix のキャッシュの全損で約 35 分かかる日は、待ち窓
  (20 分)を超えて、コードに問題の無い PR の `PR title` が誤検知で落ちた。
- #1065: 窓の中でも、`CI passed` を待つので、毎回 3〜4 分かかっていた。

裁定(2026-10-27、本人): **PR 時の検査だけ**、待たずに、head の `.github/workflows/` 配下の
**全 workflow** から読んだ job の名前(`name:`、無ければ job id)と静的に突き合わせる。報告の
有無は required check 自身(`CI passed`)が保証するので、`PR title` が重ねて待つ必要は無い。

- **全 workflow を読む理由**: `quality.json` の required context は `CI passed`(`ci.yml`)と
  `PR title`(別 workflow の `pr-title.yml`)の 2 つで、`ci.yml` だけを読むと、正常な PR でも
  必ず落ちる。`PR title` の job は、この検査を走らせている job 自身で、その名前が一覧に入る
  ことをテストで固定する。
- **名前の展開**: `name:` の `${{ … }}` はワイルドカード、matrix の job は `<名前> (…)`、
  reusable workflow を呼ぶ job は `<呼び出し側の名前> / …` を取りうる名前に足す。YAML は
  汎用パーサでなく行ベースで読む(依存ゼロを保つ。他リポジトリの CI が sparse-checkout して
  build する)。
- **読み切れないとき**: フロー形式の `jobs:` などで読み切れない workflow があり、context が
  見つからないときは、fail でなく warning にする。判定できないことを、他リポジトリの CI の
  fail にしない。
- **D3 との関係**: D3 の「実測で、YAML は静的パースしない」は、`apply-rulesets`(D5、PUT/POST
  の直前)と `github-audit`(D8)には変わらず適用する。そこは宣言を live へ書く前・監査の
  時点で、実際に報告されたか(trigger の条件で走らない job を含む)を見る必要があるので、
  実測を残す。この Amendment が外すのは、毎 PR の required check の待ちだけ。
- **失うもの(検出のみの限界)**: 存在するが、その PR の trigger では走らない job(`on:` の
  条件)は、静的には分からない。実測の層(上記 2 つ)が検出する。
- **後方互換**: 環境変数 `RULESETS_CONTEXT_CHECK_TIMEOUT` / `_INTERVAL` は読まない(設定
  されていても無視する)。他リポジトリが `@main` の action を取りに来るので、バイナリの
  引数・終了コード・出力の行の形は変えない。

### 執行点

- `crates/pr-contract-check/src/contexts.rs`
- `crates/pr-contract-check/src/job_names.rs`
- `crates/pr-contract-check/tests/contexts.rs`
- `crates/pr-contract-check/tests/consistency.rs`
- `.github/actions/pr-title/action.yml`

## 将来拡張(未実装)

宣言 → live の反映を GitHub App による中央 reconciler で自動化する設計を
ここに残す(D9 の対抗馬)。

- dotfiles に scheduled workflow(`rulesets-reconcile.yml`、cron + 手動
  `workflow_dispatch`)を新設する。
- GitHub App を 1 つ作成し、`Administration: write` + `Contents: read` の
  repository permission で必要な全リポジトリにインストールする
  (fine-grained、`Metadata: read`/`Actions: read` も込み)。
  `actions/create-github-app-token` で installation token を発行する。
- workflow は installation token で各リポジトリの default branch の
  `.github/rulesets/*.json` を取得し、`judge_rulesets` と同じ正規化で
  live と比較して drift があれば PUT/POST する。宣言に無い live ruleset
  は既存の apply-rulesets.sh と同様に**削除しない**(報告のみ)。
  `required-context-unreportable` な宣言(このリポジトリの workflow が
  一度も報告しない context)は自動 apply せず Issue を起票する。
- ADR-436 の releaser App とは別の App にする(権限の分離——releaser App
  は `Administration` を持たない)。
- account-level secret が 1 つで済む(installation token は App の秘密鍵
  から発行するため、各リポジトリに秘密鍵を複製する必要が無い)。
