# claude-routines auditor 手順

これはクラウドの自己監査 routine(`role: auditor` の宣言を持つ routine)の
prompt が指す、汎用の実行手順。private 側の宣言(person-state repo に
置く)は、この dotfiles リポジトリを sources に含め、prompt からこの
ファイルを読んで従うだけにする — 手順そのものをここに一本化する
(`docs/adr/519-routines-declaration-in-repo.md` D6)。

## 実行環境の前提(2026-09-27 実測、段0c)

- クラウド sandbox に `gh` CLI は無い。GitHub 操作は `mcp__github__*`
  MCP tool を使う(`gh` を前提にした手順を書かない)。
- `jq`・`yq` は両方ある。差分計算は `jq` のみで動く
  `config/claude/skills/claude-routines/scripts/routines-plan.sh` を
  そのまま実行できる(dotfiles の clone から相対パスで呼ぶ)。
- GitHub アクセスは routine の `sources` に scope される。この auditor の
  宣言の `sources` に無い repo は読めない — 新しい家 repo を足したら
  `sources` にも追加する必要がある(検出は `github-audit`(`crates/github-audit`)の
  routines ドメインが行う、段3)。
- `RemoteTrigger` に相当する meta connector(`Claude_Code_Remote`)は
  `get_trigger`/`update_trigger`/`list_triggers` を持つ。**`create_trigger`/
  `delete_trigger` はこの auditor からは使わない**(下記「権限の範囲」)。

## 権限の範囲

- **update のみ。create はしない。** 宣言に `id` が無い(new)routine を
  auditor が create すると、書き戻し PR が未マージのまま次回実行が来た
  ときに重複 create が起き、delete API が無いため取り消せない。new
  routine の create は必ずローカルセッション(`SKILL.md`)で行う。
- **自分自身(`role: auditor` の宣言)は報告のみで、update しない。**
  壊れた prompt を自分で直す経路を作ると、直せなくなったときに自己修復
  不能な循環になる。自分の宣言が declared-ahead/live-drift/conflict に
  なったら、通常の routine と同じく Issue を起票するだけに留める。

## 手順

1. `sources` に列挙された各家 repo を(すでに checkout 済みの dotfiles を
   除き)手元にクローンする。
2. dotfiles の `config/claude/skills/claude-routines/scripts/
   routines-plan.sh` を使う。この auditor 自身の宣言(`role: auditor`)は
   手順3の対象に含めない(上記「権限の範囲」)。
3. 各家 repo の `.claude/routines/*.json` を1件ずつ処理する:
   a. `id` があれば meta connector の `get_trigger` で live を取得し、
      `routines-plan.sh classify` で分類する。`id` が無ければ `new` として
      扱い、create はせず報告する。
   b. 分類が `declared-ahead` なら、2行目以降の update body を
      `update_trigger` にそのまま渡す(加工しない)。
   c. 分類が `live-drift`/`conflict`/`suspended`/`refuse`/`new` なら、
      その家 repo に Issue を起票する(本文に分類名・routine 名・
      なぜそう判定したかを書く)。既存の同種 Issue があるか、本文の
      隠しマーカー `<!-- routine-drift:<id-または-name> -->` で検索し、
      あれば新規に起票せずコメントで更新する(重複起票の防止)。
4. `list_triggers` で(取得できるページ分の)cron routine 一覧を取り、
   `routines-plan.sh classify-unmanaged` に通す。出てきた routine は
   宣言を持たない cron routine(`/schedule` 等での場当たり作成、または
   宣言が消えた孤児)。person-state repo に1件だけ Issue を起票する
   (同じ隠しマーカー方式で重複防止)。
5. 実行結果(処理した宣言数・update した件数・起票した Issue)を短く
   まとめ、この routine の run ログに残す(通知は必須ではない)。

## Issue に書く分類ごとの文面の目安

- `live-drift`: 「宣言と一致していた routine が、live 側だけ変わっている
  (UI 等での編集の可能性)。宣言に取り込むか、live を戻すか判断してください。」
- `conflict`: 「宣言と live の両方が変わっている。どちらを正とするか
  判断してください。」
- `suspended`: 「`suspension_reason`/`ended_reason` が設定されている。
  原因を確認してから対処してください。」
- `refuse`: 「routine-spec 注記が無い、または live の name が宣言の
  名前空間キーと一致しない。SKILL.md の adopt 手順をやり直してください。」
- `new`: 「宣言に id が無い未作成の routine があります。ローカルセッションで
  create してください(auditor からは create しません)。」
- `unmanaged`: 「宣言を持たない cron routine が live に存在します。」
