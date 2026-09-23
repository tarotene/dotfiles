# ADR-0030 — GAS/clasp 基盤の配置(正本分散・ナレッジは dotfiles・秘密はホストローカル)

- Status: Accepted
- Date: 2026-09-22
- Issue: No-Issue(grill-me セッション中に発見・裁定)

## Context

ある個人プロジェクトの Google Apps Script (GAS) スクリプトが、
script.google.com への手動コピペ実行を前提としていた。実行結果(生成した
Google Form の URL 等)はブラウザの実行ログにしか出ておらず、ログを閉じると
拾い直せずスクリプトの再実行が必要になった実例が発生した。加えて、そのスク
リプトは `FormApp.create` を毎回呼ぶ構造で、再実行のたびに Drive 上に Form/
Spreadsheet が増える(冪等でない)問題もあった。

これを機に「GAS を直接触れる CLI」を導入したいという要望が出た。Google 公式
の CLI である clasp が一次情報(google/clasp README、npm registry、
developers.google.com、いずれも 2026-09-22 取得)上も事実上唯一の現実的な
選択肢であることを確認済み。

導入にあたり、この個人は Google Docs / Spreadsheet 系の自動化を他の文脈でも
頻繁に使っており、GAS の CLI 基盤は今後の別プロジェクトでも再利用される見込
みが高い。そのため「今回のスクリプト 1 本の修正」ではなく「基盤をどこに置く
か」を先に決める必要があった。検討した配置先は: (a) GAS 専用の素振り用
リポジトリ(`clasp-playground`)を新設する、(b) 個人の person-state hub
(private リポジトリ)に集約する、(c) 各利用リポジトリに分散させ、共通部分
(ツール・ナレッジ)だけを dotfiles に置く。

## Decision

1. **コード正本は各利用リポジトリに分散配置**する。GAS のコード +
   `.clasp.json` はそれを使うプロジェクトのソースと同居させ、`rootDir` で
   解決する。GAS 専用の集約 monorepo は作らない — 個々の GAS プロジェクトは
   多くの場合そのリポジトリのドメイン知識(設問文言・処理対象のデータ等)と
   密結合しており、コードだけ切り出すと二重正本の同期コストが増える。
2. **ツール(clasp バイナリ)と運用ナレッジは dotfiles に集約**する。
   `home/modules/packages.nix` に `google-clasp` を追加し、`gas-clasp-ops`
   スキル(`config/claude/skills/gas-clasp-ops/SKILL.md`)に初回セットアップ・
   ログイン・日常操作・スクリプト側の規約を持つ。ADR-0001(home-manager as
   source of truth)に従う — 個人が繰り返し使う CLI ツールは home-manager 層
   の責務。
3. **秘密(OAuth `client_secret.json`・`~/.clasprc.json`)はホストローカルに
   置き、home-manager 管理外・git 管理外**とする。ADR-0010 が退役させた
   「シェル起動時の自動復号」パターンには戻さない。ADR-0022(esa トークン)
   とは異なり GPG 暗号化はしない — clasp 自体が復号ラッパーを挟める構造で
   はなく(コマンドが `~/.clasprc.json` を直接読む)、暗号化すると実行の
   たびに手動で復号・配置し直す手間が生まれるだけで ADR-0022 のような
   launcher 経由の自動復号にできないため。これは `gh auth` のトークン平文
   保存と同型の受容とする。
4. **clasp 専用の素振り用リポジトリ(`clasp-playground`)は今は新設しない**。
   今回の導入(既存の個人プロジェクトへの実戦導入)自体が事実上の素振りを
   兼ねる。「新しい GAS プロジェクトを素早く試したい」需要が繰り返し発生
   した時点で切る(YAGNI)。
5. **person-state hub への集約は不採用**。person-state hub の charter
   (個人の能力・履歴・状態のドメインモデル)と GAS 運用ツール群は目的が
   異なり、同居させるとスコープが濁る。

## Consequences

- 新しい GAS プロジェクトを始めるたびに、そのリポジトリに `.clasp.json` /
  `appsscript.json` を足す一手間が発生する(集約 monorepo なら 1 箇所で済む
  トレードオフ)。`gas-clasp-ops` スキルの手順に従えば数分で済む想定。
- GCP プロジェクト・OAuth クライアントは個人で 1 つを使い回す設計のため、
  複数の GAS プロジェクトを `clasp run-function` で扱うには、各 Apps Script
  プロジェクト側で GCP Project Number を同じ共通プロジェクトに個別に紐付ける
  一手間が発生する。
- 秘密のホストローカル平文保存は、当該ホストのディスク暗号化・ログイン保護
  に依存する(ADR-0003 の脅威モデルの範囲内)。
- clasp-playground を切らない判断は再検討可能: 素振り需要が実際に発生したら、
  この ADR を supersede せず Amendment で対応する。

## Verification

- `home-manager switch` 後、`clasp --version` が v3 系(3.x)を返す。
- `gas-clasp-ops` スキルが `~/.claude/skills/` および `~/.agents/skills/` に
  配備され、新規セッションのスキル一覧に現れる。

## Amendment (2026-09-23 — GCP プロジェクトの命名は personal-cloud-projects.md に従う, No-Issue)

Decision §2 は「個人で 1 つの Cloud プロジェクト + OAuth クライアントを
使い回す」とだけ書き、その命名・設定値の決め方には触れていなかった。
一方この ADR の 2 日前(2026-09-20)に、まさにこの種の判断を都度せずに
済ませるための決定論的な導出規則が `docs/personal-cloud-projects.md`
(#197)として既に正本化されていた。本 ADR はそれを参照せずに書かれたため、
`gas-clasp-ops` スキルの初回セットアップ手順(手順2)は「External /
Testing」という独自の指示を残していた。これは `personal-cloud-projects.md`
の「Testing のまま test user 登録で運用しない(refresh token が 7 日で
失効する)」という決定と矛盾していた。

この ADR が対象とする個人プロジェクトで実際の GCP セットアップ作業を
進める過程でこの矛盾が発覚したため、以下を修正した:

- `docs/personal-cloud-projects.md` に「適用例2: 個人用 GAS 自動化
  スクリプト群(gas-clasp-ops)」を追加。共有基盤(下流に複数スクリプトを
  持つツール)の場合、`<tool>` は下流スクリプトごとではなく共有基盤自体
  (`gas-clasp-ops`)とする扱いを明記した。
- `gas-clasp-ops/SKILL.md` の初回セットアップ手順を、上記適用例2の導出値
  (プロジェクト ID `gas-clasp-ops-<github-username>` / 同意画面アプリ名
  `gas-clasp-ops` / 作成直後に Publish app → In production / OAuth
  クライアント名 `gas-clasp-ops`)を明示する形に書き換え、Testing 運用の
  指示を削除した。

Decision §2 自体(GCP プロジェクト・OAuth クライアントを個人で 1 つ使い回す
という設計)は変更しない。命名・公開ステータスの決め方を
`personal-cloud-projects.md` に委譲する接続を追加しただけの Amendment。
