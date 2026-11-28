# herdr-tab-bar-local — タブバー右側にホストごとの 1 行を出すスロット

Herdr のタブバー右側(`ui.tab_bar_right`)には、claude-usage(`docs/claude/claude-usage.md`)のほかに、ホストごとに内容が異なる 1 行を出したい場面がある。その内容は、このリポジトリ(public)に名前を書けないツールが生成することがある(ADR-0034: 私的な実値は書かない、ADR-0025: 自作の pre-release CLI の存在を書かない)。

そこで public 側には、**内容に依存しない機構**だけを置く。

## 何をするか

`config/herdr/config.toml` の `tab_bar_right` に、次の command エントリが 1 件ある。

```toml
{ type = "command", command = "\"$HOME/.config/herdr/tab-bar-right.local\"", interval_seconds = 60, timeout_seconds = 10 }
```

Herdr は 60 秒ごとに `/bin/sh -lc` でこのファイルを実行し、**成功したときの出力の最終行**をタブバーに表示する(仕様は `docs/claude/claude-usage.md`「表示先」節と同じ)。

- ファイルがない、実行できない、失敗する、出力が空、timeout する: このセグメントだけが消える。hostname と時計は残る。
- 出力は ANSI エスケープを含まない 1 行にする。

ファイルは git 管理の外にある。配置するのは各ホストの所有者で、個人機では private 側の宣言が配置し、それ以外のホストでは手動で配置する。`config/` にも `home/` にも、このファイルの内容やそれが呼ぶツールの名前は書かない。

## 無音の故障になる

この機構は故障が症状としてしか出ない。ファイルがない場合も、中身が壊れている場合も、ツールが失敗する場合も、見た目は同じ「何も表示されない」になる。#89(通知の `delivery` が `off` のまま無音で再試行を続けていた)と同じ形である。

そのため、実体側では「ゼロ件」と「読めていない」の出力を分ける。空出力をゼロ件の意味に使うなら、読めていないときは空にせず、読めていないと分かる文字列を出力する。

## 撤去する条件

スロットを作った理由は、呼び先のツールが公開されるまで名前を書けないことだけである。そのツールが公開され、public に名前を書けるようになったら、`config/herdr/config.toml` に直接コマンドを書き、このスロットと各ホストの実行ファイルを削除する。dotfiles 側で変えるのはこの 1 件のエントリとこの文書だけであり、ADR-0025 の D4 と同じ向きの撤去である。

## 根拠

- `docs/adr/0025-update-own-tools-local-registry.md` D2・D4: 登録はホストローカルに置き、対象側に痕跡を残さない。公開に達したら登録を消す。
- `docs/adr/0034-machine-state-wrapper-flake.md`: 規則・スキーマは public、実例化した具体値は private。
- Herdr の command 仕様(取得 2026-10-14): ui.tab_bar_right
  https://raw.githubusercontent.com/herdrdev/herdr/v0.9.4/docs/next/website/src/data/config-reference.json
