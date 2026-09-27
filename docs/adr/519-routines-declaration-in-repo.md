# ADR-519 — Claude Code routine の正本を対象リポジトリ自身の宣言に一本化する

- Status: Accepted
- Date: 2026-09-27
- Issue: No-Issue(`/grill-me` セッション中に発見・裁定)

## Context

Claude Code routine(claude.ai 上の scheduled cloud agent)は、
`claude.ai/code/routines` の Web UI・`/schedule`・`RemoteTrigger` ツール
経由で個別に作るしかなく、定義(prompt・cron 式・対象 repo・model)が
どこにも版管理されていなかった。ユーザーの依頼(`/grill-me
Claude Code Routine を as-a-code できないですか？`)から始まったグリル
セッションと、その後の Plan agent による敵対的レビュー(replan)を経て、
ADR-503(`docs/adr/503-rulesets-declaration-in-repo.md`、required status
check の正本を対象リポジトリ自身の宣言に一本化する)と同型の解として、
対象リポジトリ自身の `.claude/routines/` を正本にする形に決めた。

実測(`RemoteTrigger` ツール、および段0の実機プローブ、2026-09-27)で
次を確認している。詳細は `docs/claude/claude-routines.md`「実測」節。

- API(`/v1/code/triggers`)は list/get/create/update/run/
  create_webhook_trigger。**delete が無い**。`list` は cursor を無視し
  1 ページ目(20 件、newest-first)しか返さない。
- `update` は `job_config.ccr` に触れると全置換になる。
  `environment_id` を省略すると 400、`session_context` を省略すると
  `sources`/`model` が消え `allowed_tools` がサーバ既定値に置き換わる。
- 全 routine で `mcp_connections`(connector 5件、uuid まで一致)が同一 —
  アカウント既定セットで、routine ごとの宣言対象ではない。
- `sources` が空の routine が実在する(prompt 内で `gh repo clone` を
  手動実行する形)。家リポジトリは `sources[0]` から導出できない。
- クラウド sandbox に `gh` CLI は無い。GitHub アクセスは routine の
  `sources` に scope される。
- クラウドの meta connector(`Claude_Code_Remote`)には
  `delete_trigger`/`list_triggers` がある(ローカルの `RemoteTrigger`
  ツールには露出していない)。

## Decision

### D1: 宣言の正本は対象リポジトリ自身

routine の設定と prompt 本体を、その routine が主に対象とする
「家リポジトリ」の `.claude/routines/<name>.{json,md}` に置く(D1)。
dotfiles(本リポジトリ)は差分計算ツールと手順だけを持ち、宣言そのものは
一切持たない — 対象の多くが private リポジトリで、ADR-0034 により実 ID・
repo 名を本リポジトリに書けないため(D2)。

### D2: prompt 本体は薄いポインタとして live に展開する

live の prompt は `Read and follow .claude/routines/<name>.md in
<home_repo>.` という固定文言だけを持ち、宣言ファイルの `<name>.md` が
実体を持つ。routine は sources を clone して動くため、repo 内ファイルを
正本にできる(D3)。

### D3: 3-way 差分は prompt 末尾の routine-spec 注記で行う

宣言(`.json` + `.md`)と live 側それぞれから正規化射影(name・cron_utc・
model・environment_id・sources・allowed_tools・enabled・prompt 本文)を
作り、sha256 でハッシュ化する。live の prompt 末尾に
`routine-spec: <sha256>` という注記行を持たせ、これを Kubernetes の
last-applied-configuration annotation に相当するものとして使う(D4)。
宣言側のハッシュ・live 側のハッシュ・注記の3つを比較し、
in-sync/declared-ahead(宣言が先行、自動 update 対象)/live-drift(live
側だけ UI 等で変わっている)/conflict(両方乖離)/suspended
(`suspension_reason`/`ended_reason` が非空)/refuse(注記が無いか
name が一致しない)/new(宣言に id が無い)の7分類に振り分ける。
差分コア(`scripts/routines-plan.sh` + `scripts/lib.jq`)は bash + jq の
みで動く決定的 CLI とし、ローカルのスキルはその出力(create/update
body)を `RemoteTrigger` にそのまま渡すだけにする(D5、slot-hit.sh
〔#504〕と同じ「決定的 CLI + 薄い中継」の型)。

### D4: 同定は trig_ ID の pin + name の名前空間キー検証

`list` が cursor を無視して 1 ページ目しか返さない(実測)ため、
live 側の同定は `name` 検索に頼れない。宣言に pin した `trig_` ID を
`get` で引き、live の `name` が `<home_repo>:<name>` の名前空間キーと
一致するかで検証する(D6)。

### D5: 定期監査・反映はクラウドの自己監査 routine が担う

週次 cron の自己監査 routine(`role: auditor`)が、宣言先行(declared-
ahead)の routine だけを `update` する。**create はしない** — 未マージの
書き戻し PR がある間に次回実行が来ると重複 create が起き、delete API が
無いため取り消せない。**auditor 自身の宣言も update しない** — 壊れた
prompt を自分で直せない循環を避けるため、自分自身は report のみ
(D7)。new(宣言はあるが未作成)・live-drift・conflict・suspended・
refuse は Issue を起票する(隠しマーカーで重複防止)。`create` を伴う
新規 routine の作成は常にローカルセッションで行う。

### D6: auditor 手順の正本は dotfiles、private 側は薄いポインタ

`config/claude/skills/claude-routines/auditor.md` に汎用手順を書き、
private な auditor 宣言の prompt はこのファイルを読んで従うだけにする —
ADR-396(decision-colocation)により決定成果物には執行点が要るが、
private repo の prompt はそもそも本リポジトリの執行点にはできないため、
汎用手順そのものをここに置くことで執行点を満たす。

### D7: ローカルの宣言外書き込みは構造検査 guard で deny する(段2)

`crates/routines-write-guard`(PreToolUse、`RemoteTrigger` の cron 付き
create/update が対象)は、bypass を持たない構造検査にする — `RemoteTrigger`
は Bash ツールではないため、既存の `rulesets-write-guard` が使う
`BYPASS=` コマンド接頭辞方式(Bash の字句解析)は成立しない。通過条件は
name が名前空間キーの形であること、prompt 最終行が
`routine-spec: [0-9a-f]{64}` であることの2つ — スキルの出力は常にこれを
満たす。

### D8: 廃止は state: retired、delete は使わない

クラウドの meta connector に `delete_trigger` が存在することを実機で
確認した後も、ローカルの `RemoteTrigger` ツールにはその経路が無いこと、
削除は不可逆であることから、廃止は `state: retired` で `enabled:false`
固定・宣言ファイル保持のままとする裁定を維持した(ユーザー確認済み)。

## Alternatives considered

- **宣言を dotfiles に集約する**: 対象 repo の多くが private で、
  実 ID・repo 名を本リポジトリに書けない(ADR-0034)ため不採用。
- **定期監査を private hub の GitHub Actions(GitHub App token)にする**:
  ADR-503 D9 が「App 秘密鍵の全リポジトリ複製」を理由に reconciler を
  退けたのと同型の検討をしたが、クラウド routine は新しい資格情報を
  要らないため、その理由が当たらない。ただし triggers API 側の経路が
  未検証だったため、まず段0c で meta connector を実地検証してから
  クラウド routine 案を採った。
- **auditor に create/delete も持たせる**: 重複 create と不可逆な
  delete のリスクがあり、Issue 化による人間確認を優先した(D5)。

## Consequences

- routine の設定・prompt が対象リポジトリの PR レビューを通るようになる。
- `RemoteTrigger`/meta connector 呼び出しは依然手動(スキル・auditor
  経由)——完全な GitOps reconciler ではない。live 側の UI 編集は
  auditor が検出するだけで、自動で宣言側に上書きしない。
- 宣言に無い run-once/webhook routine・アカウント既定の connector 設定は
  この仕組みの管理対象外のまま。
- research preview の API が変わった場合、`scripts/routines-plan.sh` の
  jq ロジックと `build_body`/projection の形状を追従させる必要がある。

## 執行点

- `config/claude/skills/claude-routines/scripts/routines-plan.sh` — 差分
  コアの CLI 本体(build-body/classify/classify-unmanaged)
- `config/claude/skills/claude-routines/scripts/lib.jq` — 正規化射影・
  body 組み立ての共有関数
- `config/claude/skills/claude-routines/scripts/selftest.sh` — 自己検査
  (7分類 + unmanaged 検出 + run-once 拒否を網羅)
- `config/claude/skills/claude-routines/SKILL.md` — ローカルセッションの
  create/adopt/apply 手順
- `config/claude/skills/claude-routines/auditor.md` — クラウド自己監査
  routine の汎用手順(D6 の執行点)
- `docs/claude/claude-routines.md` — 決定表・実測の記録

段2(`crates/routines-write-guard`)・段3(`scripts/github-audit` の
routines ドメイン)の執行点は、それぞれの段の PR でこの節に追記する。

## Verification

- `config/claude/skills/claude-routines/scripts/routines-plan.sh --selftest`
  (ネットワーク不使用、7分類全件 + unmanaged 検出 + cron_utc 必須検査)
- CI の「Every --selftest is wired」チェックが本スクリプトを検出すること
  (段1で `.github/workflows/ci.yml` に配線)
- `cargo test -p routines-write-guard`(段2、名前空間キー・注記の構造検査
  7ケース)
- `scripts/github-audit --selftest`(段3、routines ドメインの
  not-applicable/ok/drifted 4ケースを含む既存 selftest 一式)
