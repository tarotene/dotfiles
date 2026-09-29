# venue-search — 会場探しの判断知識と URL 生成

スクリプト: `config/claude/skills/venue-search/scripts/venue-urls.sh`
スキル: `config/claude/skills/venue-search/SKILL.md`
関連: 別の private な person-state リポジトリ側の ADR-0015
(演奏企画の付随作業の定型化。出典は private リポジトリ側のため、ここでは
決定の存在だけを参照する)

合わせ・個人練習の会場探しは、これまで Instabase・SpaceMarket・Google
検索・使い慣れた店舗(ノア等)を場当たりで巡回する形で行っていた。
コーディングエージェント経由で行う以上、エージェントが空き状況を機械的に
確認できる経路を優先し、予約はコストの低い方法(Web 予約が中心)で人間が
行う分担にする。

## なぜ公式 API 前提を諦めたか

2026-09-27 時点の調査で、Instabase・SpaceMarket・ピアノスタジオノア・
23区の施設予約システム・スペイシーのいずれにも公開 API は見つからなかった
(規約本文・robots.txt・開発者ページを確認、二次情報の裏取りのみで一次
情報が無いものは「見つからず」として扱った)。このため、当初の希望
(API アクセスできるサービスを優先)は次点(エージェントが探すところまで
はでき、人間の予約が低コストで済むサービス)に切り替えた。

## なぜ URL 生成を決定的スクリプトに切り出したか

`slot-availability` の判定コア(`scripts/slot-hit.sh`)と同じ理由 — 「どの
サービスを見に行くべきか」を毎回その場の会話で組み立てるとブレが出る。
`state/places.toml`(登録済みの公共施設・常用スタジオ)とピアノ要件の
有無という、決定的に扱える入力から出力できる部分(見に行くべき URL の
列挙)はスクリプトに固定し、実際のページ取得・空き確認・候補の絞り込み
という文脈判断を要する部分だけを Claude が担う。

## ハブは新設しない

`state/places.toml` は演奏企画のデータ(`state/performances/`)と同じ
person-state リポジトリに置く裁定になっている。このスキル専用のハブ
マーカーを新設せず、`performance-planning` の `scripts/performance-hub`
をそのまま流用する — 同じリポジトリを指すマーカー方式を2つ持つ理由が
無い(還元性、`docs/claude/writing-style.md` のマーカー方式と同じ設計
判断の型)。

## ログイン要否で経路を分けた理由

Claude Code(Playwright/WebFetch)はログインが要るページの先を見に行く
ことが原理的に難しい(認証情報の受け渡し自体が secrets 衛生の問題になる)。
このため空き確認はログイン不要のページに限定し、予約(ログインが要る
操作)は人間に委ねる設計にした。`venue-urls.sh` の出力に
`login_required` を持たせ、`false` のものを優先して見に行く手順にした
理由はここにある。

## ノアの DOM 手順をコードとして保全した理由

会場探しの初回実運用(2026-09-28、オーボエ協奏曲の合わせ4回)で、ノアの
予約検索ページ(JS 描画・共有不可能な URL)を Playwright で操作して空きを
確認した。同じ DOM セレクタ・抽出ロジックを次回また一から探索し直すのは
無駄なので、`scripts/noah-chart.js` に切り出し、`SKILL.md` の「3.1」に
手順として固定した(セレクタ自体は decidable な事実であり、探索コストを
払い続ける理由が無い — `slot-availability` が判定ロジックを
`scripts/slot-hit.sh` に固定した設計と同じ動機)。ただし DOM 構造はサイト
側の変更で壊れうるため、実行時にセレクタが見つからない場合は素直に
再探索して `noah-chart.js` を更新する(コードは「今分かっている近道」を
記録するだけで、サイト仕様そのものを制御できるわけではない)。

同じ実運用で、週送りボタンが今日から約13週先で `_disabled` になる
(ノア側の表示可能範囲の上限)ことも分かった。この事実は「空きが無い」
と混同すると誤報告になるため、SKILL.md の手順6に明文化した。

## 提示規則は person-state リポジトリ側に一元化した(ADR-0017)

「合わせが確定していて会場未定のとき、何を・どう相手に提示するか」という
規則(例: 既定会場をコマによってそのまま提示するか、前後の予定を確認して
から提示するか)は、`state/performances/scripts/obligations.sh` が
`venue-booking` の閉じた導出フィールド `stage`(`book`/`propose`/
`confirm-first`/`agree-area`)として既に計算済みで返す。このスキルは
`stage` の値で分岐するだけで、規則そのものをここで再導出しない —
規則の正本を 2 箇所に持たない設計(`slot-availability` の判定コアを
person-state 側に置かない、という既存の分担の裏返し)。

## 参照

- person-state リポジトリ側の ADR-0015(演奏企画の付随作業の定型化)・
  ADR-0017(合わせ会場の提示方針)。このスキルが読み書きするデータモデル
  の決定文書。出典は private リポジトリ側のため、ここでは決定の存在だけ
  を参照する。
- person-state リポジトリ側の `state/places.toml`(会場探しの起点データ)
- `docs/claude/performance-planning.md`(ハブ解決の流用元)
- `docs/claude/slot-availability.md`(判定コアの分離という同型の設計判断)
