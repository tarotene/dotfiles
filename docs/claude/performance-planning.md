# performance-planning — 演奏本番の練習・合わせ・当日ロジ支援

実装: `crates/hub-resolve`(bin `performance-hub`、旧 `scripts/performance-hub` を #414 / ADR-0024 Stage 4e で Rust へ移植)
スキル: `config/claude/skills/performance-planning/SKILL.md`
関連: 別の private な person-state リポジトリ側の ADR-0010
(`state/performances/` のデータモデル。出典は private リポジトリ側の
ため、ここでは内容ではなく決定の存在だけを参照する)

演奏本番(アマオケ・室内楽・ソロ)の練習計画提案・合わせ調整の統合・
会場確保の委譲・タイムスケジュール資料の取り込み・付随作業の確認・
当日タイムテーブル/遠征の Calendar dispatch を行うスキル。データの正本
(`state/performances/*.toml`、演目・共演者・合わせ/当日の確定時刻・
付随作業の事実を含む)は、この dotfiles とは別の private な person-state
リポジトリに置く裁定になっている(slot-availability の個人設定と同じ
理由)。dotfiles 側はそのデータを読み書きする手順だけを持つ。

## ハブの解決

`writing-style-hub`(docs/claude/writing-style.md)と同型のマーカー方式。
`performance-hub` が `state/performances/` を含む person-state
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

## 6つの機能

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
で行う)。ADR-0014 により `rehearsal.status` が保存されなくなったため、
「交渉中」の判定は `status == "negotiating"` ではなく `start` キーの
不在で行う(person-state リポジトリ側で `status` フィールド自体が
スキーマから削除された)。

### 3. 会場確保(venue-search への委譲)

会場探しそのもの(候補サービスの列挙・空き確認・候補提示)は
`config/claude/skills/venue-search/` が持つ。本スキルは「合わせが確定
していて会場未定なら委譲する」「見つかった会場を `rehearsal.location` に
書き戻す」の2点だけを担い、会場探しの判断知識を重複して持たない
(還元性 — 個人練習のスタジオ予約からも venue-search を呼べるように、
演奏本番のスキルに閉じない設計、`docs/claude/venue-search.md` 参照)。

「委譲すべきか」「委譲する前に何を確認するか」は person-state リポジトリ
側の ADR-0017 が `obligations.sh` の閉じた `stage`(`book`/`propose`/
`confirm-first`/`agree-area`)として導出済みで返す。このスキルは
`stage` を読んで venue-search 呼び出しの前段(相手への確認要否)を
分岐するだけで、規則(既定会場・コマ別の確認要否)そのものはここに
複写しない(出典は private リポジトリ側のため、ここでは決定の存在だけ
を参照する)。

### 4. タイムスケジュール資料の取り込み

主催者から払い出されるタイムスケジュールのシートは形式が主催者ごとに
バラバラで、実例もまだ乏しいため、専用パーサは持たない。資料の項目と
person-state リポジトリ側のフィールドの対応表を SKILL.md に持ち、資料の
有無によらず同じ項目を埋める手順にした(データモデルが資料の存在を
前提にしないようにする — 依頼にあった「それの存在を前提にはしない」を
反映)。

### 5. 付随作業の確認

会場確保・ドレスコード確認・スーツ準備/クリーニング・宿泊要否判断/予約は
person-state リポジトリ側の `state/performances/scripts/obligations.sh`
が演奏データから導出するビュー(ADR-0015)。このスキル側に対応する新規
スクリプトは無く、出力をそのまま読んで対応する手順のみを持つ
(「1. 練習計画提案」が capacity-check.sh/milestone-check.sh の出力を
そのまま読むのと同じ設計)。

### 6. 当日タイムテーブル・遠征の Calendar dispatch

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

`crates/hub-resolve/tests/hub.rs`(`cargo test --workspace`、CI の rust ジョブ)は
縮退経路すべてと、環境変数がマーカーより優先されることを、実機の
`$HOME`/`$XDG_CONFIG_HOME` から隔離した一時ディレクトリで検査する。
ネットワーク・実際の person-state リポジトリへのアクセスは不要。

## 参照

- person-state リポジトリ側の `docs/adr/0010-performance-planning-data-model.md`
  (このスキルが読み書きするデータモデルの決定文書)、`docs/adr/0014-
  performance-events-projection.md`(合わせ・当日の確定時刻の射影)、
  `docs/adr/0015-performance-obligations.md`(付随作業の定型化)、
  `docs/adr/0017-rehearsal-venue-proposal-policy.md`(合わせ会場の提示
  方針)。出典はいずれも private リポジトリ側のため、ここでは決定の
  存在だけを参照する。
- person-state リポジトリ側の `state/performances/README.md`(スキーマ・
  検証スクリプトのポインタ)
- `docs/claude/writing-style.md`(マーカー方式の先行例)
- `docs/claude/slot-availability.md`(判定コアの分離・bash+yq+jq 移植の
  先行例、YAGNI 判断の先例)
- `docs/claude/venue-search.md`(会場探しの委譲先。ハブを共有する設計)
