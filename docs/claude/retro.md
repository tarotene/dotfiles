# 作業終了時のレトロ(ADR-0000)

PR を作ったセッションの最初の Stop で、`wrapup-stop-gate` がセッション全体の振り返りを
求める。出口(inbox → Issue 化 → `/wrapup-chores`)は [wrap-up inbox](wrapup-inbox.md) と
共有し、入口と強制だけが別。決定の理由と代替案は
[ADR-0000](../adr/0000-retro-stop-gate.md)。

## 流れ

1. PR を作成する(gh-edit-allow の台帳に `pr ` 行が積まれる。Codex は台帳が無いので現ブランチの
   PR のうちセッション開始後に作られたもの)。
2. 次の Stop で `kind="retro"` のポインタが出る。モデルは
   `wrapup-stop-gate --retro-procedure <session_id>` を実行して手順書を読む。
3. 手順書には、行に引くべき出来事の id(`verdict:…` / `pr-gate` / `memory:…`)と、
   ユーザー発言の一覧(Claude のみ)が並ぶ。モデルは `--retro-add` で行を書く。
4. 全行を kind ごとの表にして PR に 1 件コメントする(先頭は `<!-- wrapup-retro -->`)。
5. `--retro-close <session_id> <comment-url>` が `gh api` でコメントを読み戻して完了にする。

## 行の語彙

| 欄 | 値 |
|----|----|
| `kind` | `user-correction` / `insight` / `friction` / `gate-hit` / `none` / `skipped` |
| `disposition` | `inbox` / `issue:#N` / `none:<理由>` |
| `mechanism` | `prose` / `script` / `gate` / `existing:<名前>` / `none:<理由>`(user-correction・friction・gate-hit で必須) |

`none` と `skipped` は `disposition: none:<理由>` が必須。`skipped` は `quote`(ユーザーの
発言の逐語)も必須。`existing:<名前>` は、その名前が `what` に書かれていなければ受け付けない。

## 状態ファイル

`${WRAPUP_RETRO_DIR:-${XDG_STATE_HOME:-~/.local/state}/claude/wrapup/retro}/<session_id>.*`

| 拡張子 | 内容 |
|--------|------|
| `jsonl` | 行(`--retro-add` が検証して追記) |
| `count` | retro の block 回数(上限 3) |
| `escalated` | 上限に達して警告を出した印 |
| `transcript` | Stop 時に渡された transcript のパス(手順書がユーザー発言を抽出する) |
| `closed` | 完了の印(`--retro-close` が読み戻してから書く) |

## 限界

- `gate-events.jsonl` は session_id を持たないので、突き合わせの対象にできない。
- ユーザーの指摘を見落とさないかは、発言一覧の提示とモデルの判断に頼る(gate は判定しない)。
- 上限(3 回)に達すると警告 1 回で通る。放置を防ぐだけで、強制し続けはしない。
- PR の無いセッションは自動では発火しない。`/retro` で手動に踏む。
