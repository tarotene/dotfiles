# ADR-1305: 環境の保守の対象の語彙を起こし、載らない skill を落とす検査を CI に配線する

- Status: Accepted
- Date: 2026-10-31
- Issue: #1269
- Amends: なし(ADR-1293 の土台の上に語彙を足し、ADR-1293 の D6 が後の段に置いた `--skills` の CI への配線を果たす。D1〜D7 の決定は変更しない)

## Context

ADR-1293 は、skill を「対象の状態遷移を行う、または前提を満たす手順書」と定め、語彙 JSON の共通の形とグラフ検査を置いた。`--skills`(どの語彙にも載らない skill を落とす検査)は実装して負例を確かめたが、載せ切るまで CI の必須にしなかった(D6)。
ADR-1301 は開発の流れの 4 対象の語彙を足し、載っていない skill は 22 本になった。残りは、環境の保守で動かす対象(リポジトリ設定・routine・GPG 鍵・モデルの pin・GAS プロジェクト・Drive のファイル・ブラウザのセッション・skill 自体)に属する skill 14 本(別名 8 本は正の skill に読み替える)である。

2026-10-31 に、それぞれの `SKILL.md` の本文(`claude-routines` は `auditor.md` と `scripts/routines-plan.sh` の `classify` を含む)を読み、状態と遷移を本文から読み取れる範囲に限って、8 対象の語彙を起こした。
対象が別の person-state リポジトリにある 5 つの skill は、`external-objects.json` に名前だけを足した。

## Decision

### D1: 8 対象の語彙を、共通の形(ADR-1293 D3)で `config/agents/<対象>/vocabulary.json` に起こす

| 対象 | `object` | 状態(終端は太字) |
|---|---|---|
| 1 リポジトリの設定 | `repo-settings` | `unconfigured`・`chartered`・`governed`・`configured`・**`verified`** |
| routine | `routine` | `unmanaged`・`new`・`refuse`・`in_sync`・`declared_ahead`・`live_drift`・`conflict`・**`suspended`**・**`retired`** |
| GPG のサブ鍵 | `gpg-subkey` | `expiring`・`rotated`・`revoked`・`exported`・`nix_updated`・`synced`・`reencrypted`・`pr_created`・`applied`・**`completed`** |
| モデルの pin | `model-pin` | `stale`・`inventoried`・`chosen`・`updated`・**`done`** |
| GAS プロジェクト | `gas-project` | `local_ahead`・`pushed` |
| Drive のファイル | `drive-file` | `local`・`uploaded`・`remote`・`fetched`・**`verified`** |
| ブラウザのセッション | `browser-session` | `logged_out`・`awaiting_login`(owner は person、`shown_by` は `skill:browser-login-handoff`)・`claimed`・`logged_in`・**`resumed`** |
| skill を足す・改める | `skill-authoring` | `drafted`・`wired`・`deployed` |

事象の主なものは次のとおり。

- `repo-settings`: `exist`・`create`(`repo-create-guard` が前提)・`preview`・`charter`・`govern`(`rulesets-write-guard` が前提)・`verify`・`fix_missing`。charter と governance は別々に導入できるので、どちらが先に済んだかで `charter` と `charter_governed`、`govern` と `govern_chartered` の 2 本に分かれる。
- `routine`: `declare`・`create`・`claim`・`adopt`(`claim` の時点で live の prompt に注記が無く、`classify` は `refuse` を返す。`adopt` は update で注記を付ける)・`edit_declaration`・`edit_live`・`apply`・`resolve`・`suspend`・`retire`・`report`(週次の auditor が Issue を起票する)。
- `gpg-subkey`: SKILL.md §1 の 8 ステップが `rotate`・`revoke`・`export`・`edit_nix`・`sync`・`reencrypt`・`open_pr`・`apply` に当たり、`complete` が §2 の完了条件 4 つ。`rotate` は `rotated` か `revoked` のどれか(`--revoke-old` の有無)に至る。`revoke` は rotate より後の状態からも起き、そのあと `export` から `sync` をやり直す(§3・§4)。
- `model-pin`: `follow`・`inventory`・`choose`・`edit`・`fix_leftover`・`verify`。
- `gas-project`: `link`・`push`・`run`・`edit`。
- `drive-file`: アップロードは `stage_local` → `upload` → `verify_upload`、取得は `locate` → `fetch` → `verify_fetch`。
- `browser-session`: `detect`・`handoff`・`login`(person)・`reask`・`confirm`・`resume`。
- `skill-authoring`: `author`・`wire`(`ci` が前提。`vocab-graph-check --skills` を回す)・`deploy`・`revise`・`propose_revision`(`promotion-detect` が毎日、昇格・降格の候補を検出する)。

`gates` と `shown_by` の名前は `pipeline-map.toml` のノード id、`crates/` のクレート名、`config/agents/skills/` の skill 名に解決する(`vocab-graph-check --all` が確かめる)。

### D2: 状態と遷移は本文から読み取れる範囲に限り、読めない遷移は「不明」として Consequences に挙げる

本文に無い状態は新しく作らない。本文が手順を書かない遷移は、語彙に書かず、Consequences の「不明にした遷移」に挙げる。次の 4 点は、本文の記述から状態の位置を読み取った箇所で、本文が状態として書くものではない。
(1) `gas-project` の `edit`(本文は「手元のコードを反映する」と書くだけで、手元を直すとリモートが古くなる遷移は読み取り)。
(2) `drive-file` の `stage_local` と `locate`(ファイルが現れる事象。skill の遷移ではない)。
(3) `routine` の `claim`(adopt を 2 つの事象に分けた。`routines-plan.sh classify` の実装が、注記の無い live を `refuse` と判定することに基づく)。
(4) `gpg-subkey` の `expiring`(SKILL.md の description の「鍵の有効期限が近い」と、`docs/operations.md` の `remind` に基づく)。

### D3: `repo-settings` は 1 リポジトリの導入の流れを持ち、複数リポジトリのずれは `repo-config-drift` に任せる

`repo-config-drift`(ADR-1297)は、監査が drifted と報告した(リポジトリ × ドメイン)の組の裁定の流れで、`routed`(repo-charter・repo-governance へ送った)で終わる。`repo-settings` は、その先の、1 リポジトリに charter と governance を導入して検証するまでを持つ。
`repo-settings` は drifted の状態を持たない(導入後のずれは `repo-config-drift` が追う)。同じ事象(`charter`・`govern`)の担い手に、`repo-charter`・`repo-governance`・`github-audit-triage` を並べて残し、重なりは Issue に切り出した(D7)。

### D4: 対象が別の person-state リポジトリにある skill は、`external-objects.json` に名前だけを足す

ADR-1293 の D2 のとおり、中身も個人の対象の詳細も書かない。object の名前は一般語にする。

| `object` | skill(別名は読み替え) |
|---|---|
| `venue` | `venue-search` |
| `performance` | `music-performance-planning`(別名 `performance-planning`) |
| `calendar-slot` | `human-task-scheduling`(別名 `external-call-scheduling`) |
| `writing-document` | `writing-style` |
| `presentation` | `presentation-authoring`(別名 `talk-script`) |

検査に、`object` の形(kebab-case)・一意・dotfiles の語彙の `object` との非重複を足した。重なると、同じ名前が dotfiles の対象と別リポジトリの対象の両方を指す。

### D5: skill・hook の段階(散文・スクリプト・gate)は、語彙に保存しない。実体から導く検査は、同じ枠に載せられる

ADR-543 の D4 のとおり、段階は宣言せず実体から導く。語彙の事象は `carriers`・`gates` に skill・CLI・ノードを名指しし、`pipeline-map.toml` のノードは `lane`(発火のきっかけ)・`stack`・`auto` を持つので、事象ごとの段階は、すでにある入力だけから導ける。
2026-10-31 の語彙の事象 127 件に、一時の PoC(リポジトリには入れていない)で次の規則を当てた。`skill:` だけの担い手は散文、`cli:` や自動で発火しないノードはスクリプト、`lane` が tool-call・plan-approval・stop・git-hook・ci の自動発火のノードを `gates` に含むものは gate。結果は、散文だけ 46 件、gate を含むもの 22 件、担い手の無い person の事象 9 件などに分かれた。
限界は 2 つある。skill に同梱されたスクリプトは、語彙に名前が出ない限り散文に見える(skill のディレクトリの `scripts/` の有無で補える)。事象の単位では、最も強い段階しか出ない。
結論は「載せられる」。実装(事象ごとの段階を報告するモード)は #1304 に切り出した(段階は事実の導出なので、報告のみで落とさない案)。

`agent-tooling-authoring` は、`skill-authoring` の `author`・`wire`・`deploy`・`revise` の担い手として載せ、`promotion-detect` を `propose_revision` の担い手として載せた。

### D6: `vocab-graph-check --skills` を CI の `dry-run` ジョブに配線する

ADR-1293 の D6 は、必須の job への配線を、環境の保守の語彙を足す段で、全 skill が載ったことを確かめてから行うと置いた。この段で、`--skills` は終了コード 0(載っていない skill が 0 本)になったので、`--selftest` と `--all` の隣に 1 ステップ足した。
`ci-passed` が `dry-run` を待つので、載っていない skill を足した PR は落ちる。例外の一覧は持たない。名指しできない skill は、仕えている skill への統合の候補とする。`--selftest` の `--skills` の負例(載らない skill、別名の読み替え、`external-objects.json` の typo)は配線後も働く。

### D7: 遷移表で見えた重複・衝突は、統合せず Issue に切り出す

ADR-1301 の D6 と同じ。この段で見えたのは次の 3 件。

- `gpg-subkey-rotation` の SKILL.md が、冒頭で手順の数を 7 と書き、§1 に 8 つ並べている(#1302。8 ステップと書く設計ドキュメントと合う 8 が実体)。
- `repo-charter`・`github-audit-triage`・`repo-governance` が `charter` と `govern` の事象を重ねて担い、`repo-charter` §8 が `core` 型の手順を複写し、description・topics の反映の時機が `github-audit-triage` にだけ規定されている(#1303)。
- 段階の導出の実装(#1304。D5)。

## 先行例との対比

- D1: 8 対象の語彙を、共通の形で `config/agents/<対象>/vocabulary.json` に起こす。
  先行例: ADR-1301 D1(開発の流れの 4 対象の語彙を、skill 本文から読み取れる状態と遷移に限って起こした) docs/adr/1301-dev-flow-vocabulary.md (取得 2026-10-31)
  差分: 一致 — 形・置き場・グラフ検査をそのまま使い、対象を足す。環境の保守の対象は、人の端末での操作(YubiKey・ブラウザの認可)と外部システム(GitHub・claude.ai・Drive)が絡むので、`by` に person を持つ事象が多い。
  軸: 表現不可能 — 閉語彙。担い手・前提の名前が実在に解決しない語彙は書けない(`vocab-graph-check` が落とす)。
- D2: 状態と遷移は本文から読み取れる範囲に限り、読めない遷移を「不明」に挙げる。
  先行例: ADR-1301 の Consequences「不明にした遷移」(本文が書いていない遷移は語彙に書かず、一覧にする) docs/adr/1301-dev-flow-vocabulary.md (取得 2026-10-31)
  差分: 一致 — 記録の仕方を踏襲し、本文の記述から位置を読み取った 4 点を明記する。
  軸: 検出のみ — 語彙が本文の範囲に収まるかは、人が本文と突き合わせて読む。
- D3: `repo-settings` は 1 リポジトリの導入の流れを持ち、複数リポジトリのずれは `repo-config-drift` に任せる。
  先行例: ADR-1301 D2(`plan` は計画 1 枚の流れを持ち、要求項目の処分は `plan-requirement` に任せ、複写しない) docs/adr/1301-dev-flow-vocabulary.md (取得 2026-10-31)
  差分: 一致 — 導入の流れとずれの裁定を別の語彙にし、状態を複写しない。`repo-config-drift` の `routed` が `repo-settings` の `exist` につながる関係は、語彙に書かない(対象をまたぐ性質は書かない。ADR-1293 D7 の条件 3)。
  軸: 表現不可能 — 単一正本。ずれの状態を 2 つの語彙に複写しない。
- D4: 別リポジトリの対象を担う skill は、`external-objects.json` に名前だけを足す。
  先行例: ADR-1293 D2・D6(別リポジトリの対象に仕える skill は、対象の名前と skill 名だけを `external-objects.json` に書く) docs/adr/1293-state-transition-vocabulary.md (取得 2026-10-31)
  差分: 異なる — 形(kebab-case)・一意・語彙の `object` との非重複の検査を足した。名前が重なると、どちらの対象を指すか曖昧になる。
  軸: 表現不可能 — 閉語彙。同じ名前が 2 つの対象を指す状態を、検査で書けなくする。
- D5: 段階は語彙に保存しない。実体から導く検査は、同じ枠に載せられる。
  先行例: ADR-543 D4(段階は宣言させず、検出器が実体を直接見て導出する) docs/adr/543-existing-means-and-deterministic-promotion.md (取得 2026-10-31)
  差分: 一致 — 保存しない方針をそのまま守り、導出の入力を語彙の `carriers`・`gates` と `pipeline-map.toml` に特定した。導出の実装は #1304 に切り出す。
  軸: 表現不可能 — 単一正本。宣言欄を作ると、実体とのズレという不正状態が表現できてしまう。
- D6: `--skills` を CI の `dry-run` ジョブに配線する。
  先行例: ADR-1293 D6(必須の job への配線を、環境の保守の語彙を足す段で、全 skill が載ったことを確かめてから行う) docs/adr/1293-state-transition-vocabulary.md (取得 2026-10-31)
  差分: 一致 — 置いた条件(載っていない skill が 0 本)を確かめてから配線した。
  軸: 検出のみ — skill を足す操作そのものは防げず、載せ忘れを CI で検出する。
- D7: 重複・衝突を、統合せず Issue に切り出す。
  先行例: ADR-1301 D6(遷移表で見えた重複・衝突は、統合せず Issue に切り出す) docs/adr/1301-dev-flow-vocabulary.md (取得 2026-10-31)
  差分: 一致 — 統合の判断は Issue に委ね、この段では skill の本文を変えない。
  軸: 検出のみ。

## 却下した案

- `repo-settings` に drifted の状態を持たせる。D3 のとおり、`repo-config-drift` と二重になる。
- `routine` を `awaiting-item` の具体化にし、対応表を足す。この段では足さない。`live_drift`・`conflict`・`suspended` は本人の裁定を待つ状態だが、写し方(P1 の放置への手段が、週次の auditor が同じ Issue をコメントで更新する形で足りるか)は検討していない(未着手)。
- 段階を語彙の欄に持たせる。D5 のとおり、ADR-543 の D4 が退けた。
- `--skills` の例外の一覧を作る。D6 のとおり、名指しできない skill は統合の候補で、例外にしない。この段では名指しできない skill は残らなかった。

## Consequences

### 載せ方

| skill | 語彙 | 事象(担い手として) |
|---|---|---|
| `repo-charter` | `repo-settings` | `create`・`charter`・`charter_governed`・`verify`・`fix_missing` |
| `repo-governance`(別名 `rust-`・`typst-`・`astro-site-governance`) | `repo-settings` | `preview`・`govern`・`govern_chartered` |
| `github-audit-triage`(`repo-config-drift` に載っていたものに追加) | `repo-settings` | `charter`・`charter_governed`・`govern`・`govern_chartered`・`verify` |
| `claude-routines` | `routine` | `declare`・`create`・`claim`・`adopt`・`edit_declaration`・`apply`・`resolve`・`retire`・`report` |
| `gpg-subkey-rotation` | `gpg-subkey` | `rotate`・`revoke`・`export`・`edit_nix`・`sync`・`reencrypt`・`open_pr`・`apply`・`complete` |
| `model-pin-update`(別名 `copilot-model-bump`) | `model-pin` | `inventory`・`choose`・`edit`・`fix_leftover`・`verify` |
| `gas-clasp-ops` | `gas-project` | `link`・`push`・`run`・`edit` |
| `drive-upload` | `drive-file` | `locate`・`upload`・`fetch`・`verify_upload`・`verify_fetch` |
| `browser-login-handoff` | `browser-session` | `detect`・`handoff`・`reask`・`confirm`・`resume`、`awaiting_login` の `shown_by` |
| `agent-tooling-authoring`(別名 `skill-gardening`) | `skill-authoring` | `author`・`wire`・`deploy`・`revise` |

`--skills` の載っていない skill は 22 本から 0 本になった。名指しできない skill は残らなかったので、統合の候補は新たには挙げない。

### 載せ方の弱いもの

`gas-clasp-ops` と `drive-upload` は、Issue で「知識型」と見られていた。名指しできたのは、`clasp push`(手元 → リモートの反映)と `rclone copy`(手元 → Drive)が状態を動かす操作だからである。ただし `gas-project` の状態は 2 つだけで、`edit` は読み取りである(D2)。ホストごとの初回セットアップ・ログインの失効・認可は対象の状態でなくホストの前提で、語彙の外に置いた。
`browser-login-handoff` は、状態を動かす事象のほとんどが対話の手順で、機械の検査が無い(`gates` が空)。`skill-authoring` の `wire` が `gates` に挙げる `ci` は、語彙への名指しの漏れ(`--skills`)と、配備の突き合わせ(`checks.agent-skills`)を回す。設計ドキュメント(`docs/claude/<name>.md`)の有無を検査するかは確かめていない(未確認)。

### 不明にした遷移(本文に書かれていないので語彙に書かない)

- `repo-settings`: review 層の撤去(`repo-governance` の `--delete-ruleset Review`)。governance 導入後の再実行。`core` 型が governance のどの層に属するか(#1303)。exempt 後の扱いは `repo-config-drift` が持つ。
- `routine`: `enabled` と `disabled` の切り替え(本文は `retired` の手順だけを書く)。`suspended` の対処のあとの遷移(「原因を確認してから手で対処する」)。UI の編集で注記が消えたとき `live_drift` と `refuse` のどちらになるか(`classify` の実装は、注記が無ければ `refuse` を先に返すが、本文は書かない)。
- `gpg-subkey`: `rotate` が途中で失敗したときの復旧の細目(§4 は `revoke` 単独で収束させると書く)。カード紐付けの旧サブ鍵の扱い。期限が切れてから回すときの違い。完了のあと、次の期限が近づくと新しい対象として `remind` から始まる。
- `model-pin`: 新モデルがプレビューだけのときの扱い。上流確認で互換性が無いと分かったときの遷移。
- `gas-project`: デプロイ(`clasp deploy`)。ログインの失効(Testing のままだと 7 日)からの再ログインの扱い。
- `drive-file`: token の失効からの再認可(ブラウザの認可を伴い、本人にしかできない)。同名ファイルの上書き、消す操作(本人の依頼が無い限り使わない)。
- `browser-session`: 確認できない状態が続くときの打ち切り。personal ホストで、再接続と確認のどちらを先に行うか(本文は「確認したら再接続」と書き、再接続が旧い process を先に終えるかは未確認と書く)。保存前とアクセス障害の手順(`reference/browser-operations.md`)。
- `skill-authoring`: skill の廃止・統合。hook・CLAUDE.md の器を選んだ場合の配線。昇格を実際に行う遷移(候補の検出まで)。

### 昇格の判断(ADR-543)

D5 の PoC の結果、散文だけの事象は 46 件ある。昇格の兆候(同じ規範違反の再発、逐語反復)は、この段では探していない。段階の分布は、昇格の候補を探す入口になりうる(#1304)。

### そのほか

- `scripts/vocab-graph-check --all`・`--refinements`・`--selftest`・`--skills` がすべて終了コード 0。`--selftest` は 42 件から 45 件に増えた(`external-objects.json` の `object` の形・重複・語彙との重なり)。
- `docs/agents/state-vocabulary.md` に「新しい skill の足し方」を足した。

## 執行点

- `config/agents/repo-settings/vocabulary.json`
- `config/agents/routine/vocabulary.json`
- `config/agents/gpg-subkey/vocabulary.json`
- `config/agents/model-pin/vocabulary.json`
- `config/agents/gas-project/vocabulary.json`
- `config/agents/drive-file/vocabulary.json`
- `config/agents/browser-session/vocabulary.json`
- `config/agents/skill-authoring/vocabulary.json`
- `config/agents/vocabulary/external-objects.json`
- `scripts/vocab-graph-check`
- `.github/workflows/ci.yml`
