# ADR-0000 — デザインの目視のために、画面に出る使い捨ての Chrome の Playwright を、ログイン用の Firefox と分けて足す

- Status: Accepted
- Date: 2026-11-26
- Issue: No-Issue（本人の依頼。Web ページの見た目を作り直す前に、エージェントが操作するブラウザを本人が画面で見られるようにする）
- Related: ADR-1677(Pages のデザイン基準)、ADR-1694(図・画像を手元の別窓に出す)

## Context

エージェントが操作する Playwright は、普段の Firefox のプロファイルの複製(ログイン状態を引き継ぐ)で、素の Firefox を **headless** で動かしている
(`docs/claude/claude-mcp-servers.md`「headless で動かす」)。2026-10-28 に headed を試したところ、普段の Firefox のほかに窓が 2 枚増えて邪魔だったため、
headless にした。原因は、Playwright の BiDi 版 Firefox が persistent context の起動時のウィンドウを拾えず、新しく作ることだった。

その結果、エージェントが開いたページ(作っている見本・`localhost`・公開ページ)を、本人が自分の画面で見て操作できない。
画面を撮って `show-image`(ADR-1694)で別窓に出す経路はあるが、静止画を見るだけで、操作の往復はできない。Web の見た目を作り直す作業(ADR-1677 の基準の上に、
全面に通す見た目の世界を決める)では、本人が実際の画面で見て判断することが要る。

窓が増える問題は Firefox の BiDi 版の挙動で、Chrome でも起きるかは分かっていなかった。

## Decision

### D1: 画面に出る使い捨ての Chrome を、別の MCP サーバー `playwright-chrome` として足す。ログイン用の Firefox(`playwright`)は変えない

`home/identities/personal.nix` に `playwright-chrome` を宣言する。起動は、既存の起動ラッパー(`crates/playwright-mcp-launcher`)の新しい形 `--chrome` で、
`npx @playwright/mcp@0.0.83 --browser chrome --isolated`(`--headless` は付けない)。`--isolated` はプロファイルをメモリだけに置くので、ディスクに残さず、ログインも持ち込まない。
用途の分け方は、ログインが要る作業は `playwright`(Firefox の複製、headless)、ログイン不要の目視は `playwright-chrome`。

先行例: Microsoft, `@playwright/mcp` 0.0.83 の `--help`(`--browser`・`--isolated`・`--headless` の説明)(取得 2026-11-26、手元のキャッシュ `cli.js` を実行して読んだ)。
dotfiles `docs/claude/claude-mcp-servers.md`「headless で動かす」(2026-10-28 の裁定と、窓が 2 枚になる原因)(取得 2026-11-26)。
差分: 異なる — 既存は、ログインを引き継ぐために Firefox の複製を 1 本だけ持つ設計だった。ここは、ログインが要らない用途を、ログインも複製も持たない別の使い捨てに分ける。
Firefox の「窓が増える」問題を headed のまま抱えるのでなく、問題の無い用途に別のブラウザを充てる。
軸: 表現不可能 — 目視の用途とログインの用途を別のサーバーに分けると、「ログイン済みの複製を headed で動かして窓が増える」状態そのものが生じない。
本命: `--browser chrome --isolated`(下の実測で、窓が 1 つであることを確かめた)。
対抗馬: `playwright` を headed に戻す案(同じ評価軸「窓が増える問題を、どの単位で消すか」。窓が 2 枚増える問題が戻る。2026-10-28 の実測)。`show-image` だけで済ませる案(同軸。見るだけで、本人が操作できない)。
外した候補: Chrome DevTools MCP(感触で外した。未調査。分析ではない)。
既存手段: crates/playwright-mcp-launcher — 拡張(`--chrome` を足す。新しい道具は作らない)。

### D2: `--chrome` は、Firefox の複製の引数と併用できない型にする。版の pin は 1 か所のまま

引数の解釈の結果を `Cli::{Firefox(Args), Chrome { passthrough }}` の列挙型にし、`--chrome` と `--rsync`・`--clone`・`--source-profile`・`--firefox-dir`・`--sync-only` の併用を、
解釈の段階で拒否する(終了コード 2)。複製を持つ Chrome を表現できない。`@playwright/mcp` の版の pin は、`PLAYWRIGHT_MCP` の 1 か所のまま(`chrome_npx_args` も同じ定数を使う)。

先行例: `crates/playwright-mcp-launcher/src/lib.rs` の `PLAYWRIGHT_MCP`(版の pin を起動ラッパーが持つ。#849)(取得 2026-11-26)
差分: 一致 — 同じ pin を、新しい形にも使う。Nix の `args` に版を直に書く案(pin が 2 か所になる)は採らない。
軸: 表現不可能 — 単一正本 > 複写+同期。

## Verification

2026-11-26、vega。`@playwright/mcp` 0.0.83 と同じ `playwright-core`(1.64.0-alpha-1790635538000)で、Chrome 152.0.7977.82 を `channel: 'chrome'`・`headless: false` の
persistent context で起動し、CDP で数えた(sandbox の外で単体実行)。

| 項目 | 結果 |
|---|---|
| Playwright が見たページの数 | 1(`about:blank`) |
| CDP の `Target.getTargets` の page の数 | 1 |
| `Browser.getWindowForTarget` の windowId の種類 | 1 |

Firefox の BiDi 版が、起動時のウィンドウを拾えず 2 枚目を作るのと違い、Chrome は起動時のウィンドウをそのまま使う。

未確認: 実際の Claude Code のセッションで、`playwright-chrome` の MCP サーバーが GUI の窓を出せるか(Claude Code が MCP サーバーを sandbox の外で起動する想定だが、`hms .` で配備した
あとの再起動したセッションで確かめる)。確かめる手順は、人手の確認として、確認の台帳に載せる。

## Alternatives considered

D1 の対抗馬・外した候補を参照。

## 執行点

- `crates/playwright-mcp-launcher/src/lib.rs`
- `crates/playwright-mcp-launcher/src/main.rs`
- `crates/playwright-mcp-launcher/tests/launch.rs`
- `home/identities/personal.nix`
- `docs/claude/claude-mcp-servers.md`
