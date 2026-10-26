# エージェント向けツールの命名と配置

この規約は dotfiles で保守する自作の CLI・hook・skill・共通ライブラリと、その設定・文書に適用する。
新しい対象を追加・改名するときは「名前と責務」「正本と配布」を読み、移行時には[移行手順](tooling-migration.md)も使う。
第三者製 skill・plugin は upstream の名前を使う。疑問や例外は、このリポジトリの PR で議論する。

## 名前は責務を端的に説明する

名前は、その対象の責務を過不足なく、一般に理解できる語で端的に記述する。
適切に名付けられない場合は、責務の切り方を見直す。名詞・動詞など品詞を一律に揃える規則は設けない。
たとえば `gpg-prewarm` は署名と復号を前倒しするため、署名だけを表す名前を使わない。
`presentation-authoring` は発表原稿を正本にし、スライドを派生させる責務を持つ。

| 語 | このリポジトリでの意味 |
|---|---|
| guard | 禁止操作・不正な状態から対象を守る |
| gate | 必要な条件が揃うまで進行・完了を止める |
| check | 対象の適合性を判定する |

この区別は本リポジトリの語彙であり、業界標準とは扱わない。
発火イベントは登録宣言が持つ。PreToolUse・Stop といった時点から接尾辞を決めない。
許可の裏付けを返す `allow`、状態を知らせる `advisory` など、上の三語以外も責務に合えば使う。

## 正本と配布を分ける

共有 skill は `config/agents/skills/`、共有参照資料は `config/agents/reference/`、共有指示は
`config/agents/AGENTS.md`、共有設計文書は `docs/agents/` に置く。
共有の配布・hook 宣言は `home/modules/agents.nix` が持ち、`home/common.nix` から明示的に import する。
製品固有の設定・説明は `config/claude/`・`config/codex/`・`docs/claude/` など製品別の場所に残す。
配布先と対応製品の範囲は、共有正本への移動だけを理由に広げない。

Rust の実装は `crates/<責務名>/` に置く。ライブラリの境界は[責務一覧](agent-libraries.md)を参照する。
各機能が永続状態のパスを所有し、汎用ライブラリに機能別の状態パスを集めない。
意味の異なる解析結果やタイムアウトは、似た API でも区別して残す。

skill の frontmatter の `name` は、その `SKILL.md` を含むディレクトリ名に一致させる。
複数の配布先に同じ補助資料が必要なら、同じ source ファイルを配り、本文を複製しない。
ブラウザのフォーム保存・アクセス障害は `browser-operations.md`、CI 障害は
`repo-governance-common/reference/ci-failures.md`、人手タスクの予定化は `human-task-scheduling` を正本とする。
利用する skill には、その場面で必ず読む指示を残す。

## 変更は配線と検証まで揃える

改名するときは package、実行名、skill 宣言名、配布宣言、内部呼び出し、文書、pipeline-map を同時に更新する。
旧公開 CLI・skill は移行手順の削除条件が揃うまで薄い入口として残す。
履歴や引用の旧名称、永続データのパス、環境変数、記録 ID、承認記録、投稿マーカーは変更しない。
凍結した退役一覧や旧コマンドの照合値を、新名称へ書き換えない。

名前の意味と責務の切り方はレビューで判断する。名前とディレクトリ、配布元の実在、manifest と登録の対応は
Nix 評価と `crates/pipeline-map/tests/coverage.rs` で検査する。

## 出典

- Sameer Ajmani, “Package names”, 2015-02-04, https://go.dev/blog/package-names （取得 2026-10-26）。短く明確な責務名を選ぶ考え方を採用し、Go 固有の綴りは移植しない。
- Google, “C++ Style Guide — Choosing Names”, 公開日記載なし, https://google.github.io/styleguide/cppguide.html#Choosing_Names （取得 2026-10-26）。読者に意味が伝わる名前を選ぶ原則を採用する。
- Agent Skills, “Specification — name field”, 公開日記載なし, https://agentskills.io/specification#name-field （取得 2026-10-26）。ディレクトリ名と name の一致を採用する。
