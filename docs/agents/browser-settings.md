# browser-settings — サービスの設定画面の 1 項目を読み取り・貼り付け・読み戻しする

実装: `crates/browser-settings`(Rust。`headless_chrome` で Chrome の DevTools Protocol を操作する手動 CLI)
データ: `config/browser-settings/services.toml`(サービスごとの画面探索定義。`~/.config/browser-settings/services.toml` に配置)
Issue: #1420

## 動機

月に 1 回、複数のチャットサービスの設定画面で、保存済みの設定文を読み取り、更新した文を貼り付け、反映されたことを読み戻して比べる作業がある。これまでは毎回その場でスクリプトを書いていた(2 回)。同じ操作が毎月必要になるため、1 本の CLI にまとめる。

## 使い方

```
browser-settings list                       services.toml のサービスの名前
browser-settings login  <service>           ウィンドウを開いて設定画面を開く。ログインして Enter で閉じる(専用プロファイルに残る)
browser-settings read   <service>           項目の現在の値を標準出力に出す
browser-settings paste  <service> --file F  項目に F の文を入れ、読み戻した値が F と一致するかを報告する(保存はしない)
browser-settings verify <service> --file F  項目の現在の値が F と一致するかを報告する
```

終了コード: 0 = 一致・成功、1 = 不一致、2 = 失敗(画面を開けない、項目がない、ログインが切れている、引数の誤り)。
`--profile DIR`(専用プロファイル。既定 `${XDG_STATE_HOME:-~/.local/state}/browser-settings/profile`)と `--services FILE`(既定 `~/.config/browser-settings/services.toml`)で保存先を変更できる。

## 設計

- **専用プロファイルの Chrome を起動のたびに立ち上げて接続し、終わったら閉じる。** 普段のブラウザには触れない。普段のブラウザを CDP 用のポート付きで常時起動する案は、そのポートに認証がなく、手元のどのプロセスからもログイン済みのブラウザを操作して Cookie を読めてしまうため採らない(`docs/claude/claude-mcp-servers.md` の 2026-10-28 の節)。Chrome 136 以降は、普段のプロファイルに対するこのポートを無視し、別の `--user-data-dir` を要求する(Will Harris, Google, <https://developer.chrome.com/blog/remote-debugging-port>、公開 2025-03-17)。専用プロファイルを使うため、この制約にも適合する。
- **各サービスへのログインは、専用プロファイルで最初の 1 回だけ人が手で行う**(`login`)。パスワードや認証コードはこの CLI では扱わない。Cookie は専用プロファイルに残り、次回からは `read` などが動作する。ログインが切れたら `login` で入り直す。
- **サービスごとの画面探索はデータ(`services.toml`)で定義し、コード側にサービス固有の記述を増やさない。** `url`、項目を出すまでの操作(`steps`: `click` または `wait`)、読み書きする項目の CSS セレクタ(`field`)を書く。セレクタは実際の画面を確認してから追加する。
- **`paste` は保存確定の手前で必ず止まる。** 入力欄(`input`・`textarea`)にはネイティブの setter で値を入れて `input` と `change` のイベントを送出し(フレームワーク管理の値にも反映される)、contenteditable は全選択して置き換える。保存ボタンは押さない。保存は人が行う(外向きの操作を代行しない)。テストで、組み立てた JavaScript に `click` や `submit` が含まれないことを検証する。
- セレクタと文は JavaScript の文字列リテラルとして JSON でエスケープして埋め込む(引用符・改行・記号を含む文が式の外へ出ない)。
- 比較時は改行を `\n` に揃え、末尾の改行だけを無視する。途中と先頭の空白には意味があるため無視しない。不一致時は最初に異なる行を出力する。

## テスト

- 純粋な部分(`services.toml` の読み込み、JavaScript の組み立て、比較)は `cargo test -p browser-settings` で実行する。
- 実際の Chrome に対するテスト(`tests/chrome.rs`)は `#[ignore]` としている(`google-chrome` が必要であり、CI には Chrome がない)。ログインが必要なサービスには触れず、ローカル HTML(`file://`)の fixture に対して、専用プロファイルで `textarea` と contenteditable への貼り付けと読み戻し(引用符・改行・`<b>`・バックスラッシュを含む文)、項目がないときの失敗を確かめる。手元で `cargo test -p browser-settings --test chrome -- --ignored --test-threads=1` として実行する(2026-11-10 に vega で 3 件が通過した。Claude Code の sandbox 内では Chrome が起動できず止まったため、sandbox の外で実行した。原因は未確認)。

## 既存手段と代替

- 既存手段: 自前。却下した候補は Playwright MCP の `--cdp-endpoint`(対話操作には足りるが、毎月同じ操作を決定論的に回す入口にならない)と、`playwright-core` を呼ぶ Node スクリプト(このリポジトリのツールは Rust 製であり、Node 側への依存を増やさない)。
- 本命の CDP クレート: `headless_chrome` 1.0.22(最終 push 2026-06-11、archived でない、README に保守停止などの警告なし、同期 API であり tokio 不要。crates.io と GitHub API、取得 2026-11-10)。対抗馬は `chromiumoxide` 0.9.1(最終 push 2026-04-03、async で tokio が必要)。この CLI は 1 回の操作を順に行うだけなので、同期 API の前者を採った。
- 軸: 検出のみ。保存を人にしか行えない形(`paste` に保存操作が存在しない)にして、外向きの操作を代行しない。

## 執行点

- crates/browser-settings/src/lib.rs
- crates/browser-settings/src/chrome.rs
- crates/browser-settings/tests/chrome.rs
- config/browser-settings/services.toml
- home/modules/agents.nix
- pipeline-map.toml
