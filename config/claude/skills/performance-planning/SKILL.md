---
name: performance-planning
description: 演奏本番(アマオケ・室内楽・ソロ)の練習計画提案・合わせ調整の統合・会場確保の委譲・タイムスケジュール資料の取り込み・付随作業(会場確保・ドレスコード・宿泊)の確認・当日タイムテーブルと遠征の Calendar dispatch を行う判断知識。演奏本番・練習計画・合わせ確定・会場が決まった・当日スケジュール・遠征ロジ・キャパ確認・マイルストーン確認・タイムスケジュール表・付随作業の確認、といった文脈で使う。practice plan, rehearsal confirmation, venue confirmed, day-of timetable, travel logistics for a concert, incoming time-schedule sheet、といった英語の文脈でも使う。データの正本(演目・共演者・キャパ宣言値・合わせ/当日の確定時刻・付随作業の事実)は別の private な person-state リポジトリの `state/performances/` にあり、このスキルはそこへの読み書き手順を持つ。候補日程一覧 × カレンダーの当たり判定は slot-availability に、会場探しそのものは venue-search に委譲する(本スキルはその上位で「本番」単位のデータと結びつける)。
---

演奏本番の練習・合わせ・当日ロジを本番のたびに場当たり的に組まないための
判断知識。データモデルの決定根拠は別の private な person-state リポジトリ
側の `docs/adr/0010-performance-planning-data-model.md`(合わせ・当日の
確定時刻の射影は ADR-0014、付随作業の定型化は ADR-0015 — いずれも出典は
private リポジトリ側のため、ここでは決定の存在だけを参照する)、設計動機は
`docs/claude/performance-planning.md` を参照。

## 前提: ハブの解決

`performance-hub`(home-manager でデプロイ、PATH 上で呼べる)で person-state
リポジトリのチェックアウトパスを解決する。未設定ならユーザーに設定してもらう
(`docs/claude/performance-planning.md` 参照)。以降、このパスを `$HUB` と
書く。venue-search スキルも同じハブを使う。

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

`rehearsal` の `status`(negotiating/confirmed/done)は ADR-0014 により
保存されなくなった — `start`/`end` の有無と `end` が過去かどうかから導出
される。以降の手順は「`status` を書き換える」ではなく「`start`/`end` を
追記する」ことで確定を表す。

1. 対象 performance ファイルを JSON 化し、`coordination_url` が一致し
   `start` キーを持たない(= まだ交渉中の)rehearsal 要素の `key` を
   特定する:
   ```
   yq -p toml -o json "$HUB/state/performances/<id>.toml" \
     | jq --arg url "<候補日程一覧の URL>" \
       '.rehearsals | map(select(.coordination_url == $url and (has("start") | not)))'
   ```
   複数件ヒットした場合(同時に複数の合わせ候補が調整中)は、日付や
   `purpose` で本人に確認してから対象を1件に絞る — 自動で1件目を選んで
   はいけない。
2. slot-availability の `finalize` を実行し、Calendar 側のマーカーを
   確定させる(同スキルの手順どおり)。
3. Edit ツールで、手順1 で特定した rehearsal 要素に確定した日時を
   `start`/`end`(`{date}` または `{dateTime, timeZone}`)として追記する。
   会場が既に分かっていれば `location` も追記する(未定なら「3. 会場確保」
   へ進む)。既存のコメント・他のフィールドはそのまま残す(yq の TOML
   書き込みを使わない理由がここ — 全体を再生成すると手書きの `★ TODO`
   コメント等が失われる)。
4. 対応する `appearances[].milestones` にまだ触れない — 合わせの確定は
   マイルストーン(D8)とは別軸であり、自動で段階を進めない。

## 3. 会場確保

会場探しそのもの(候補サービスの列挙・空き確認・候補提示)は
`config/claude/skills/venue-search/` に委譲する。本スキルが持つのは
「いつ委譲すべきか」「見つかった結果をどこに書き戻すか」の判断だけ。

1. 対象 rehearsal の `venue_by` を確認する(既定は `self`)。
   - `organizer`: 主催者が用意するため venue-search には委譲しない。
     主催者から会場が伝わったら `location` に書くだけでよい。
   - `self`/`partner`: 「6. 付随作業」の `venue-booking` が OPEN なら
     venue-search に委譲する対象。
2. `obligations.sh --json` で当該 rehearsal の `venue-booking` の `stage`
   を読む(閉語彙 `book`/`propose`/`confirm-first`/`agree-area` —
   規則そのものは person-state リポジトリ側の決定〔ADR、存在だけ参照〕に
   あり、このスキルは `stage` で分岐するだけで規則を再導出しない)。
   - `propose`/`confirm-first`: venue-search に `proposal`(既定会場)を
     渡して提案文を作ってもらう。相手が合意したら Edit ツールで `area`
     (駅名)を追記する(→ `book` に進む)。
   - `agree-area`: 相手にエリアの希望を尋ねる(venue-search の従来手順)。
   - `book`: 次の手順(候補提示・予約)へ進む。
3. venue-search スキルに、対象 rehearsal の `area`(あれば)・
   `venue_needs`(グランドピアノ等)を渡して候補を出してもらう。
4. 人間が予約したら、確定した会場名を Edit ツールで対象 rehearsal の
   `location` に追記する(`venue-booking` はこれで完了になる)。

## 4. 資料の取り込み

主催者からタイムスケジュールのシートが払い出されることがあるが、これを
前提にした専用パーサは持たない(相対時刻の表記も資料の形式も主催者ごとに
バラバラで、実例がまだ乏しい — YAGNI、`docs/claude/performance-planning.md`
参照)。資料が無い演奏でも、下記の対応表と同じ項目を対話で埋める。

対応表(資料に出てくる典型項目 → フィールド):

| 資料の項目 | フィールド |
|---|---|
| 開場・開演・終演見込み(本番当日) | トップレベル `start`/`end` |
| 各回の合わせ日程・会場 | `rehearsals[].start`/`end`/`location` |
| ドレスコード | トップレベル `dress_code`(無ければ `"none"`) |
| 宿泊の要否・宿泊先 | `travel.lodging.decision`/`name` |
| 集合時刻・搬入等(相対時刻) | `day_timetable`(「5. 当日タイムテーブル」参照) |

1. 資料(PDF・スプレッドシート・メール本文など、形式は問わない)を読み、
   上の対応表に沿って値を拾う。
2. 現在の TOML との差分(追加・更新するフィールドと値)を提示し、
   AskUserQuestion で承認を取る。
3. 承認後、Edit ツールで反映する(TOML 書き込みの分担は「前提」のとおり)。

## 5. 当日タイムテーブル・遠征の Calendar dispatch

`day_timetable` の相対時刻の表記は実例がまだ乏しく、決定的パーサ化は
時期尚早(YAGNI、`docs/claude/performance-planning.md` 参照)。以下は
Claude が対話的に行う手順:

1. 対象 performance ファイルを JSON 化し、`day_timetable`・`travel` の
   中身を読む。`★ TODO` のプレースホルダが残っている項目は、Calendar へ
   の登録対象から除外し、埋まっていないことを本人に伝える。
2. 本番の開始時刻(トップレベル `start` が確定していればそれ、まだ
   未確定なら本人に確認する)を基準に、`day_timetable` の各エントリの
   相対時刻を絶対時刻へ変換する。
3. `travel.legs` の各区間について、確定済みの日時があればそのまま、
   無ければ「要確定」として一覧に残す(勝手に時刻を仮定しない)。
   `travel.lodging` は「6. 付随作業」の `lodging-decision`/
   `lodging-booking` が完了しているかどうかで扱いを分ける(未完了なら
   Calendar への登録対象にしない)。
4. 組み立てた Calendar 下書き予定の一覧(件名・開始・終了・説明)を
   提示し、AskUserQuestion で承認を取る。
5. 承認後、`create_event` で登録する。送り先は主カレンダー
   (`tarotene@gmail.com`)に一本化する — 「Claude プロジェクト管理」
   カレンダーは新規予定の送付先として使わない(person-state リポジトリ
   側の裁定、`docs/claude/performance-planning.md` 参照)。
6. `travel.legs[]`/`travel.lodging` に対応する確定情報を Edit ツールで
   書き戻す(TOML 書き込みの分担は上記「前提」のとおり)。

## 6. 付随作業(会場確保・ドレスコード・宿泊)の確認

会場確保・ドレスコード確認・スーツ準備/クリーニング・宿泊要否判断/予約は
保存レコードではなく、演奏データから毎回導出するビュー(ADR-0015)。

```
bash "$HUB/state/performances/scripts/obligations.sh"
```

出力の OPEN(未完了)/OVERDUE(期限超過)な項目ごとに対応する:

- `venue-booking`: `stage` に応じて「3. 会場確保」の該当手順へ。
- `dress-code-confirm`: 本人にドレスコードの有無を確認し、`dress_code`
  に記入する(無ければ `"none"` を明記する — キー省略は「未確認」を
  意味するため、区別する)。
- `attire-prep`/`attire-cleaning`: 本人が完了したら `attire.prep`/
  `attire.cleaning` を `done`(または不要なら `not-needed`)に書き換える。
- `lodging-decision`: 本人に宿泊するかどうかを確認し、
  `travel.lodging.decision` に `stay`/`day-trip` を記入する。
- `lodging-booking`: `decision = "stay"` の演奏について、宿泊先が決まったら
  `travel.lodging.name` に記入する(予約自体は人間が行う)。

これらのフィールドを埋めるだけで、対応する期限イベントが Calendar から
自動で消える(`obligations.sh` は導出ビューなので、保存されたレコードを
別途消す操作は不要)。

## 外部サービスへの書き込みはサニタイズ対象

Calendar 等の外部サービスに送る文章は、`external-call-scheduling` と
同じ理由でサニタイズする — プロジェクト名・リポジトリ名・内部の Issue
番号・内部ファイルパスを書かない。

## 事例

(まだ無し。新しい失敗事例が出たらここに追記する。)
