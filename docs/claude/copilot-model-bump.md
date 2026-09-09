# copilot-model-bump スキル

`config/claude/skills/copilot-model-bump/SKILL.md` — 外部 AI CLI(GitHub Copilot CLI 等)に `--model` で固定 pin してある具体モデル ID を、新モデルの GA・旧モデルの廃止に追従して更新するときに発動する判断知識。

## 動機

`copilot-plan-review.sh` の critic は `COPILOT_PLAN_REVIEW_MODEL` で `gpt-5.6-sol` を明示 pin していた(#80 の Codex → Copilot CLI 移行時に導入)。「Copilot Review Hook をアップデートしてほしい」という依頼を受けて `gpt-6-astra`(2026-09-04 GA)へ bump した際、変更対象は次の3種にまたがっていた:

1. 呼び出し本体の既定値(env var フォールバック)
2. selftest 内の固定値 3箇所(既知値へ固定し直すブロック・偽 copilot が検証する契約値・チェックのラベル文字列そのもの)
3. 設計ドキュメント(呼び出し例・環境変数表・「なぜこのモデルか」の記述)

1箇所だけ書き換えて selftest を流すと、ラベル文字列に埋め込まれた期待値だけが古いまま残っても検出されない(ラベルと期待値の両方が同じ古い ID を指していれば `check` は通ってしまう)ため、grep による棚卸しを最初のステップとして固定した。

## スキルにした理由

この bump 作業は、pin を持つ他の外部 CLI 連携(将来増える可能性がある)にも再発する定型作業であり、かつ「pin 箇所を漏れなく洗い出す」「上流の GA・廃止情報を一次資料で確認する」は機械的に強制できない判断知識である。フックではなくスキルとして `~/.claude/skills/` に置いた。

## 内容の抽象化について

SKILL.md 本文は `copilot-plan-review.sh` 固有の記述を避け、「呼び出し本体 / selftest / ドキュメント」という一般化した3分類と、CLI 非依存の検証パターン(無効モデル ID は送信前に即時エラーになることを利用したスラッグ確認)で書いている。初出の実例(gpt-5.6-sol → gpt-6-astra)だけを `home/modules/claude.nix` のコメントに残し、スキル本体が特定の hook に縛られないようにした。

## 運用

新しい bump が発生するたびに SKILL.md 自体を書き換える必要はない想定。もし別の外部 CLI 連携(pin の持ち方が大きく異なるもの)で本手順が当てはまらないケースに遭遇したら、その差分を本文に追記する。
