# issue-index — 自分に関係する open Issue の索引だけを注入する hook

曖昧な依頼(「あの不具合直して」)が既存 Issue に接地せず、重複実装・既に決着した設計
判断の掘り返し・進行中 PR との衝突が起きる問題への対処。`CLAUDE.md` に「Issue を確認
せよ」と書いても実行はモデルの気分次第なので、**環境の現在状態は hook で運ぶ**——
静的な指示は `CLAUDE.md`、現在状態は hook、という切り分けを実装する
(`docs/wrapup-inbox.md` と同じ設計原則)。

| 部品 | イベント | 役割 |
|------|----------|------|
| `issue-index.sh` | SessionStart(`startup\|resume\|compact`) | 自分に関係する open Issue の索引(番号・タイトル・ラベル・起票者)と現ブランチの PR を `additionalContext` で注入 |

配備は `home/modules/claude.nix`(スクリプトは `home.file`、`~/.claude/settings.json`
への登録は activation 時の冪等 jq マージ)。全ホスト共通・repo 非依存。

## なぜ「巡回」ではなく「索引」なのか

「全 Issue を巡回して本文を流し込む」は実測すると成立しない:

| リポジトリ | open Issue | @me assign |
|---|---|---|
| `tarotene/dotfiles`(public) | 12 | 0 |
| `arkedge/virde-nt0` | 745 | 43 |
| `arkedge/aocs-all` | 739 | 68 |
| `arkedge/virde` | 146 | 28 |

会社リポジトリは open Issue が 700 件超あり、全件取得は 5 秒以上かかる(セッション冒頭
の予算を壊す)。したがって本文ではなくポインタ(番号・タイトル・ラベル・起票者)だけを
渡し、深掘りは `gh issue view` で Claude 自身に任せる。

## なぜ `gh issue list` ではなく Search API か

`gh api "search/issues?q=repo:{owner}/{repo}+is:issue+is:open+assignee:@me+sort:updated-desc&per_page=15"`
の 1 リクエストで「正確な総数 + 更新の新しい順の上位 15 件 + ラベル + 起票者」が
同時に取れる(実測 0.7〜1.0 秒)。

**`gh issue list --limit N` の `N` は取得上限であり総数ではない。** `--limit 300` を
渡すと 300 件返るが、実際の総数は 739 件だった——これで「N 件のうち 15 件」と言うと
嘘になる。Search API の `total_count` は取得していない分も含めた正確な総数であり、
`sort:updated-desc` も全体に対して効く(ローカルソートでは @me が 15 件を超える repo
で上位が保証できない)。

`{owner}/{repo}` プレースホルダは cwd の git remote から解決されるが、`gh api` には
`gh issue list`/`gh pr list` にある `-R/--repo` が無い。hook プロセスの cwd がフック
実行時にプロジェクトと一致するとは限らないため、`owner_repo()` で git remote から
自前で `owner/repo` を解析し、リテラルな `repo:owner/repo` をクエリに埋め込む
(`gh pr list` の方には `-R` を渡す)。これで cwd に依存せず動く。

### `incomplete_results` を無視してはいけない

Search API は検索が時間内に完了しないと、見つかった分だけを返して
`incomplete_results: true` を立てる(レスポンス最上位キーは
`["incomplete_results","items","search_type","total_count"]`)。このとき `total_count`
と `items` は途中結果なので、そのまま「68 件のうち 15 件」と言うと不正確な数字を
確信度高く提示してしまう。索引そのもの(`items`)は実在する Issue なので捨てず注入
するが、件数の文を「検索が完了しなかったため総数・省略件数は不正確」に差し替え、
stderr にも 1 行出す。嘘の正確な数字を出さないことが「沈黙の切り捨てを作らない」の
実体である(Codex plan-review R2-C-1 で確定)。

### 失敗判定は「両方失敗」では見ない

`@me` 側の Search が 0 件でフォールバックした先(全体側)だけが失敗した場合、
「両方失敗の AND」で判定すると成功扱いになってしまう(Codex plan-review R2-C-2)。
そこで常に「採用しようとした側」を見る:

```
rc_mine != 0              → 失敗(採用しようとした側 = mine自体が読めない)
mine.total_count > 0      → scope=mine(rc_all は参照しない。結果を使わないため)
rc_all != 0                → 失敗(採用しようとした側 = フォールバック先が読めない)
all.total_count > 0        → scope=all
else                        → 対象外(Issue が無い / Issues 機能が無効)
```

`arkedge/harmonia`(Issues 機能を無効化している)は両方の Search が
`total_count=0` を返すので、エラーメッセージ照合なしで自然に「対象外」に落ちる
(実機で確認済み)。

Search API は core とは別枠の **30 req/min** を消費する。1 セッションあたり最大
4 リクエスト(mine / all / pr / viewer)なので、通常運用では枯渇しない。

## 信頼境界: なぜ `--author @me` を使わないか

`labels` は collaborator しか付けられないので信頼してよいが、**タイトルは第三者
(untrusted)が自由に書ける**。`--author @me` で自分の起票だけに絞れば安全になる
ように見えるが、実測では成立しない:

- `arkedge/aocs-all` の `@me` assign 68 件のうち **27 件(4 割)は他人起票**
- 絞ると索引が大きく欠け、「曖昧な依頼を既存 Issue に接地させる」という目的が
  そもそも果たせなくなる

**制御文字除去 + 120 文字切り詰めは防御ではない。** payload を削るだけで、短い命令文
の意味までは消せない(Codex plan-review R1-A-2 / R2 で指摘され、この境界を受け入れる
ことを利用者が確定した)。実際の境界の可視化は行単位で行う: **他人起票の行にだけ
`(起票: <login>)` を付ける**。自分の login は `gh api graphql -f query='{viewer{login}}'`
で並行取得し、取得に失敗した場合は起票者注記そのものを出さない(誤って「全部自分の
Issue」に見せる方が悪いため)。

## 縮退

すべて exit 0(SessionStart は exit 2 でもブロックできず、stderr はユーザーにしか
見えない)。

| 分類 | 条件 | 挙動 |
|---|---|---|
| 対象外 | `jq`/`gh` 不在、git repo でない、GitHub remote が解決できない、採用した scope の Search が `total_count=0` | 完全沈黙(stdout・stderr とも空) |
| 失敗 | 前提が揃っているのに、採用しようとした側の Search が非 0 で終了(認証切れ・rate limit・ネットワーク不通など) | stderr に 1 行、stdout は空 |
| 部分縮退 | 現ブランチの PR 取得、または起票者(viewer login)の取得だけが失敗 | その行/装飾だけを落とし、索引自体は注入する |
| 不完全 | 採用した側が `incomplete_results: true` | 索引は注入するが件数の文を「不正確」に差し替え、stderr にも 1 行 |

git 判定(git repo か・GitHub remote か)は `git remote -v` の静的検査のみで、
ネットワークには出ない(`docs/wrapup-inbox.md` と同じ流儀)。**未認証はここでは「対象外」
に分類しない**——`gh api` は認証切れだと非 0 終了するので、上の「失敗」バケットに
自然に落ちる(ドキュメント上の想定と実装が一致する)。

## PR 行

現ブランチに対応する open PR を 1 行だけ足す。herdr の worktree はブランチ 1 本 =
タスク 1 本なので、今のセッションに関係する PR は高々 1 件になる。

「PR が無い」(問い合わせは成功したが配列が空 → 「なし」と書く)と「PR の有無が
分からない」(問い合わせ自体が失敗 → 行を出さない)を区別する。混同すると、実は
問い合わせが失敗しているのに「なし」と確信度高く誤報することになる。

## `clear` を外し `compact` を含める理由

`/clear` は「文脈を捨てたい」という利用者の意思表示。`compact` は逆に文脈を**続けたい**
意思表示で、要約後は索引が落ちている可能性が高く再注入の価値が最も高い。
`autoCompactEnabled: false` の設定では発火は手動 `/compact` 時のみ。`fork` は元セッション
の文脈を引き継ぐので不要。

## `register` の既知の制約

`home/modules/claude.nix` の `registerHooks` は command の一致だけで存在判定する
冪等マージなので、**matcher を後から変えても既存エントリは更新されない**。
matcher を変更する場合は `~/.claude/settings.json` の該当エントリを手で消してから
`home-manager switch` する。

## 検証

- `bash config/claude/hooks/issue-index.sh --selftest` — 縮退ゲート・
  mine→all フォールバックの非対称な失敗判定・`incomplete_results` の扱い・
  タイトルの sanitize・起票者注記・PR 行の状態区別の回帰テスト(CI の `ci.yml`
  でも実行)。
- 手動 E2E: `printf '{"cwd":"%s"}' "$PWD" | bash config/claude/hooks/issue-index.sh`
  で実際のリポジトリに対する索引を確認できる。新しいセッションを開いて
  system reminder に `[issue-index]` ブロックが入ることも確認する(fail-open は
  壊れても気付きにくいため)。
