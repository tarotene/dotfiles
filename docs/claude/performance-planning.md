# performance-planning — 演奏本番の練習・合わせ・当日ロジ支援

スクリプト: `scripts/performance-hub`
スキル: `config/claude/skills/performance-planning/SKILL.md`
関連: 別の private な person-state リポジトリ側の ADR-0009
(`state/performances/` のデータモデル。出典は private リポジトリ側の
ため、ここでは内容ではなく決定の存在だけを参照する)

演奏本番(アマオケ・室内楽・ソロ)の練習計画提案・合わせ調整の統合・
当日タイムテーブル/遠征の Calendar dispatch を行うスキル。データの正本
(`state/performances/*.toml`、演目・共演者情報を含む)は、この dotfiles
とは別の private な person-state リポジトリに置く裁定になっている
(slot-availability の個人設定と同じ理由)。dotfiles 側はそのデータを
読み書きする手順だけを持つ。

## ハブの解決

`writing-style-hub`(docs/claude/writing-style.md)と同型のマーカー方式。
`scripts/performance-hub` が `state/performances/` を含む person-state
リポジトリのチェックアウトパスを解決する。

- `$PERFORMANCE_HUB`(環境変数)
- `${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/performance-hub` の中身
  (1行、絶対パス)

縮退はハブ未設定・パス不在・レイアウト不一致のいずれも明示的なメッセージを
出して非 0 で終わる(`writing-style-hub` と同じ binary-existence gating)。

## TOML への書き込みは yq でサポートされない(重要な制約)

実測(2026-09-27、yq v4.50.1): `yq -i '...' -p toml -o toml file.toml` は
配列・テーブルを含む非スカラー値の TOML 出力を試みると
`only scalars ... are supported for TOML output at the moment` で失敗する。
つまり `state/performances/*.toml` が持つ `[[appearances]]`・`[[rehearsals]]`
のような配列オブテーブルを、yq で TOML として書き戻すことはできない。

このため、このスキルの設計は「TOML の**読み取り**(yq -p toml -o json)は
決定的スクリプトが担うが、TOML への**書き込み**は Claude が Edit ツールで
行う」という分担にした(slot-availability の判定コアが担う「決定的な
判定はスクリプトに固定する」原則とは異なる領域 — TOML 書き込みは yq が
正式サポートしないツール的制約によるものであり、判断のブレを避けるため
ではない)。決定的スクリプトは「どの要素を更新すべきか」を JSON で示す
ところまでを担い、実際のファイル編集は手順内で Claude が行う。

## 3つの機能

### 1. 練習計画提案

person-state リポジトリ側の `state/performances/scripts/capacity-check.sh`
(需要 vs 供給)と `scripts/milestone-check.sh`(マイルストーン目標日の
到来)の出力をそのまま読み、今週の練習配分案を組み立てる。決定的
スクリプトは person-state リポジトリ側に既にあるため、このスキル側に
新規スクリプトは無い — 週次レビュー(`state/reviews/weekly/routine.md`
の Cadence 割当節)から呼ばれる想定で、割当そのものはソルバ化しない
(person-state リポジトリ側の既存決定を継承)。

### 2. 合わせ調整の統合

slot-availability(`config/claude/skills/slot-availability/`)の
`judge`/`finalize` をそのまま呼ぶ。追加するのは「確定後、person-state
リポジトリ側の `[[rehearsals]]` のどの要素を更新すべきか」を決定的に
示す jq クエリのみ(TOML 書き込み自体は上記の制約により Claude が Edit
で行う)。

### 3. 当日タイムテーブル・遠征の Calendar dispatch

`day_timetable`(相対時刻)・`travel.legs`(区間、確定日時なし)は、本番
開始時刻という追加情報が要る上、相対時刻の表記(スキーマ上は自由記述)が
まだ実例 1 件も無い段階で決定的パーサを先行開発するのは投資が早すぎる
(YAGNI — `docs/claude/slot-availability.md`「アダプタはコード化しなかった」
と同じ判断)。当面は Claude が対話的に読み、本番開始時刻を確認した上で
絶対時刻を計算し、Calendar 下書き予定を作る手順のみを SKILL.md に持つ。
実例が複数溜まった段階で、共通する相対時刻の表記が見えたら決定的
スクリプト化を再検討する。

## Dispatch 先カレンダー

person-state リポジトリ側の裁定により、下書き予定の送り先は主カレンダー
(`tarotene@gmail.com`)に一本化されている。「Claude プロジェクト管理」
カレンダーは新規予定の送付先として使わない。

## CI selftest

`performance-hub --selftest`(`.github/workflows/ci.yml` に配線)は
縮退経路すべてと、環境変数がマーカーより優先されることを、実機の
`$HOME`/`$XDG_CONFIG_HOME` から隔離した一時ディレクトリで検査する。
ネットワーク・実際の person-state リポジトリへのアクセスは不要。

## 参照

- person-state リポジトリ側の `docs/adr/0009-performance-planning-data-model.md`
  (このスキルが読み書きするデータモデルの決定文書。出典は private
  リポジトリ側のため、ここでは決定の存在だけを参照する)
- person-state リポジトリ側の `state/performances/README.md`(スキーマ・
  検証スクリプトのポインタ)
- `docs/claude/writing-style.md`(マーカー方式の先行例)
- `docs/claude/slot-availability.md`(判定コアの分離・bash+yq+jq 移植の
  先行例、YAGNI 判断の先例)
