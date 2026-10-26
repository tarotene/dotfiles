# キリ番(Issue / PR 番号のキリ番を祝う)

Issue と PR は同じ番号の列を共有する。その番号がキリ番に当たったら、昔の個人
ホームページの作法で祝う: 祝福のコメント、ラベル、そして大キリ番とゾロ目では
キリリク(キリ番を踏んだ人が、お題をリクエストできる権利)の受付。

遊びの機構であり、required check ではない。落ちても PR や Issue を止めない。

## 規則

単一の正本は `crates/kiriban/src/lib.rs` の `classify`。

| 種類 | 当たる番号 | 例 |
|---|---|---|
| 大キリ番 | 1000 の倍数 | 1000, 2000, 10000 |
| ゾロ目キリ番 | 111 以上で全桁が同じ数字(2 桁の 11, 22 は頻度が高すぎるので除く) | 111, 999, 1111 |
| キリ番 | 100 の倍数(大キリ番を除く) | 100, 1100 |
| 前後賞 | 1000 の倍数の ±1 | 999, 1001, 1999, 2001 |

1 つの番号が複数に当たることがある(999 はゾロ目で、大キリ番 1000 の前賞でもある)。
見出しは 大キリ番 > ゾロ目 > キリ番 > 前後賞 の順で 1 つ選び、ほかに当たった前後賞は
本文に書き添える。

## 起きること

| 種類 | 祝福コメント | ラベル `kiriban` | キリリク受付 Issue |
|---|---|---|---|
| 大キリ番 | ○ | ○ | ○ |
| ゾロ目キリ番 | ○ | ○ | ○ |
| キリ番(100 の倍数) | ○ | ○ | なし |
| 前後賞のみ | ○ | ○ | なし |

- **殿堂入りの一覧** は台帳を持たない。`label:kiriban` の検索が一覧になる
  (`https://github.com/<owner>/<repo>/issues?q=label%3Akiriban`)。人が書き足す
  記録を増やさない。
- **キリリク受付 Issue**(ラベル `kiririku`)の本文は、踏んだ番号へのリンク、ゲッター名、
  お題を書く欄、受付期限(30 日後の日付)。担当者は付けない。閉じるのは人で、お題を
  果たしたときか、期限が切れて捨てるとき。自動では閉じない。
- **bot が踏んだとき**(`user.type == "Bot"` か、ログイン名の末尾が `[bot]`)は
  「踏み逃げ」の文面にし、キリリク権は管理人(リポジトリの owner)預かりと書く。
- **`@` は書かない**。通知は取り消せないので、踏んだ人はログイン名の文字列で書く
  (テストが 1〜20000 のすべての番号で保証する)。

## 入れ子

キリリク受付 Issue 自身も番号を 1 つ使う。それがキリ番に当たったら、同じ run の中で
祝福コメントとラベルだけ付ける(キリリクは開かない。入れ子は 1 段で止まる)。
GITHUB_TOKEN で開いた Issue は新しい workflow を起動しないので、同じ run で処理する。
このとき踏んだのは `github-actions[bot]` なので、文面は「踏み逃げ」になる。

## 冪等

祝福コメントとキリリク受付 Issue は、本文の先頭の印(`<!-- kiriban:N -->`、
`<!-- kiriban-request:N -->`)で既存を探し、あれば作らない。同じ番号を何度
`workflow_dispatch` しても増えない。キリリク Issue が既にあるときは、その本文から期限を
読み直して祝福コメントに書く。

## 仕組み

PR タイトル検査(`.github/actions/pr-title`)と同じ形。

- `crates/kiriban`: 判定と文面の単一正本。依存ゼロの Rust。時刻に依存しない(期限は
  呼び出し側が渡す)
- `.github/actions/kiriban/action.yml`: dotfiles を sparse-checkout して crate を build
  し、`gh` で書き込む composite action
- `.github/workflows/kiriban.yml`: 各リポジトリの小さな workflow。正本は
  `config/agents/skills/repo-governance-common/templates/.github/workflows/kiriban.yml`
  で、`ci.yml` の `kiriban template is in sync` が、dotfiles 自身の複写との一致を
  検査する(違ってよいのは `# pipeline-node:` の 1 行だけ)

起動は `issues: opened`、`pull_request: opened`、`workflow_dispatch`(番号を渡す)。

## 制約

- fork からの PR と Dependabot の PR は、GITHUB_TOKEN が読み取り専用で書き込めない
  ので job ごと skip する(skipped は成功扱い)。`pull_request_target` にすれば書けるが、
  fork のコードに書き込み権限を渡す経路を作るので採らない。取りこぼした番号は
  `workflow_dispatch` で後から祝える。
- Discussion も同じ番号の列を使う。Issue / PR でない番号は、notice を出して何もしない。
- この機能を入れる PR 自身の `opened` では、action が `@main` にまだ無いので job が
  1 回失敗する。以後は起きない。

## 既存のリポジトリに足す

governance の `copy-files.sh` を `--only` で呼ぶと、この workflow の 1 ファイルだけを置く。

```
~/.claude/skills/repo-governance-common/scripts/copy-files.sh \
  --owner <owner> --repo <repo> --dest <checkout のパス> \
  --only .github/workflows/kiriban.yml
```

そのあと commit して PR を作る。新しいリポジトリには、governance の各型(core / rust /
typst / astro)の `copy-files.sh` が、最初から置く。

## 遡って祝う

```
gh workflow run kiriban.yml -R <owner>/<repo> -f number=<N>
```

番号ごとに 1 回ずつ実行する。結果は `gh issue list -R <owner>/<repo> --label kiriban --state all`
と、その番号のコメントで読み戻す。

## 経緯

- 2026-10-25: dotfiles が #999・#1000・#1001 を踏んだのを機に導入した。様式は昔の個人
  ホームページのキリ番文化(キリ番ゲット、踏み逃げ禁止、キリリク)。
