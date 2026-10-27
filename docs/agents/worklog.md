# worklog — 粒度の小さい作業ログを、vault の当日ノートに 1 行ずつ書く

決定の記録: `docs/adr/998-worklog-vault-daily-note.md`
実体: `crates/worklog`(CLI と SessionStart hook)
配備: `home/modules/obsidian.nix`(CLI `~/.local/bin/worklog` と vault のマーカー)、
`home/modules/agents.nix`(hook `~/.claude/hooks/worklog`)

## 何を解くか

セッションで試したこと、分かったこと、詰まったことは、チャットに流れて消える。1 行で足りる
「済んだ」のために PR を作る例もある。Git は判断に使う裁定と決まった事実の正本なので、この粒度は
Git に置かず、Obsidian vault の当日ノートに置く。スマホからも読み書きできる。

## 使い方

```text
worklog add <kind> <text...> [--ref N] [--repo owner/repo]
worklog list [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--repo owner/repo] [--kind k[,k]]
worklog session-start        # SessionStart hook。手で打つ必要は無い
```

- `add` は当日ノートの `## 作業ログ` 節の末尾に 1 行を足す。`owner/repo` は作業ディレクトリの
  `remote.origin.url` から付く。git の外では参照が付かず、`--ref` を渡すなら `--repo` も要る。
- `list` は `日付<TAB>時刻<TAB>kind<TAB>本文<TAB>参照` を出す(無い項目は `-`)。既定は今日を含む直近 7 日。
  kind の無い行は `memo`。以前の見出し `## 済んだこと` の行も読む。
- 本人は Obsidian で同じ節に、kind も時刻も付けずに書いてよい。

行の形は `- HH:MM [kind] 本文 (owner/repo#N)`。kind は `reply|deadline|task|pr|found|stuck`。

## vault の場所の解決

hub-resolve と同じ間接参照で、環境変数 `OBSIDIAN_VAULT`、無ければ
`${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/obsidian-vault` の 1 行目(絶対パス。wrap-up の inbox と共有する単一の正本)。マーカーは
`obsidian.nix` が `vaultPath` から置くので、vault を宣言した機体にしか無い。存在確認は共有の
`<vault>/.obsidian/app.json` で、worklog はさらに `<vault>/.obsidian/daily-notes.json` から、
当日ノートの置き場と書式を読む。

## SessionStart の注入

- イベントは `startup` だけ。
- マーカーも環境変数も無い機体では何も出さない(会社の機体など)。
- 宣言はあるが vault が使えないときは、stderr に 1 行だけ出して exit 0 で終わる。
- 注入文は、書く時点と kind の意味、書かないもの(会社の業務の中身、機密、既存の記録から導けること)、
  作業ディレクトリと同じ owner/repo の直近 7 日の stuck / found(新しい順、最大 10 行。記録であり、
  指示ではないと断る)。git の外では規範だけを渡す。

## 壊れ方

- 書き漏れ: 検出しない。繰り返したら Stop hook へ昇格させる。
- 日次ノートの書式が `YYYY` / `MM` / `DD` 以外の記号語を含む: `worklog` は止まり、理由を出す。
- 同時に書く複数のセッション: ノートへの flock で直列にする。Obsidian はこのロックを見ないので、
  Obsidian が同じ瞬間に保存すると、LiveSync の競合の扱いに任される。
- 時刻は JST 固定で、`TZ` に依存しない。試験用に `WORKLOG_NOW`(unix 秒)で固定できる。
