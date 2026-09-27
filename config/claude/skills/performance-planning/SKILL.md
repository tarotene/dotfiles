---
name: performance-planning
description: 演奏本番(アマオケ・室内楽・ソロ)の練習計画提案・合わせ調整の統合・当日タイムテーブルと遠征の Calendar dispatch を行う判断知識。演奏本番・練習計画・合わせ確定・当日スケジュール・遠征ロジ・キャパ確認・マイルストーン確認、といった文脈で使う。practice plan, rehearsal confirmation, day-of timetable, travel logistics for a concert、といった英語の文脈でも使う。データの正本(演目・共演者・キャパ宣言値)は別の private な person-state リポジトリの `state/performances/` にあり、このスキルはそこへの読み書き手順を持つ。候補日程一覧 × カレンダーの当たり判定そのものは slot-availability に委譲する(本スキルはその上位で「本番」単位のデータと結びつける)。
---

演奏本番の練習・合わせ・当日ロジを本番のたびに場当たり的に組まないための
判断知識。データモデルの決定根拠は別の private な person-state リポジトリ
側の `docs/adr/0009-performance-planning-data-model.md`(出典は private
リポジトリ側のため、ここでは決定の存在だけを参照する)、設計動機は
`docs/claude/performance-planning.md` を参照。

## 前提: ハブの解決

`performance-hub`(home-manager でデプロイ、PATH 上で呼べる)で person-state
リポジトリのチェックアウトパスを解決する。未設定ならユーザーに設定してもらう
(`docs/claude/performance-planning.md` 参照)。以降、このパスを `$HUB` と
書く。

```
HUB="$(performance-hub)" || exit 1
```

## 前提: TOML の読み書きの分担

`$HUB/state/performances/*.toml` の**読み取り**は `yq -p toml -o json` で
JSON 化してから jq で処理する。**書き込み**は yq の TOML エンコーダが
配列・テーブルをサポートしないため(`docs/claude/performance-planning.md`
の「TOML への書き込みは yq でサポートされない」参照)、Edit ツールで直接
行う — 決定的スクリプトは「どこを・何に書き換えるべきか」を示すところ
までを担う。

## 1. 練習計画提案

1. 対象本番の需要 vs 供給を確認する:
   ```
   bash "$HUB/state/performances/scripts/capacity-check.sh"
   ```
2. マイルストーン目標日の到来を確認する:
   ```
   bash "$HUB/state/performances/scripts/milestone-check.sh"
   ```
3. 両方の出力(供給不足の警告・見積もり未記入の出番・到来済みの
   マイルストーン)を踏まえ、今週の練習配分案を組み立てて提示する。
   **割当を最適化するソルバではない** — 候補を出すだけで、確定は
   週次レビュー(`state/reviews/weekly/routine.md` の Cadence 割当節)の
   中で本人が行う(ADR-0006 D4)。
4. 供給側(`capacity.toml` の `weekly_minutes`)が `0`(未確定)のままなら、
   まずそれを実測値に更新するよう促す — 未確定を偽の「間に合う」と
   混同しない。

## 2. 合わせ調整の統合

候補日程一覧 × カレンダーの当たり判定・確定化そのものは
`config/claude/skills/slot-availability/` の `judge`/`finalize` に委ねる
(手順は同スキルの SKILL.md を参照)。本スキルが追加するのは、確定後に
person-state リポジトリ側の `[[rehearsals]]` へ反映する手順のみ。

1. 対象 performance ファイルを JSON 化し、`coordination_url` が一致し
   `status == "negotiating"` の rehearsal 要素のインデックスを特定する:
   ```
   yq -p toml -o json "$HUB/state/performances/<id>.toml" \
     | jq --arg url "<候補日程一覧の URL>" \
       '.rehearsals | to_entries | map(select(.value.coordination_url == $url and .value.status == "negotiating"))'
   ```
   複数件ヒットした場合(同時に複数の合わせ候補が調整中)は、日付や
   `purpose` で本人に確認してから対象を1件に絞る — 自動で1件目を選んで
   はいけない。
2. slot-availability の `finalize` を実行し、Calendar 側のマーカーを
   確定させる(同スキルの手順どおり)。
3. Edit ツールで、手順1 で特定した rehearsal 要素を書き換える:
   `status = "negotiating"` → `"confirmed"`、`calendar_event` を確定した
   Google Calendar のイベント ID に設定する。既存のコメント・他の
   フィールドはそのまま残す(yq の TOML 書き込みを使わない理由がここ —
   全体を再生成すると手書きの `★ TODO` コメント等が失われる)。
4. 対応する `appearances[].milestones` にまだ触れない — 合わせの確定は
   マイルストーン(D8)とは別軸であり、自動で段階を進めない。

## 3. 当日タイムテーブル・遠征の Calendar dispatch

`day_timetable`・`travel.legs` の相対時刻の表記は実例がまだ乏しく、決定的
パーサ化は時期尚早(YAGNI、`docs/claude/performance-planning.md` 参照)。
以下は Claude が対話的に行う手順:

1. 対象 performance ファイルを JSON 化し、`day_timetable`・`travel` の
   中身を読む。`★ TODO` のプレースホルダが残っている項目は、Calendar へ
   の登録対象から除外し、埋まっていないことを本人に伝える。
2. 本番の開始時刻(このスキーマにはフィールドが無いため、まだ未確定なら
   本人に確認する)を基準に、`day_timetable` の各エントリの相対時刻を
   絶対時刻へ変換する。
3. `travel.legs` の各区間について、確定済みの日時があればそのまま、
   無ければ「要確定」として一覧に残す(勝手に時刻を仮定しない)。
4. 組み立てた Calendar 下書き予定の一覧(件名・開始・終了・説明)を
   提示し、AskUserQuestion で承認を取る。
5. 承認後、`create_event` で登録する。送り先は主カレンダー
   (`tarotene@gmail.com`)に一本化する — 「Claude プロジェクト管理」
   カレンダーは新規予定の送付先として使わない(person-state リポジトリ
   側の裁定、`docs/claude/performance-planning.md` 参照)。
6. 確定した Calendar イベント ID を、手順3で対応する `travel.legs[].
   calendar_event` / `travel.lodging.calendar_event` に Edit ツールで
   書き戻す(TOML 書き込みの分担は上記「前提」のとおり)。

## 外部サービスへの書き込みはサニタイズ対象

Calendar 等の外部サービスに送る文章は、`external-call-scheduling` と
同じ理由でサニタイズする — プロジェクト名・リポジトリ名・内部の Issue
番号・内部ファイルパスを書かない。

## 事例

(まだ無し。新しい失敗事例が出たらここに追記する。)
