# pr-hygiene-check — 自分の open PR の汚れを、決定論で見つける

`crates/pr-hygiene-check`(bin)。自分が作った open PR のうち、履歴や mergeable の構造が汚れたものを**見つけるだけ**の
CLI で、直さない。直す手順は `stacked-pr` スキル §4「セッションの外で作った open PR の汚れを直す」。入口は、朝の
机(person-state リポジトリの手順の段 5。非公開なので、ここには書かない)の「汚れの掃除」。

## なぜ必要か

自分の open PR は、時間が経つと汚れる(2026-11-15 の実測: 予定データのように、よく動くファイルを触る PR 3 本が
`DIRTY / CONFLICTING`。衝突した PR は CI も回らない)。既存の仕組みは、どれもセッションの外で作った open PR の状態を
見ていなかった。

| 既存の仕組み | 見ているもの | 足りない点 |
|---|---|---|
| `pr-gate`(`G_stack`・`G_base`) | 今のブランチの PR、または今のセッションの連鎖 | 他の PR を見ない。`docs/agents/pr-gate.md` が、base の遅れを block にしない(rebase・force-push を始めさせない)と決めている |
| `issue-inbox --collect` | 今のリポジトリの open PR(番号・題名・head・更新日・ファイル) | 母集団が 1 リポジトリ。base・mergeable・コミットの履歴を持たない |
| `github-audit` | 1 つの持ち主のリポジトリの設定 | 母集団がリポジトリで、PR ではない |
| `stack-base-guard` | `gh pr create`・`gh pr edit --base`・マージの実行前 | 作成時の予防で、事後の汚れを見つけない |

## 分類(閉じた語彙)

| 分類 | 意味 | 見つけ方(GraphQL) |
|---|---|---|
| `conflict` | base と衝突している | `mergeable = CONFLICTING` |
| `merged-ancestor` | マージ済みの別の PR に属するコミットを含む(下の段が squash でマージされた後の「汚染 diff」。ADR-0027) | コミットの `associatedPullRequests` に、自分以外の `merged` な PR |
| `base-gone` | base ブランチがない | `baseRef = null` |
| `merge-commit` | 親が 2 つ以上のコミットがある | `parents.totalCount > 1` |
| `behind` | base に遅れていて、最新化が必要 | `mergeStateStatus = BEHIND` |
| `unknown` | 判定できなかった | `mergeable = UNKNOWN` のまま、コミットが 100 を超える、問い合わせの失敗 |

積み上げた PR(同じリポジトリで、ある PR の base が別の PR の head)は、下の段が汚れていれば上の段に `below`(根本原因となった
PR の番号)を付ける。上の段は下の段を直すと積み直しが必要になるため、汚れに数える。出力は、リポジトリ、段の深さ、
番号の順(下の段から直す順)となる。

終了コードは、0 = すべて clean、1 = 汚れあり、2 = 判定不能(明確な汚れがなく、`unknown` だけが残った。持ち主の
表・`gh` の失敗・引数の誤りも 2)。

## 母集団は、同じ側の持ち主だけで作る

`gh search prs --author @me --state open --archived=false --owner <持ち主>…` の `--owner` に、`repo_scope::Policy::
same_side_owners()`(この機体の契約と同じ側の持ち主。`docs/agents/repo-scope-guard.md`)だけを渡す。別の側の
持ち主には拒否に頼らず最初から問い合わせない。検索が返した結果も持ち主で絞り直す(絞りが効かなかった場合の
二重の防御)。`--owner` を指定できるのはその側の持ち主の中だけで、別の側の持ち主は引数の段階で拒否する。

持ち主の表を配布していない機体は規則がないため、指定した持ち主か、ログイン中のアカウント自身(`gh api user`)を使う。

## 実装の要点

- 検索 1 回(`--limit 1000`)で PR の一覧を取得し、リポジトリごとに GraphQL 1 回(別名 `p<番号>: pullRequest(number: N)`、
  最大 40 件ずつ)で構造を取得する。コミットは先頭の 100 件、各コミットの `associatedPullRequests` は先頭の 5 件。
  100 件を超える PR は `unknown`(確認していない分を clean とは言わない)。
- `mergeable` は、GitHub が計算するまで `UNKNOWN` で返る(2026-11-15 に、1 回目が `UNKNOWN`、再取得すると `DIRTY` になるのを
  実測した)。計算中のものだけを 2 秒待って最大 3 回再取得する。
- `gh` と待ち時間は、`Gh` トレイトと `sleep` 関数として注入できる。判定(分類・積み上げの位置・並び・出力)は純関数で、
  記録した JSON fixture でテストする。

## 限界

- 閉じた PR・アーカイブ済みリポジトリの PR は見ない。
- fork から出した PR は、`--author @me` で見つかっても、base 側リポジトリの持ち主が表になければ母集団に入らない。
- 「直したあとに clean か」は、GitHub の `mergeable` の計算に依存する。修正直後は `UNKNOWN` になる可能性があるため再取得する。
- 汚れの有無だけを言う。意図して開いたままの PR(`DO-NOT-MERGE`・draft)も報告する。直すかは、呼び出し側が本人に聞く。

## 既存手段と代替

- 既存手段: 自前 — 却下: `issue-inbox --collect` の拡張(母集団が今のリポジトリ 1 つ)、`github-audit` の新しいドメイン
  (母集団がリポジトリ)、`gh stack sync`(force-push の不具合の記録があり、積み直しは素の git にしている。
  `docs/agents/stacked-pr.md`)。
- 先行例: `docs/agents/pr-gate.md`「G_base / G_worktree を block にしない理由」の「検出は決定的なスクリプト、履歴の改変は
  人が明示的に起動する入口に任せる」をそのまま採る。
