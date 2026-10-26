# codex-plan — Plan と実行のモデルを切り替える

`codex-plan` は標準 Codex TUI を起動し、同じ会話内で Plan を
`gpt-6-astra`、Default を `gpt-6.1-sol` に切り替える。推論量は両方 `medium`。
目的は Plan にフロンティアモデルを使い、実行側の ChatGPT 利用枠の消費を抑えること。
消費削減率は測定していない。API 単価から ChatGPT の利用枠を推計しない。

## 操作

```sh
codex-plan                         # Default / Sol で新規起動
codex-plan '調査して計画を立てて'
codex-plan resume <SESSION_ID>      # 保存されたモード・モデル・推論量を復元
codex-plan resume --last
codex-plan status                  # 保存ペア、未設定なら既定ペアを表示
codex-plan configure --plan-model gpt-6-astra --plan-effort medium \
  --execute-model gpt-6.1-sol --execute-effort medium
```

起動後は標準の Shift+Tab や `/plan`、計画の実行操作で切り替える。
`/model` による選択はその場で保持し、次のモード切り替えで設定ペアへ戻る。
resume 直後は保存済みの状態を保持し、次の切り替えから現在のペアを使う。
終了時に native が表示する `codex --remote unix://... resume ...` の socket は
このランチャーが回収するため、再開には `codex-plan resume <SESSION_ID>` を使う。
native の「running work continues」という案内と異なり、専用サーバーは終了する。

`configure` は4項目をすべて指定する。native の `model/list` とモデルID・推論量を
照合してから、`$XDG_CONFIG_HOME/codex-plan/settings.toml`
(未指定時 `~/.config/codex-plan/settings.toml`)を一括で保存する。
未設定の場合だけ既定値を使う。壊れた設定や部分的な設定から既定値へ戻さない。
変更は次回起動から有効。モデル世代は推測して更新せず、このコマンドで明示する。

利用不可・利用枠枯渇時は native のエラーを表示し、中継による代替モデルへの
自動切り替えはしない。カタログにあることはアカウントでの推論成功を保証しない。

通常の `codex` と herdr の起動設定は変更しない。解除は `codex-plan` を終了して
`codex` を使うだけ。ペア設定を初期化したいときは設定ファイルを退避する。
home-manager はバイナリだけを配備し、ペア設定も Codex の `config.toml` も配備・同期しない。
標準 TUI 自身による信頼設定や `/model` の設定保存は従来どおり native が行う。

## 通信と責務

```text
標準 Codex TUI --remote unix:///tmp/codex-plan-*/rpc.sock
    ↕ WebSocket / Unix socket
codex-plan
    ↕ JSONL / stdin・stdout
native codex app-server --listen stdio://
```

起動時は短命の app-server で initialize、全ページの model/list、
collaborationMode/list を照合する。互換性のないプロトコル・モデルを検出したら
TUI や推論を開始しない。その後、別の専用 app-server と標準 TUI を起動する。
検証中に要求された承認を勝手に許可しない。

`collaborationMode/list` 応答の Plan/Default の `model` と `reasoning_effort` を補う。
加えて、明示的なモード切替の `thread/settings/update` / `turn/start` にある
モデルと推論量をペアへ戻す。native は手動選択した Plan 推論量を次回にも
保持するため、プリセットだけでは復帰できない。start/resume の応答から初期モードを
取り、同じモードでの設定変更は手動選択として通す。切替直後の古い設定を含む
turn/start も、次の手動設定更新まで補正する。native の適用結果通知をそのまま
返し、TUI が実際に適用された設定を表示する。履歴や指示文からモードを推測しない。
IDは整数と文字列を区別し、サーバーからの承認要求とは別に扱う。
未知のフィールドは保持する。それ以外の要求・応答・
通知は JSON とサイズを検証して元の文字列を転送する。
会話・モード判定・Plan 指示・sandbox・承認・hook・推論は native が担当する。

TUI には現在の作業ディレクトリと Plan 推論量を CLI 引数で渡す。
新規起動だけ実行モデルと実行推論量も渡す。resume にそれらを渡すと
native が保存モデルを上書きするため、渡さない。

送受信を独立して進め、片方向の pipe が詰まっても逆方向を読み続ける。
中継の JSON 行と WebSocket の上限は256 MiB。**native TUI 0.164.0 の
受信上限は128 MiB**なので、256 MiBまで TUI へ届けられるという意味ではない。
上限超過・不正JSON・切断では診断して終了し、不完全なメッセージを転送しない。
切断後に接続し直して要求を再送しない。

起動ごとに0700の一時ディレクトリを原子的に作り、socket と子プロセスを分離する。
終了時は標準入力を閉じて native の終了を最大5秒待ち、残ったプロセスグループを
終了させて回収する。SIGINT・SIGTERM・SIGHUP、起動失敗も回収対象。
configure 中の事前検証にも同じシグナル回収を適用する。
TUI を強制終了した場合はランチャーが起動前の端末状態を復元する。
SIGKILLやマシンクラッシュではプロセス内の cleanup は実行できない。

## 設計根拠

取得日はすべて2026-10-26。OpenAI Codex は rust-v0.164.0
(`6aaac694803004a63c6f393d153c96d22521a6b9`)に固定してコードを確認した。

- D1: upstream の TUI と app-server を採用し、preset だけを中継で補う。
  OpenAI, [preset 定義](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/models-manager/src/collaboration_mode_presets.rs#L6)、
  [適用処理](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/tui/src/chatwidget/settings.rs#L690)、
  [protocol](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/app-server-protocol/src/protocol/v2/collaboration_mode.rs#L19)。
  差分は null のモデル欄と推論量をペアで補うこと。本体 fork と比較して
  upstream 正本を保ち、モードをログやプロンプトから推測しない。
- D2: Rust と Tokio / tokio-tungstenite を採用する。
  このリポジトリ, [ADR-0024](adr/0024-hook-cli-scripts-target-rust.md) (2026-09-21);
  snapview, [tokio-tungstenite v0.28.0](https://github.com/snapview/tokio-tungstenite/blob/v0.28.0/src/lib.rs#L127)。
  シェルによる双方向中継と比較し、ID・状態の型を持たせる。WebSocket の
  フレーム処理を自作せずライブラリへ任せる。依存は Cargo.lock に固定する。
- D3: ペア設定を単一の正本として保存する。
  このリポジトリ, [Claude の拡張](claude/opusplan-model-aliases.md)、
  [実装](../crates/claude-plan-model/src/lib.rs)。利用者の選択保持は同じ。
  差分はグローバルなエイリアスの書き換えではなく専用起動だけへの適用と、
  世代IDを明示的に選ぶこと。通常の Codex 設定への複写・同期を不要にする。
- D4: native の resume と推論量優先順位を維持する。
  OpenAI, [resume 判定](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/tui/src/app/config_persistence.rs#L31)、
  [Plan 推論量の上書き](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/tui/src/chatwidget/settings.rs#L720)、
  [WebSocket 上限](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/app-server-client/src/remote.rs#L67)。
  計画からの補足は Plan 推論量の起動引数指定と TUI 側128 MiB制限の明記。
- D5: プリセットに加え、モード切替要求のモデルと推論量だけを補正する。
  OpenAI, [手動推論量の保持](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/tui/src/app/model_defaults.rs#L80)、
  [設定更新はキュー投入](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/app-server/src/request_processors/turn_processor.rs#L830)、
  [適用通知による画面同期](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/tui/src/chatwidget/settings.rs#L495)。
  実測ではプリセットだけだと Plan/high→Default/medium→Plan/high となった。
  計画の「応答だけ」を拡張し、明示的なモード値で切替を認識する。
  同モードでの手動選択を奪わず、通知待ちの古い turn/start も補正する。
  権限・承認・指示の補正や native 本体の fork は不要。

既存手段: `crates/codex-plan` — 自前 — native 設定だけでは Plan 専用の
モデルを指定できず、観測用 hook は要求の制御に使えないため中継を追加する。
TUI・推論・履歴・フレーム処理は既存手段を採用する。

## 執行点

- `crates/codex-plan/src/lib.rs`
- `crates/codex-plan/src/runtime.rs`
- `crates/codex-plan/src/main.rs`
- `home/modules/claude.nix`

## 検証

`cargo test -p codex-plan` は preset、未知フィールド、ID型、手動モデル・承認の
非改変、設定の一括保存、無効なモデルと推論量、20 MiB の往復、切断、0700と
並列 socket の独立、子プロセス回収、行の上限を検査する。
Unix socket を作れる実行環境が必要。

実機の検証結果は PR の検証欄に記録する。標準 TUI の表示に加えて保存履歴の
`turn_context` の model、effort、collaboration_mode、approval_policy、
sandbox_policy を読み、応答と対応づける。利用枠削減率は未測定。
