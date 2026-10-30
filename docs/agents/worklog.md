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

行の形は `- HH:MM [kind] 本文 (owner/repo#N)`。kind は `reply|deadline|task|pr|found|stuck|mail|slot`。

## mail — 読んだメールの出来事と、未裁定項目の状態

決定の記録: `docs/adr/1256-worklog-mail-kind.md`(ADR-998 の追記)と、それを改める
`docs/adr/1259-mail-lifecycle.md`。状態機械・時間の決まり・終端の控えは `docs/agents/mail-lifecycle.md`。

メールを読んだセッションが、出来事と出典の印だけを残す kind。本文は次の形で、コードが検査する
(外れる、または今の状態から許されない出来事は、書かずに止まる)。

```text
worklog add mail "<出来事>: <要旨> [<媒体>:<スレッド>/<受信> [due:YYYY-MM-DD]]"
worklog mail open | check | sweep [--dry-run] | restore <鍵> | forget <鍵>
```

- 出来事(閉語彙。正本は `config/agents/mail-lifecycle/vocabulary.json`): `read`・`ask`・`draft`・`decide`・
  `apply`・`pass`・`move`。`remind`・`stale` は `mail sweep` だけが書く。
- 要旨: 結論だけ(相手は組織・サービス名か種類、何を決めたか)。120 文字まで。`@`・`http`・角括弧を含められない。
- 出典の印: 媒体は `gmail|linkedin|lapras|none`。鍵は `<媒体>:<スレッド>/<受信>`(`[A-Za-z0-9_=-]+`。
  Gmail の受信はメッセージ ID、それ以外は `YYYYMMDDTHHMM`)。`none` は件数を集計する `read` の行 `[none]` にだけ使う。
- 鍵が冪等の鍵。同じ受信を読み直しても項目は増えず、同じ出来事は記録済みになる。
  mail には owner/repo の参照を付けない(作業ディレクトリが違っても、同じ行は重ならない)。
- 「どのスレッドを処理済みか」は `worklog mail open`(未了)と `worklog list --kind mail --since YYYY-MM-DD`(全部)で引く。
- メールの本文・件名・相手の名前・アドレスは書かない。それらは Gmail に残り、ここには結論と鍵だけを置く。
  既存の記録(Gmail の下書き、PR のマージ状態、カレンダー)から導けることを書かない規範は、そのまま保つ。

## slot — 作業の枠(自分一人でやる、時間枠つきの作業)の出来事と状態

決定の記録: `docs/adr/0000-task-slot-worklog.md`。状態・行の形・CLI は `docs/agents/task-slot.md`。

`mail` と同じ作りで、`[slot]` 行を出来事として畳み込む(`plan`・`resched`・`done`・`drop`)。時間枠(`start:`・
`end:`。ISO 8601 のオフセット付き)と参照 Issue は行に持ち、本文・個人名は書かない。タイマーは無く、
終了を過ぎたものは `worklog slot overdue` が導く。既存の自由記述の `task` kind とは別の kind である。

```text
worklog add slot plan <鍵> --start <ISO 時刻> --end <ISO 時刻> [--issue owner/repo#N …]
worklog slot open | overdue [--now <ISO 時刻>] | check
```

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
