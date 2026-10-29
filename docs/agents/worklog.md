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
worklog session-start [--agent claude|codex|copilot]   # SessionStart hook。手で打つ必要は無い
```

- `add` は当日ノートの `## 作業ログ` 節の末尾に 1 行を足す。`owner/repo` は作業ディレクトリの
  `remote.origin.url` から付く。git の外では参照が付かず、`--ref` を渡すなら `--repo` も要る。
- `list` は `日付<TAB>時刻<TAB>kind<TAB>本文<TAB>参照` を出す(無い項目は `-`)。既定は今日を含む直近 7 日。
  kind の無い行は `memo`。以前の見出し `## 済んだこと` の行も読む。
- 本人は Obsidian で同じ節に、kind も時刻も付けずに書いてよい。

行の形は `- HH:MM [kind] 本文 (owner/repo#N)`。kind は `reply|deadline|task|pr|found|stuck|mail`。

## mail — 読んだメールの裁定の結論

決定の記録: `docs/adr/0000-worklog-mail-kind.md`(ADR-998 の追記)

メールを読んで裁定したセッションが、結論と出典の id だけを残す kind。本文は次の形で、コードが検査する
(外れると書かずに止まる)。

```text
worklog add mail "<処分>: <要旨> [<媒体>:<id>]"
```

- 処分(閉語彙): `read`(読んだ。何もしない・片付いている)、`draft`(返信の下書きを作った)、
  `pass`(断る・返さないと決めた)、`decide`(通知への判断を決めた)、`apply`(正本へ反映した。予定、フィルタ規則など)。
- 要旨: 結論だけ(相手は組織・サービス名か種類、何を決めたか)。120 文字まで。`@`・`http`・角括弧を含められない。
- 出典の印: 媒体は `gmail|linkedin|lapras|none`、id は `[A-Za-z0-9_=-]+`(Gmail や LinkedIn のスレッド id)。
  `none` は id を付けない `[none]` で、「約 N 件を読んだ」のような集計の行にだけ使う。
- 出典の印は冪等の鍵。同じ kind・本文の行は時刻が違っても重ならない。同じスレッドでも処分が違えば別の行。
  mail には owner/repo の参照を付けない(作業ディレクトリが違っても、同じ行は重ならない)。
- 「どのスレッドを処理済みか」は `worklog list --kind mail --since YYYY-MM-DD` の出力を id で grep して引く。
- メールの本文・件名・相手の名前・アドレスは書かない。それらは Gmail に残り、ここには結論と id だけを置く。
  既存の記録(Gmail の下書き、PR のマージ状態、カレンダー)から導けることを書かない規範は、そのまま保つ。

## vault の場所の解決

hub-resolve と同じ間接参照で、環境変数 `OBSIDIAN_VAULT`、無ければ
`${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/obsidian-vault` の 1 行目(絶対パス。wrap-up の inbox と共有する単一の正本)。マーカーは
`obsidian.nix` が `vaultPath` から置くので、vault を宣言した機体にしか無い。存在確認は共有の
`<vault>/.obsidian/app.json` で、worklog はさらに `<vault>/.obsidian/daily-notes.json` から、
当日ノートの置き場と書式を読む。

## SessionStart の注入

- イベントは `startup` だけ。Claude Code、Codex、Copilot CLI のセッションに渡す(ADR-998 の 2026-10-28 の追記)。
  Codex は共通の hook 宣言から展開される。Copilot は `home/modules/agents.nix` が `~/.copilot/settings.json` の
  `sessionStart` に `--agent copilot` つきで登録する。出力の形は、Claude Code と Codex が `hookSpecificOutput` の
  入れ子、Copilot が最上位の `additionalContext`。
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
