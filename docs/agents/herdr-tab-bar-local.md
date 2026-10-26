# herdr-tab-bar-local — タブバー右側に、ホストごとの 1 行を出すスロット

Herdr のタブバー右側(`ui.tab_bar_right`)には、claude-usage(`docs/claude/claude-usage.md`)の
ほかに、ホストごとに中身が違う 1 行を出したい場面がある。その中身は、このリポジトリ(public)に
名前を書けない道具が作ることがある(ADR-0034: 私的な実値は書かない、ADR-0025: 自作の
pre-release CLI の存在を書かない)。

そこで public 側には、**中身を知らない機構**だけを置く。

## 何をするか

`config/herdr/config.toml` の `tab_bar_right` に、次の command エントリが 1 件ある。

```toml
{ type = "command", command = "\"$HOME/.config/herdr/tab-bar-right.local\"", interval_seconds = 60, timeout_seconds = 10 }
```

Herdr は 60 秒ごとに `/bin/sh -lc` でこのファイルを実行し、**成功したときの出力の最終行**を
タブバーに表示する(仕様は `docs/claude/claude-usage.md`「表示先」節と同じ)。

- ファイルが無い、実行できない、失敗する、出力が空、timeout する: このセグメントだけが消える。
  hostname と時計は残る。
- 出力は ANSI を含まない 1 行にする。

ファイルは git 管理の外にある。配備するのは各ホストの持ち主で、個人機では private 側の
宣言が、それ以外のホストでは手で置く。`config/` にも `home/` にも、このファイルの中身や
それが呼ぶ道具の名前は書かない。

## 無音の故障になる

この機構は、故障が症状としてしか出ない。ファイルが無いのも、中身が壊れているのも、道具が
失敗するのも、見た目は同じ「何も出ない」になる。#89(通知の `delivery` が `off` のまま無音で
再試行を続けていた)と同じ形である。

だから、実体の側で「ゼロ件」と「読めていない」を別の出力にする。空出力をゼロ件の意味に使うなら、
読めていないときは空にせず、読めていないと分かる文字列を出す。

## 撤去する条件

スロットを作った理由は、呼び先の道具が公開されるまで名前を書けないことだけである。その道具が
公開され、public に名前を書けるようになったら、`config/herdr/config.toml` に直接コマンドを書き、
このスロットと各ホストの実行ファイルを消す。dotfiles 側で変えるのはこの 1 件のエントリとこの文書
だけで、ADR-0025 の D4 と同じ向きの撤去である。

## 根拠

- `docs/adr/0025-update-own-tools-local-registry.md` D2・D4: 登録はホストローカルに置き、
  対象側に痕跡を残さない。公開に達したら登録を消す。
- `docs/adr/0034-machine-state-wrapper-flake.md`: 規則・スキーマは public、実例化した具体値は private。
- Herdr の command 仕様(取得 2026-10-14): ui.tab_bar_right
  https://raw.githubusercontent.com/herdrdev/herdr/v0.9.4/docs/next/website/src/data/config-reference.json
