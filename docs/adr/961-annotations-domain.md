# ADR-961 — GitHub Actions の warning を横断で拾う annotations ドメインを github-audit に足す

- Status: Accepted
- Date: 2026-10-23
- Issue: No-Issue(`/grill-me` セッション中に、全リポジトリの Actions の warning を
  根治する依頼から始まり、全件の annotation の実測を経て本 ADR に至った)

## Context

2026-10-23 に、非アーカイブ 24 リポジトリの各ワークフローの最新 run の annotation を
全件読んだ(`gh api repos/<r>/check-runs/<job>/annotations`)。warning は 5 種類あった:
Node.js 20 の非推奨(約 60 件・19 リポ)、キャッシュ API の 400、setup-uv / setup-python の
依存ファイル不在によるキャッシュの空振り、setup-go の同様の空振り、ルールセットの
未播き(ADR-568 Amendment 2026-10-23 の表)。

このうち Node.js 20 の非推奨は、メジャー更新が人手 merge のため滞留していた
(ADR-568 Amendment で automerge にした)。残りの「設定とリポジトリの中身の食い違い」は、
runner が実行時に出す annotation にしか現れない。workflow を静的に検査する lint
(actionlint 等)は、実行しないので見えない。GitHub には warning を CI の失敗にする設定も
無い(GitHub, "REST API endpoints for check runs — List check run annotations"
<https://docs.github.com/en/rest/checks/runs#list-check-run-annotations>、取得 2026-10-23:
`annotation_level` は `notice` / `warning` / `failure`)。同じ食い違いは、人が run の画面で
気付くまで再発しうる。

## Decision

### D1: github-audit に `annotations` ドメインを足す

既定ブランチで workflow ごとの最新の完了 run を読み、その全 job の `warning` annotation を
`missing=<kind>@<workflow>/<job>` として報告する。`failure`(赤は CI 自身が出す)と
`notice` は読まない。`kind` は `node20-deprecated:<actions>` /
`cache-dependency-file-not-found` / `cache-restore-failed` / `cache-save-failed` /
`other:<text>`。判定は決定論で、LLM を呼ばない(ADR-0015)。

本命: github-audit の新ドメイン(各リポの既定ブランチの最新 run を REST で読む)。
対抗馬: 各リポの CI に静的 lint(actionlint / zizmor)を入れる。同じ「再発を拾えるか」の軸で
劣る — 上の warning の大半は runner と実リポジトリの中身から生まれ、workflow のテキストには
現れない。
外した候補: 週次の schedule ワークフロー・クラウド routine での自動起動 — 本人が
「起動は人手」を選んだ(github-audit は今も手動のコマンド)。
既存手段: crates/github-audit — 拡張: 既存の renovate・workflows ドメインと、
github-audit-triage の「検出 → 一括修正 PR」の流れに乗る。
軸: 検出のみ — 実行時にしか出ない警告は、run の前に表現不可能にはできない。

### D2: 明示指定でだけ走り、Rust 版にだけ置く

`Domain::ALL`(既定の全実行)には入れず、`Domain::OPT_IN` に置く。`scripts/github-audit`
(bash 版)には足さない。

- 実行は runs → jobs → annotations と全件をたどるので、CI を持つリポジトリ 1 つあたり
  1 + W + W×J 回の REST を払う(24 リポジトリで約 3.5 分、2026-10-23 実測)。既定の 11
  ドメインの実行に載せると、毎回これを払う。
- 配備されているのは Rust 版(`home/modules/packages.nix`)。bash 版は、依存側
  (`scripts/rulesets-context-check` 等)が `source` するために残っているだけで、
  この判定の依存者はいない。同じ判定を 2 か所に置くと同期が要り、単一正本を崩す
  (ADR-0035 軸 1)。
- bash 版との差分テスト(`crates/github-audit/tests/differential.rs`)が見る既定の出力と
  `--help` は変わらない。`--help` の Rust 版だけの区画(`OPT_IN_USAGE`)は、テストが
  外してから比べる。

先行例: tarotene/dotfiles, `docs/github-audit.md` の `### docs (ADR-640)` ほか、全ドメインが
bash 版と Rust 版の両方にある。この ADR はその慣行から外れる(取得 2026-10-23)。
差分: 異なる — 先行する 11 ドメインは、bash 版を Rust へ移植するための差分テストの対象で、
bash 版が正本だった時代の判定である。bash 版は「依存側の移植が終わるまでリポジトリに残る」
(`crates/github-audit/tests/differential.rs` の冒頭)ものなので、新しい判定を足すと、
その移植が終わった後に消すものを増やす。
軸: 還元 — 同じ判定の複写を持たない。

### D3: 取得は一括でなく 3 段の REST にし、1 ページ(100 件)までを読む

`/actions/runs`(`branch=<既定> status=completed`、`group_by(.workflow_id)` で workflow ごとの
最新)→ `/actions/runs/{id}/jobs` → `/check-runs/{job_id}/annotations`。取得の失敗は空に縮退し、
空は「判定材料なし」として `ok` に倒す(空データを drift と読まない)。100 件を超える
リポジトリは、101 件目以降を見ない(既存の `fetch_*` と同じ水準)。

## Consequences

- warning の再発を、人が run の画面で気付く前に、横断の 1 コマンドで拾える。
  `github-audit annotations` の出力は github-audit-triage の入力にそのまま使える。
- 起動は人手のまま。定期実行は、必要になったとき(同じ見落としが繰り返されたとき)に
  ADR-543 の昇格として検討する。
- PR のブランチ上でだけ出る warning は見ない。既定ブランチに入った時点で出るものが、
  直すべきものだからである。

## 執行点

- crates/github-audit/src/domains/annotations.rs
- crates/github-audit/src/gh.rs
- crates/github-audit/src/audit.rs
- crates/github-audit/tests/pipeline.rs
