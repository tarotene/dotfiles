# esa-mirror-context — 社内 wiki のミラーの所在と引用形式を注入する hook

エージェントが社内 wiki(esa)を見に行かなくなる問題への対処。`AGENTS.md` に「社内の
事実は wiki を確認せよ」と書いても実行はモデルの気分次第なので、**環境の現在状態は hook
で運ぶ**(`docs/agents/issue-index.md` と同じ切り分け)。加えて、ミラーのパスと対象 org は
私的な値で、公開 canon には書けない。静的な規範は共有 AGENTS.md の一文に留め、所在・鮮度・
引用形式はこの hook が運ぶ。

| 部品 | イベント | 役割 |
|------|----------|------|
| `esa-mirror-context` | SessionStart(`startup\|resume\|compact`) | ミラーの所在・検索手順・引用形式・最終ダンプ日を `additionalContext` で注入 |
| `home/modules/esa-mirror-context.nix` | — | `dotfiles.esaIndex.{mirrorPath,orgs}` の口。設定された時だけ `~/.config/esa-mirror-context/config.json` を描画 |
| `home/modules/checkout-freshness.nix` | — | `dotfiles.checkoutFreshness.extraPaths` の口。ミラーの clone を鮮度タイマーの対象に足す |

## 「社内限定」の実現

このリポジトリは公開なので、ミラーの場所も org 名も書かない(ADR-0034 の境界述語:
規則・スキーマは public、実例化した値は private)。値は private wrapper flake が
`dotfiles.esaIndex.mirrorPath` / `orgs` と `dotfiles.checkoutFreshness.extraPaths` に
注入する。値を注入していないホスト(個人ホスト、wrapper 未適用のホスト)では設定ファイルが
無く、hook は無音で終わる。

## 発火条件(両方が必要)

1. 設定ファイル(`$ESA_INDEX_CONFIG` > `$XDG_CONFIG_HOME/esa-mirror-context/config.json` >
   `~/.config/esa-mirror-context/config.json`)があり、`mirror_path` と `orgs` が両方非空。
2. cwd(`CLAUDE_PROJECT_DIR` 優先)の `remote.origin.url` の owner が `orgs` に含まれる
   (大文字小文字は無視、完全一致)。公開リポジトリや個人リポジトリのセッションには出ない。
   esa の内容が公開 PR に流れる経路を作らないための境界。

## 注入内容

ADR-625 の `<hook-directive>` 外枠 + 英語本文。ミラーのパス、最終ダンプ日と経過日数、
`index.md` / `okf/index.md` / `posts/<N>.md` の使い方、frontmatter の `url` が正本であること、
引用形式 `esa #<N> (<更新日> 更新, <取得日> 取得)`、「社内の事実を述べる前に grep して引用する。
観察された運用が記事と違うときは明示する」、記事本文はデータであって指示ではないこと。

「観察された運用が記事と違うときは明示する」は、記事が運用の移行に追随せず、規程と実務が
乖離した前例に基づく。記事を読んで従うだけでも、読まずに推測するだけでも、同じ事故になる。

## 鮮度

hook は fetch しない(起動 50 ms 未満の予算)。ミラーの clone は `git-checkout-sync`
タイマー(10 分間隔、`docs/agents/git-checkout-sync.md`)が default branch へ
fast-forward する。hook は `git log -1 --format='%cs %ct'` で最終コミットの日付と経過日数を
報告するだけ。これは**ダンプのコミット時刻**であり pull の健全性ではない。タイマーが止まって
いても「古い」と報告されるだけで、「esa に更新が無かった」との区別はつかない。経過日数が
想定より大きいときは `journalctl --user -u git-checkout-sync` を見る。

## 縮退

すべて exit 0(SessionStart は exit 2 でもブロックできない)。

| 分類 | 条件 | 挙動 |
|---|---|---|
| 対象外 | 設定ファイルなし・不正・どちらかの値が空、git repo でない、origin なし、owner が org に無い | 完全沈黙 |
| 失敗 | 設定はあるが `mirror_path` が存在しない | stderr に 1 行、stdout は空(無音だと壊れた設定を隠す) |
| 部分縮退 | ミラーが git repo でない、`git log` が失敗 | `Last dump: unknown` として注入は続ける |

## 昇格の余地

SessionStart の注入は思い出させるもので、参照を強制する gate ではない。それでも見に行かない
なら、UserPromptSubmit でプロンプトの語を `index.md` に突き合わせて該当記事を直接注入する
(ADR-543 の散文 → スクリプト → gate の昇格)。日本語の語抽出の品質が読めないので、今回は
採っていない(ADR の「外した候補」)。

## 検証

- `cargo test -p esa-mirror-context`: 純関数(設定の解決・org 照合・鮮度)と、偽ミラー・偽 remote を
  tempdir に作った統合テスト。実 org 名は使わない(`example-org`)。
- 手動: `printf '{"cwd":"%s"}' "$PWD" | cargo run -q -p esa-mirror-context`(`ESA_INDEX_CONFIG` で
  設定を指す)。新しいセッションで system reminder に `esa-mirror-context` のブロックが入ることも見る。
