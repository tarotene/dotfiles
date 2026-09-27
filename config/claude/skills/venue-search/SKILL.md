---
name: venue-search
description: 合わせ・個人練習の会場(レンタルスタジオ・公民館等の公共施設)を探し、候補を人間に提示する判断知識。会場探し・練習部屋を探す・スタジオ予約・合わせの場所・グランドピアノがある部屋・公民館・区民センター、といった文脈で使う。rehearsal venue search, practice room booking, find a rental studio with a piano、といった英語の文脈でも使う。データの正本(自宅・職場・公共施設の登録状況・常用スタジオ)は別の private な person-state リポジトリの `state/places.toml` にあり、このスキルはそこへの読み取り手順と、会場サービスの一次情報知識を持つ。予約(人間が行う)そのものは代行しない — 探すところまでを担う。
---

会場探しを Instabase・SpaceMarket・Google 検索・使い慣れた店舗の場当たり
巡回にしないための判断知識。API を公開しているサービスは(2026-09-27
時点の調査で)見つからなかったため、エージェントが担うのは「ログイン
不要で見られる空き状況を機械的に確認し、候補を絞る」ところまでで、予約
自体は人間が行う(低コストで済む経路を優先する)。

## 前提: ハブの解決

`state/places.toml` は演奏企画のデータ(`state/performances/`)と同じ
private な person-state リポジトリに置く。ハブの解決は
`performance-planning` スキルが使う `performance-hub` をそのまま流用する
— このスキル専用のマーカーは新設しない(同じリポジトリを指すハブ方式を
2つ持つ理由が無い)。

```
HUB="$(performance-hub)" || exit 1
```

`performance-hub` が無い/未設定の場合の縮退はそちら側の挙動に従う
(`docs/claude/performance-planning.md` 参照)。

## 1. 要件を集める

- 日時(合わせ・練習の候補日、または探したい期間)
- 希望エリア(`state/performances/*.toml` の対象 rehearsal に `area` が
  あればそれ、無ければ `state/places.toml` の `home`/`work` の駅を起点に
  する)
- 会場要件(`venue_needs` の閉語彙。現状 `grand-piano`/`upright-piano`)

## 2. 候補サービスを機械的に列挙する

`state/places.toml` の内容(登録済みの公共施設・常用スタジオ)と、ピアノを
要件に含むかどうかから、見に行くべきサービスの一覧を決定的に出す
(候補の取捨選択自体は人間の判断に委ねる会話をブレさせないため):

```
bash scripts/venue-urls.sh --hub "$HUB" --needs grand-piano
```

出力の各要素は `service`/`branch`/`url`/`login_required`/`note`。`url` が
`null` のものは、このスクリプトが既知の URL を持たない(手動で確認する)。

## 3. 空き状況を確認する

- `login_required` が `false` のページ(例: ノアの週表示)は WebFetch/
  Playwright で直接確認する。JS で後から描画されるページ(Instabase の
  一覧等)は Playwright を使う。
- `login_required` が文字列(条件付き・未確認)のものは、まずそのページ
  自体を一度見て、ログイン無しでどこまで見えるかを確かめてから続ける。
- `note` に書かれた利用規約上の注意(例: Instabase の robots.txt が
  クエリ付き検索 URL を Disallow にし 60 秒間隔を求めている)を守る —
  個別ページを間隔を空けて見る、検索フォームへの連続リクエストはしない。
- 公共施設(えどねっと等)はピアノの有無が施設・部屋ごとに違うため、
  部屋の設備情報のページも合わせて確認する。

## 4. 候補表を提示する

日時ごとに 2〜3 案を目安に、各案へ会場名・料金(分かれば)・ピアノの機種・
予約方法(Web/電話/要利用者登録)・リンクを添えて提示する。複数回の合わせ
をまとめて依頼された場合は、回ごとに表を分ける。相手に会場の希望を尋ねる
必要がある場合(area が未合意)は、その旨を提案文に含める。

## 5. 予約は人間が行う

このスキルは予約を代行しない。確定した会場は、`performance-planning`
スキル側の手順で `rehearsal.location` に書き戻す(このスキルの責務は
候補を出すところまで)。

## 外部サービスへの書き込みはサニタイズ対象

候補提示や相手への連絡文に、プロジェクト名・リポジトリ名・内部の Issue
番号・内部ファイルパスを書かない(`external-call-scheduling`・
`slot-availability` と同じ理由)。

## 事例

(まだ無し。新しい失敗事例が出たらここに追記する。)
