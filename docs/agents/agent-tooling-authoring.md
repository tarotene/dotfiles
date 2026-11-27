# agent-tooling-authoring スキル

`config/agents/skills/agent-tooling-authoring/SKILL.md` — 作業セッションで得た知見をこの公開リポジトリに定着させるときに発動する、スキルを育てるためのメタスキル。

## 動機

`diagramming` スキルを新設する際、以下の判断を都度検討し直す必要があった:

- この知見はスキル・フック・CLAUDE.md のどれに配置すべきか
- dotfiles 側の配線(ファイル配置 → `home/modules/agents.nix` への追加 → 設計ドキュメント → switch → 動作確認)の手順
- 公開リポジトリに書く以上、社内固有の情報(社名・製品名・URL 等)をどう除去するか

これらは別の知見を蓄積するときにも毎回発生する判断であり、メタスキルとしてまとめておけば同じ検討を繰り返さずに済む。

## サニタイズ規則の正本をここに置く理由

正本は 2 つの範囲を対象とする。公開リポジトリに書く知見(§3.1)と、カレンダー・メール等の外部サービスへ送る文(§3.2)である。後者は以前 `human-task-scheduling` が事実上の本体を持ち、`music-performance-planning`・`venue-search`・`slot-availability` が禁止項目を複写していた。禁止項目の一覧を §3.2 の 1 か所に集約し、各スキルは参照のみとした。機械検査(`bleep`)の配線は `docs/agents/public-publish-guard.md` にある。

サニタイズすべき対象(固有名詞・URL・実データ等)は知見の内容に関係なく共通のチェックリストになる。各スキルの `cases.md` にこの規則を書くと、スキルが増えるたびに同じ内容が複製され、規則を更新するときに全ファイルを修正する必要が生じる。`agent-tooling-authoring` 側に正本を置き、各スキルの事例集からは「追記時は `agent-tooling-authoring` を参照」と 1 行で指すだけにすることで重複を防いでいる。

## 新しい skill が語彙に名指しされる条件(ADR-1293)

配線チェックリスト(§2)の 4 つ目に、「新しい skill は、自分が担う事象または前提を満たす事象を、対象の語彙 JSON の `carriers`・`gates`(本人に見せる状態なら `shown_by`)に `skill:<名前>` として名指しする」を置いた。別リポジトリの対象を扱う skill は `config/agents/vocabulary/external-objects.json` に名前を追加する。どこからも名指しできない skill は、関連する上位 skill への統合候補になる。
skill が増えて「どの目的でどの skill を使うか」が一覧から見えにくくなったことへの歯止めであり、未登録の skill は `scripts/vocab-graph-check --skills` が列挙する。この検査は全 skill を記載し終えるまで CI の必須ジョブにはしない(語彙の書き方と検査は `docs/agents/state-vocabulary.md`)。

## 棚卸しをスコープ外とした理由

既存スキルの肥大・重複・陳腐化を定期的に点検する「棚卸し」機能も検討したが、発動契機が「知見をスキル化したい」という単発のイベントとは異なり、定期実行(例: `/loop`)向けの機能になる。同じスキルの `description` に単発イベントと定期タスクの両方のキーワードを持たせると発動精度が落ちるため、必要になった時点で別スキルとして切り出す方針にした。
