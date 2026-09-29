# ADR-0000 — 全リポジトリの CI workflow 命名基準を統一する

- Status: Accepted
- Date: 2026-09-30
- Issue: No-Issue(`/grill-me` セッション中に発見・裁定。CI workflow の
  命名基準を統一したいという依頼から始まり、全 21 リポの棚卸しと GitHub
  公式ドキュメントの調査を経て本 ADR に至った)

## Context

owner `tarotene` の非アーカイブ 21 リポで GitHub Actions workflow の命名が
ばらついていた(2026-09-27 の棚卸し)。ファイル名が「関心別」「一括
`ci.yml`」「言語名 `rust.yml`」の3系統に分かれ、`name:` の大小文字が
揃わず(`CI`/`ci`/`Rust CI` 等)、job `name:` が約12 job で未設定(job id が
そのまま required check の context になっている例あり)、`Build`/`Check`/
`Test` のような汎用すぎる required context が複数リポで衝突しうる形になり、
`PR Title / PR title` を required にしながら `pr-title.yml` を持たないリポ
が7つあった。

GitHub 公式ドキュメント("Workflow syntax for GitHub Actions",
"Troubleshooting rules", "About protected branches" — いずれも
https://docs.github.com、取得 2026-09-27)が明記する制約は次の4点のみ:
拡張子 `.yml`/`.yaml` はどちらも可、job id の文字種、`name:` を省略すると
UI にファイルパスが表示されること、そして **required check の context は
`<job name>`(workflow 単位)または `<caller job name> / <reusable job
name>`(reusable workflow 呼び出し)であり、workflow の `name:` 自体は
context に一切含まれない**。job 名は全 workflow で一意にすべきとも明記
されている(同名 job が複数 workflow にあると判定が曖昧になり、マージが
block されうる)。つまり **壊れうるのは job `name:` の層だけ**で、ファイル
名・workflow の `name:`・step 名は GitHub 上どこからも参照されない表示
ラベルに過ぎない。

astral-sh/uv は required check を集約 job 1 本(`all required jobs
passed`)に固定する設計を採っており(取得 2026-09-27)、これは本リポジトリ
自身が ADR-468 Amendment で「matrix 全体の完了を待つ単一の集約 job を
required にする設計は、将来必要になれば別途検討する」として先送りしていた
課題(`docs/adr/468-stack-aware-heavy-ci.md` Decision 2)と同じ形をしている。

## Decision

### D1: required check は集約 job 1 本 + `PR title` に固定する

各リポの PR ゲート job を単一の `ci.yml` にインラインで集約し(D2)、
`needs:` で全 job を束ねる集約 job `ci-passed`(`name: CI passed`)を
唯一の CI required check にする。required 集合は全リポ一律
`["CI passed", "PR title"]`。

判定はインライン式で書き、skipped を合格とする(D3): `if: always()` の
後、`contains(needs.*.result, 'failure') || contains(needs.*.result,
'cancelled')` なら失敗させる。GitHub Docs が明記する意味論
(`skipped`/`success`/`neutral` はいずれも成功扱い)をそのまま集約 job に
写すだけなので外部 action を要さず、SHA pin・Renovate・zizmor の追従負担
が発生しない(re-actors/alls-green のような第三者 action は検討したが
却下 — 既存手段: `.github/workflows/ci.yml` 自前、却下理由は上記)。この
判定方式により、required の正本が「quality.json の文字列複写」から
「ci.yml の `needs` 配列」へ移り、rename・matrix 展開前 skip
(ADR-468 Amendment が実測した「未展開の job は required にできない」問題)
による context 不一致が構造的に起きなくなる。ADR-468 Amendment Decision 2
の先送りをここで決着させる。

不変条件: **`ci.yml` の全 job(`ci-passed` 除く)は `ci-passed.needs` に
含まれる**。GitHub に `needs: *` は無いため、この網羅性は lint
(本 ADR が追加する github-audit `workflows` ドメイン)で検出する
(軸: 検出のみ)。ブロッキング性そのものは「`ci.yml` 内に置くか外に置くか」
という配置で表現する(D7) — 非ブロッキング(advisory)な検査は `ci.yml`
外の自由名 workflow に置く。

### D2: PR ゲート job は `ci.yml` 1 本にインラインする

ローカル reusable workflow への分割はしない。rust-lang/rust は `ci.yml`
1 本に集約している一方、astral-sh/uv・NixOS/nixpkgs は関心ごとに
reusable 分割している(いずれも取得 2026-09-27)—規約は無く組織ごとの
選択。本リポジトリ群の job 数(最大5〜6)では reusable 分割の間接層が
テンプレのファイル単位配布以上の仕事をしないため、rust-lang/rust 型を
採る(軸: 還元)。

### D3: PR title 検査は composite action に置換する

reusable workflow は必ず `<caller job name> / <reusable job name>` の
連結名を作る。この連結ルールの誤解が、実際に required check が永久に
"Expected" のまま止まる事故(#337)と、テンプレートの権限・pin 不具合
(#491、2026-09-29 解消済み)を生んだ。連結そのものが発生しない composite
action(`tarotene/dotfiles/.github/actions/pr-title@main`)へ置換し、
context を呼び出し側 job 名 `PR title` 単独にする(軸: 表現不可能 —
連結名という不一致クラスと caller/callee 間の権限縮小問題が構造的に
消える)。判定ロジック(`scripts/pr-title-check` /
`scripts/pr-merge-settings-check` / `scripts/rulesets-context-check`)は
変更しない(既存手段: 拡張)。

### D4: ファイル名は `.yml`・kebab-case・予約名2つのみ

`.yml` 固定(`.yaml` はゼロ、actions/starter-workflows も `.yml` — 取得
2026-09-27)・kebab-case・予約名 `ci.yml` / `pr-title.yml`、それ以外は
自由な kebab 名とし、閉語彙の接頭辞規約は採らない。Actions の UI は
`name:` を表示するためファイル名の一覧性効果は `ls` にしか効かず、
trigger 種別による分類を接頭辞に載せると `on:` の複写になる(軸: 還元)。
非ゲート workflow は全体で約15本・5リポに集中しており、語彙表を常設する
保守コストが防ぐ揺れに見合わない。既存の表記揺れ(`release-nudge` /
`metrics-reminder` / `report-reminder`)は展開時に `*-reminder` へ一度
だけ寄せる。

### D5: `name:` は workflow・job とも必須、sentence case

存在検査は zizmor `anonymous-definition`(pedantic 限定)にしか無く、
大小文字の規約は GitHub にも主要 OSS にも無い(軸: 検出のみ — GitHub に
workflow スキーマを拡張する手段が無いため lint に頼る)。Title Case は
冠詞・前置詞の例外規則が機械判定できないため、先頭が小文字でないことだけ
を機械検査できる sentence case を採る(`CI passed`, `PR title`, `Nix
flake check` のように固有表記は保持)。job id は kebab-case。

### D6: 検査が無いリポも `ci.yml` を持つ

workflows ドメイン(D8)は titles/renovate/rulesets ドメインと異なり
`.github/workflows` が空でも not-applicable にしない — 検査対象が無い
リポも予約ファイルという土台は持つべきという判断(基準7)。

### D7: 命名検査は PR title composite action と github-audit の両方に置く

`scripts/rulesets-context-check` は PR title reusable 経由で既に全リポの
PR 時に自己検査を実行している(`docs/adr/503-rulesets-declaration-in-repo.md`)
— 同じ運び手に workflow-naming-check を同梱する(既存手段: 拡張)。加えて
`scripts/github-audit` に `workflows` ドメインを追加し、リポ側から
ci.yml の lint step が消されても横断監査で drift が見える形にする
(軸: 検出のみ、本質は lint。二重配置はリポ単位の即時性と横断監査の
双方を確保するため)。判定関数は bash + jq(新規に yq を依存させない —
既存手段: 拡張。`scripts/rulesets-context-check` を含むこのリポジトリの
rulesets/github-audit 系スクリプトはどれも yq を使っておらず、job 構造の
抽出は自リポジトリのテンプレートで書式を統制できる範囲では awk の行
ベース走査で足りるため、YAML 全体を構造的に読む汎用パーサは還元しない)。

quality.json の canonical 値は `repo-governance-common` テンプレート
ファイルを実行時に読む(単一正本 > 複写+同期、ADR-0035) — github-audit
にハードコードした複写は持たない。

### D8: 展開は3段階(このリポジトリの stack → 他20リポ → reusable 撤去)

先行例: `scripts/apply-rulesets.sh` は既定で default branch の宣言を
読み、`--ref` に対する最新 PR head で実測検証する(D5、`declared vs
実測 job 名` の突合)。このため各リポの移行順序は「移行 PR を作成 →
新 context が head で報告されるのを待つ → その head を明示指定して
`--reconcile` → PR title を再実行して緑を確認 → マージ」に固定する
(旧 live context のまま reconcile を先に行うと実測検証で拒否される)。
他20リポへの展開は `github-audit-triage` スキル(ADR-0015)の一括起草・
一括レビュー・一括 PR ループをそのまま使う(既存手段: 拡張、新しい展開
機構は作らない)。

## Alternatives considered

- 集約 job を導入せず個別 job 名に required 規約(命名のみ統一)を課す:
  quality.json はリポごとに異なる内容のまま残り、matrix 展開前 skip・
  rename による context 不一致は解消されない。ADR-468 Amendment が
  既に踏んだ穴を再訪するだけなので却下。
- reusable workflow を維持し、dotfiles 自身も caller 化して全リポ
  `PR Title / PR title` に統一する: 連結名という不一致クラス自体は
  残り、#337 型の事故が再発しうる。composite action へ置換する方が
  構造的に不一致を消せるため却下。

## 執行点

- `scripts/github-audit` — `workflows` ドメイン(`judge_workflows()` /
  `parse_ci_yaml()`)、`ALL_DOMAINS` への追加、GraphQL クエリの
  `declCi`/`declPrTitle` フィールド追加、`--selftest` フィクスチャ
  (本 PR で新規追加・変更)
- `docs/github-audit.md` — `workflows` ドメインの説明節(本 PR で追加)

本 ADR の D1〜D6・D8(集約 job・composite action・ファイル名規約・
展開手順そのもの)は後続の stack 段(composite action / テンプレート /
dotfiles 自身の移行 / 他リポ展開)で執行する。D7(命名検査)の一部
(github-audit 側)は本 PR が執行する。
