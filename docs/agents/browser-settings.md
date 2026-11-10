# browser-settings — サービスの設定画面の 1 項目を、読み取り・貼り付け・読み戻しする

実装: `crates/browser-settings`(Rust。`headless_chrome` で Chrome の DevTools Protocol を操作する手動の CLI)
データ: `config/browser-settings/services.toml`(サービスごとの画面の探索。`~/.config/browser-settings/services.toml` に配備)
Issue: #1420

## 動機

月に 1 回、複数のチャットサービスの設定画面で、保存してある設定文を読み取り、更新した文を貼り付け、貼れたことを読み戻して
比べる作業がある。これまで、毎回その場でスクリプトを書いていた(2 回)。同じ操作が毎月要るので、1 本の CLI にする。

## 使い方

```
browser-settings list                       services.toml のサービスの名前
browser-settings login  <service>           窓を出して設定画面を開く。ログインして Enter で閉じる(専用プロファイルに残る)
browser-settings read   <service>           項目の現在の値を標準出力に出す
browser-settings paste  <service> --file F  項目に F の文を入れ、読み戻した値が F と一致するかを報告する(保存はしない)
browser-settings verify <service> --file F  項目の現在の値が F と一致するかを報告する
```

終了コード: 0 = 一致・成功、1 = 不一致、2 = 失敗(画面を開けない、項目が無い、ログインが切れている、引数の誤り)。
`--profile DIR`(専用プロファイル。既定 `${XDG_STATE_HOME:-~/.local/state}/browser-settings/profile`)と
`--services FILE`(既定 `~/.config/browser-settings/services.toml`)で置き場を変えられる。

## 設計

- **専用プロファイルの Chrome を、起動のたびに起動して接続し、終わったら閉じる。** 普段のブラウザには触れない。普段のブラウザを
  CDP 用のポート付きで常時起動する案は、そのポートに認証が無く、手元のどのプロセスでも、ログイン済みのブラウザを操作して
  Cookie を読めるので採らない(`docs/claude/claude-mcp-servers.md` の 2026-10-28 の節)。Chrome 136 以降は、普段の
  プロファイルに対するこのポートを無視し、別の `--user-data-dir` を求める(Will Harris, Google,
  <https://developer.chrome.com/blog/remote-debugging-port>、公開 2025-03-17)。専用プロファイルなので、この制約に合う。
- **各サービスへのログインは、専用プロファイルで、最初の 1 回だけ人が手で行う**(`login`)。パスワード・認証コードは、この CLI が
  扱わない。Cookie は専用プロファイルに残り、次から `read` などが入る。ログインが切れたら `login` で入り直す。
- **サービスごとの画面の探索はデータ**(`services.toml`)で、コードにサービスを足さない。`url`、項目を出すまでの操作(`steps`:
  `click` か `wait`)、読み書きする項目の CSS 選択子(`field`)。選択子は、画面を実際に読んで確かめてから足す。
- **`paste` は、保存の確定の手前で必ず止まる。** 入力欄(`input`・`textarea`)にはネイティブの setter で値を入れて `input` と
  `change` の事象を送り(フレームワークが管理する値でも反映される)、contenteditable は全選択して置き換える。保存ボタンは
  押さない。保存は人が行う(外向きの操作を代行しない)。試験が、組み立てた JavaScript に `click` や `submit` が無いことを見る。
- 選択子と文は、JavaScript の文字列リテラルとして JSON でエスケープして埋め込む(引用符・改行・記号を含む文が、式の外へ出ない)。
- 比べるときは、改行を `\n` にそろえ、末尾の改行だけを無視する。途中と先頭の空白は意味があるので無視しない。不一致は最初に違う行を出す。

## 試験

- 純粋な部分(`services.toml` の読み、JavaScript の組み立て、比較)は、`cargo test -p browser-settings` で回る。
- 実際の Chrome に対する試験(`tests/chrome.rs`)は `#[ignore]`(google-chrome が要る。CI には Chrome が無い)。ログインが要る
  サービスには触れず、ローカルの HTML(`file://`)の fixture に対して、専用プロファイルで、`textarea` と contenteditable への
  貼り付けと読み戻し(引用符・改行・`<b>`・バックスラッシュを含む文)、項目が無いときの失敗を確かめる。手元で
  `cargo test -p browser-settings --test chrome -- --ignored --test-threads=1` で回す(2026-11-10 に vega で 3 件が通った。
  Claude Code の sandbox の中では Chrome が起動できず止まったので、sandbox の外で回した。原因は未確認)。

## 既存手段と代替

- 既存手段: 自前。却下した候補は、Playwright MCP の `--cdp-endpoint`(対話の操作には足りるが、毎月同じ操作を決定論的に回す入口に
  ならない)と、`playwright-core` を呼ぶ Node の script(この repo の道具は Rust で、依存を Node 側に増やさない)。
- 本命の CDP のクレート: `headless_chrome` 1.0.22(最終 push 2026-06-11、archived でない、README に保守の警告の記載なし、
  同期 API で、tokio を要さない。crates.io と GitHub の API、取得 2026-11-10)。対抗馬は `chromiumoxide` 0.9.1(最終 push
  2026-04-03、async で tokio を要する)。この CLI は 1 回の操作を順に行うだけなので、同期 API の前者を採った。
- 軸: 検出のみ。保存を人にしか行えない形(`paste` に保存の操作が存在しない)にして、外向きの操作を代行しない。

## 執行点

- crates/browser-settings/src/lib.rs
- crates/browser-settings/src/chrome.rs
- crates/browser-settings/tests/chrome.rs
- config/browser-settings/services.toml
- home/modules/agents.nix
- pipeline-map.toml
