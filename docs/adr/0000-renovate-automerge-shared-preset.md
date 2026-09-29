# ADR-0000 — Renovate の自動マージ方針を単一の共有 preset に一本化し、Dependabot の修正経路を閉じる

- Status: Accepted
- Date: 2026-09-29
- Issue: No-Issue(`/grill-me` セッション中に裁定)

## Context

ユーザーから「Renovate の PR を毎回 approve して merge を Web UI で
ポチポチやるのが面倒なので省力化したい。ついでに self-host のメリット・
デメリットも検討してほしい」という依頼を受け、`/grill-me` セッションで
実態調査と裁定を行った。

実測(2026-09-29、`gh` API):

- 全 21 repo の Workflow ruleset は `required_approving_review_count: 0`
  — approve は制度上どの repo でも不要。手作業の実体は「CI 緑の PR を
  Web UI で人が merge する」こと。
- 直近 6 か月の merge 済み Renovate PR は 49 本(月 8 本)。一方 telepath
  (public)は open 6 本すべて CI 赤で BLOCKED、別の稼働中 repo では CI 緑の
  PR が 5 本 7 月から放置されていた。手作業のボトルネックは実在する。
- Renovate App(Mend SaaS)は 4 repo にしか install されておらず、
  `renovate.json` を持つ残り 10 repo(dotfiles 自身を含む)は Renovate PR
  が 0 本の死んだ設定。`docs/operations.md` は「Renovate が dotfiles の
  flake.lock を週次更新している」と書いていたが、実績はゼロだった。
- automerge ポリシーは全 repo で共通にしたいが、現状は
  `config/claude/skills/{rust,astro,typst}-repo-governance/templates/
  renovate.json` の 3 テンプレートと dotfiles 自身の `renovate.json` に
  同種の設定が複写されており、ポリシー変更のたびに複数 PR が要る構造
  だった。
- Dependabot security updates(自動修正 PR)と Renovate の
  `vulnerabilityAlerts` が同じ脆弱性に対して重複して PR を出しうる状態
  だった(ある稼働中 repo に Dependabot PR 8 本、telepath(public)に
  1 本 open)。

## Decision

### D1: 非 major の更新を automerge し、major と 0.x の minor は人が merge する

`packageRules` で `matchUpdateTypes: ["patch","pin","digest","pinDigest"]`
と `matchUpdateTypes: ["minor"], matchCurrentVersion: "!/^v?0\\./"` に
`automerge: true` を付ける。`lockFileMaintenance` にも `automerge: true`
を付ける。

出典: Renovate maintainers, "Automerge configuration and
troubleshooting" <https://docs.renovatebot.com/key-concepts/automerge/>
(取得 2026-09-29) — "Lock file maintenance: lowest risk, enable
automerge directly" / "Non-major updates: automerge minor/patch,
excluding pre-1.0.0 versions using matchCurrentVersion: !/^0/"。
差分: 一致。
軸: 検出のみ — 「破壊的更新かどうか」は semver の自己申告でしか判定
できず、CI が最終防衛線になるため。

### D2: merge の実行主体を GitHub ネイティブ auto-merge にする

`platformAutomerge: true` / `automergeType: "pr"` /
`automergeStrategy: "squash"`。Renovate 自身の merge
(`platformAutomerge: false`)は採らない。

本命: なし — 両候補を「required checks 未達の merge を表現不可能に
できるか」という同じ軸で比較した。
対抗馬: `platformAutomerge: false`(Renovate がブランチ上の全 check 緑を
確認して自分で merge。ruleset の乖離に強いが、merge は次回 Renovate 実行
時まで遅延し、Mend Community Cloud の SaaS 実行間隔(4 時間)に縛られる)。
外した候補: `automergeType: "branch"` — 制約で外した(全 repo の
Workflow ruleset が PR を要求しており、Renovate 公式ドキュメントが "If
you have configured your project to require Pull Requests before
merging, it means that branch automerging is not possible" と明記)。
既存手段: `.github/rulesets/quality.json`(ADR-503)— 採用:
GitHub 標準の auto-merge。required checks の宣言は ADR-503 の in-repo
ruleset 宣言をそのまま使い、新しい gate は作らない。
出典: Renovate maintainers, "Configuration Options — platformAutomerge"
<https://docs.renovatebot.com/configuration-options/#platformautomerge>
(取得 2026-09-29) — "If you use the default platformAutomerge=true then
you should enable your Git hosting platform's capabilities to enforce
test passing before PR merge"。GitHub, "Automatically merging a pull
request"
<https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/incorporating-changes-from-a-pull-request/automatically-merging-a-pull-request>
(取得 2026-09-29)。
差分: 一致 — required checks の整備を前提条件にする(各 repo での
required checks 整備は D7 によりこの PR のスコープ外)。
軸: 表現不可能 — required checks 未達の merge をプラットフォーム層
(ruleset)で不可能にする。宣言 > 手続き。

### D3: 公開直後 3 日を待ち、脆弱性修正だけ即時にする

`minimumReleaseAge: "3 days"` を preset 全体に、`vulnerabilityAlerts`
にだけ `minimumReleaseAge: null`(即時)+ `automerge: true` を付ける。

出典: Renovate maintainers, "Configuration Options — minimumReleaseAge /
internalChecksFilter"
<https://docs.renovatebot.com/configuration-options/#minimumreleaseage>
(取得 2026-09-29) — `internalChecksFilter` の既定 `strict` は "PRs will
be skipped unless a non-pending version is available"
(`lib/config/options/index.ts` の `default: 'strict'`、同日取得)。
組織内先行例: `typst-repo-governance` テンプレートの
`minimumReleaseAge: "7 days"`(ある稼働中 repo で運用中)。
差分: 異なる — 7 日ではなく 3 日にする。週次 schedule(月曜早朝)と
合わせた実効遅延を最大 10 日に抑えるため(7 日だと最大 2 週間)。ユーザー
裁定。
軸: 検出のみ — 悪性リリースの公開そのものを表現不可能にはできず、
取り下げまでの猶予期間を買うだけ。

### D4: ポリシーを単一の共有 preset に集約する

`renovate/policy.json` を dotfiles に 1 本置き、他 repo は
`github>tarotene/dotfiles//renovate/policy`(浮動、tag pin なし)を
extends する。governance テンプレートと dotfiles 自身の
`renovate.json` はエコシステム固有ルールだけを残す。

本命: なし — 共有 preset / テンプレート複写継続 / 専用
`renovate-config` repo の新設の 3 候補を単一正本性で比較した。
対抗馬: テンプレートへの追記 + 複写継続(現行の「複写+同期」型。
github-audit で差分は検知できるが、ポリシー変更のたびに複数 PR が要る)。
外した候補: 専用 `renovate-config` repo の新設 — 感触で外した
(governance の正本が dotfiles に集約されている現状と分散する気がした。
分析ではない)。
既存手段: `config/claude/skills/*-governance/templates/renovate.json`
— 拡張: 既存テンプレートを extends 形に縮退させ、preset 参照の解決は
Renovate 標準の `github>` プリセット機構に任せる(新しい同期スクリプトは
書かない)。
出典: Renovate maintainers, "Shareable Config Presets"
<https://docs.renovatebot.com/config-presets/> (取得 2026-09-29) —
"GitHub with preset name and path: github>abc/foo//path/xyz →
path/xyz.json, Default branch"。組織内先行例: ADR-0033(索引の単一
正本化)、ADR-503(ruleset 宣言の in-repo 正本)。
差分: 一致 — Renovate 公式が推す専用 `renovate-config` repo ではなく
dotfiles 内のサブパスにする点だけ異なる(正本の集約先を増やさないため)。
軸: 表現不可能 — 単一正本 > 複写+同期。ポリシーの repo 間乖離が構造的に
起きない。

### D5: Mend App の適用範囲を「All repositories」に切り替える

install 先を列挙する registry は dotfiles に作らない。

出典: `docs/adr/436-single-releaser-github-app.md` D2「宣言の正本を
workflow ファイルの存在に置き、対象 repo を列挙するファイルは作らない」、
D5「App の install 有無は検出しない」(取得 2026-09-29)。
`scripts/github-audit` `judge_renovate()` #465 の Dashboard 代理指標。
差分: 一致 — ADR-436 と同じく列挙ファイルを持たず、既存の代理指標
(Dependency Dashboard Issue)で検出を続ける。
軸: 表現不可能 — 「入れ忘れ」という状態そのものを All repositories で
消す。宣言 > 手続き。

### D5b: Dependabot security updates を全 repo で OFF にする

Dependabot alerts は残し、修正 PR の経路は Renovate の
`vulnerabilityAlerts` に一本化する。

出典: Renovate maintainers, "Configuration Options — vulnerabilityAlerts"
<https://docs.renovatebot.com/configuration-options/#vulnerabilityalerts>
(取得 2026-09-29) — "Renovate can read GitHub's Vulnerability Alerts ...
you must enable the Dependency graph, and Dependabot alerts"。GitHub
REST "Repos" <https://docs.github.com/en/rest/repos/repos>(取得
2026-09-29)— `DELETE /repos/{owner}/{repo}/automated-security-fixes`。
差分: 一致。
軸: 還元 — 同じ脆弱性に 2 本の PR を出す重複機構を消す。

### D6: Self-host は採らず SaaS(Mend Community Cloud)を継続する

本命: なし — 依頼が「メリット・デメリットを検討」だったため、結論を
先に置かず SaaS / GitHub Actions cron / GCP Cloud Run Jobs / ホストの
systemd timer の 4 候補を同じ軸で比較した。
対抗馬: GCP Cloud Run Jobs + Cloud Scheduler(private な GCP IaC repo の
Terraform で宣言、月数ドル。毎時実行と global config の自由が得られるが、GitHub App
新設・Secret Manager・コンテナ pin の自己追従・サイレント停止の監視が
要る)。
外した候補: public dotfiles の GitHub Actions cron — 制約で外した
(Actions ログに private repo 名が出て ADR-0034 に反する)。vega の
systemd timer — 制約で外した(ADR-0010 の「ホストに秘密ファイルを
置かない」方針と衝突)。private repo の GitHub Actions cron — 感触で
外した(毎時 2〜3 分の実行で月 1500〜2000 分を消費するのが Free/Pro 枠に
対して窮屈に感じた。分析ではない)。
既存手段: `docs/adr/436-single-releaser-github-app.md` — 採用: Mend
Renovate App(<https://github.com/apps/renovate>)。将来 self-host に
移る場合も ADR-436 の「1 App 方針・PEM は Bitwarden」をそのまま流用し、
新しい秘密配布機構は作らない。
出典: Mend, "Mend Renovate Cloud-hosted Overview"
<https://docs.renovatebot.com/mend-hosted/overview/> (取得 2026-09-29)
— Community Cloud は 4 時間間隔 + webhook 駆動、同時実行 1 job、30 分
タイムアウト。Renovate maintainers, "Running Renovate"
<https://docs.renovatebot.com/getting-started/running/> (取得
2026-09-29) — self-host は "provide the infrastructure ... provision
Renovate's global config ... make sure Renovate runs regularly" を
運用側が担う。renovatebot/github-action README
<https://github.com/renovatebot/github-action> (取得 2026-09-29)。
組織内先行例: ADR-0010(旧 `RENOVATE_APP_ID`/`RENOVATE_APP_PRIVATE_KEY`
の退役 = 過去に self-host を止めた実績)。
差分: 一致 — Renovate 公式の hosted/self-hosted の分担どおり。
軸: 還元 — self-host が単独で担う仕事は「4 時間→1 時間への間隔短縮」
だけで、automerge の実効遅延は D3 の `minimumReleaseAge` 3 日の方が
支配的。より安い手段(SaaS 継続)で足りる。

#### Self-host との比較表

| 観点 | SaaS(Mend Community Cloud、継続) | Self-host(参考: GitHub Actions cron / GCP Cloud Run Jobs) |
|---|---|---|
| 実行間隔 | 4 時間(webhook駆動で PR 操作は即時反映) | 毎時可能(cron 次第) |
| 運用コスト | ゼロ(App install のみ) | インフラ(runner/Cloud Run)・スケジューラ・障害監視が要る |
| 秘密の管理 | Mend 側(こちらは何も持たない) | GitHub App(ADR-436 の 1 App 方針を流用)+ PEM 配布経路が要る |
| global config の自由度 | Mend 側の制約に従う(hostRules 等は repo 単位で足りる範囲) | 完全に自由 |
| 実行ログ | Dependency Dashboard 経由のみ | 自前で保持可能 |
| 費用 | 無料 | GitHub Actions は private repo で月 1500〜2000 分消費、GCP は月数ドル |

**Self-host へ移る発火条件**(いずれか):
- Mend Community Cloud の無料枠が縮小・廃止される。
- 複数 Organization を横断する運用が必要になり、1 App でのアカウント
  跨ぎ管理が要る(#506 で検討中の GitHub App as-code 化と合流する場合)。
- private registry 等、hostRules だけでは足りない global config が
  必要になる。
- 実行ログを自前で保持する必要が生じる(監査要件等)。

### D7: このリポジトリ側の執行は検出と設定適用までとし、各 repo への展開は別セッションに委ねる

`scripts/github-audit` の新 drift コード(検出)と
`apply-repo-settings.sh`(適用側の宣言値)までを執行点とし、各 repo の
`renovate.json` を書き換える apply スクリプトはこの PR チェーンでは
作らない。

出典: `docs/adr/0015-unified-github-audit-and-triage-loop.md`(検出は
決定論・修正は triage ループ)、`docs/adr/436-single-releaser-github-app.md`
D4「執行は github-audit の新ドメイン(検出)に限り、apply スクリプトは
作らない」(取得 2026-09-29)。
差分: 一致。
軸: 還元 — 展開は既存の `github-audit-triage` スキルで担え、新しい配布
機構は不要。

### D8: judge_renovate() の隣接する偽陽性・偽陰性を同じ PR チェーンで直す

`:disableDependencyDashboard` extends を dashboard-disabled として
未検知な偽陽性(ある Rust 系 private repo)と、typst repo が manifest
判定から漏れて not-applicable になる偽陰性(ある Typst 系 private
repo)は、Stage 3 と同じ関数(`judge_renovate()`)を触るため、この PR
チェーンの一段として直す。

出典: `docs/adr/0027-uncertainty-first-stacking.md`、
`config/claude/skills/scope-inventory/SKILL.md` §4(取得 2026-09-29)。
差分: 一致。
軸: 還元 — 展開セッションの findings から偽陽性・偽陰性を消し、triage の
手戻りを無くす。

## Alternatives considered

- Dependabot version updates(Renovate を廃して Dependabot に一本化) —
  検討したが採らない。Dependabot はグルーピング・monorepo 対応・
  カスタムマネージャが Renovate に劣り、既に 21 repo中 4 repo で
  Renovate が稼働中で移行コストが対価に見合わない。
- Preset を tag pin する(`github>tarotene/dotfiles//renovate/policy#v1`)
  — 検討したが採らない。dotfiles は単一チームの private な運用ポリシーで
  あり、破壊的変更が起きたらその場で全 repo に同時反映されてよい
  (むしろ tag pin すると D4 の「単一正本」が骨抜きになる)。

## Consequences

- 各 repo への展開(renovate.json の extends 書き換え、required checks
  整備、Mend App の Web UI 切替、Dependabot security updates の無効化)は
  `github-audit-triage` スキルの別セッションで行う。
- 展開が完了するまで、各 repo の automerge は preset を extends した
  時点から有効になる(App が動いている 4 repo から順に効果が出る)。
- `renovate-app.md`(Stage 2 で追加予定)が Mend App の運用手順の正本に
  なる。

## 執行点

- renovate/policy.json
- renovate.json
- .github/workflows/nix.yml
