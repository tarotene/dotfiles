# codex-plan — Plan と実行のモデルを切り替える

`codex-plan` は標準 Codex TUI を起動し、同じ会話内で Plan を
`gpt-6.1-sol`、Auto（実行／native Default）も `gpt-6.1-sol` にする。
未設定時の推論量は両方 `medium`。保存済みのペアは維持する。
比較表と手元の使用量を見て、追加課金なしで普段の一日を続けられるか判断する。
品質・消費削減率・一日持続は未実測。API 単価から定額枠のドル上限は推計しない。
Auto は実行モードの表示名であり、承認方式を表すものではない。

## 操作

```sh
codex-plan                         # Auto / Sol で新規起動
codex-plan '調査して計画を立てて'
codex-plan resume <SESSION_ID>      # 保存されたモード・モデル・推論量を復元
codex-plan resume --last
codex-plan status                  # 保存ペア、未設定なら既定ペアを表示
codex-plan compare                 # 仕様・固定例・直近7暦日・現在の利用枠
codex-plan compare --days 14 --json # 機械可読の比較資料
codex-plan configure              # 比較 → Plan → Auto → 保存確認
codex-plan configure --plan-model gpt-6-astra --plan-effort medium \
  --execute-model gpt-6.1-sol --execute-effort medium
```

起動後は標準の Shift+Tab や `/plan`、計画の実行操作で切り替える。
`/model` による選択はその場で保持し、次のモード切り替えで設定ペアへ戻る。
resume 直後は保存済みの状態を保持し、次の切り替えから現在のペアを使う。
終了時に native が表示する `codex --remote unix://... resume ...` の socket は
このランチャーが回収するため、再開には `codex-plan resume <SESSION_ID>` を使う。
native の「running work continues」という案内と異なり、専用サーバーは終了する。

`configure` は番号選択で Plan のモデルと推論量、Auto のモデルと推論量を選ぶ。
Enter は現在の選択を維持し、`q`・EOF・保存確認での否定は保存しない。
4引数による明示指定も引き続き使える。非TTYでは明示指定の使い方を表示し、保存しない。
native の `model/list` 全ページ（hiddenを含む）とモデルID・推論量を
照合してから、`$XDG_CONFIG_HOME/codex-plan/settings.toml`
(未指定時 `~/.config/codex-plan/settings.toml`)を一括で保存する。
未設定の場合だけ既定値を使う。壊れた設定や部分的な設定から既定値へ戻さない。
変更は次回起動から有効。モデル世代は推測して更新せず、このコマンドで明示する。

利用不可・利用枠枯渇時は native のエラーを表示する。TUI終了後には観測した
構造化エラーを capacity（供給混雑）、利用枠枯渇、コンテキスト不足、その他／不明に
分け、native の候補と `configure` → `resume` の手順を案内する。
`serverOverloaded` は残枠があっても起こり、利用枠枯渇とは区別する。
短期の `rateLimitExceeded` も定額枠の枯渇とは断定しない。
nativeの再試行中の通知を停止とは扱わず、そのターンの成功や次ターン開始で
古い停止理由を解除する。中継による自動再送・自動モデル変更はしない。
候補への掲載はアカウントでの推論成功を保証しない。

通常の `codex` と herdr の起動設定は変更しない。解除は `codex-plan` を終了して
`codex` を使うだけ。ペア設定を初期化したいときは設定ファイルを退避する。
home-manager はバイナリだけを配備し、ペア設定も Codex の `config.toml` も配備・同期しない。
標準 TUI 自身による信頼設定や `/model` の設定保存は従来どおり native が行う。

## 比較表を読む

公開仕様は `crates/codex-plan/src/catalog.rs` の単一データにモデルID、単価、
課金条件、著者、資料名、公開日（判明分）、URL、取得日をまとめてある。
可用性の正本は毎回取得する native `model/list`。Claude の3モデルは比較専用。
未知のモデルを native が返した場合も選択できるが、価格・仕様は他世代から補わない。
一覧取得に失敗しても `compare` の公開資料は読める。新しいペアの保存は拒否する。

API context と最大出力、Codex の既定contextと実効値、API単価、creditsは別の列で読む。
Codex参考値は0.164.0に同梱された272,000と、その95%の258,400。
設定上書きの上限872,000は自動適用されない。remote catalogや設定で値は変わる。
`model/list` にcontext欄はないため、履歴の `model_context_window` も別に表示する。
過去の観測値から現在の設定を断定しない。独自の品質点は付けない。

固定例は10リクエストの合計。各リクエストは通常入力10,000、キャッシュ読取90,000、
出力2,000で、書込は0。Standard・globalのテキスト料金で、ツール料金や税は含まない。

| モデル | API相当額 USD | Codex credits |
|---|---:|---:|
| GPT-6 Astra | 2.90 | 72.50 |
| GPT-6.1 Sol | 0.49 | 12.25 |
| GPT-6 Sol | 0.58 | 14.50 |
| GPT-6 Luna | 0.029 | 0.725 |
| Claude Fable 5.1 | 2.225 | 対象外 |
| Claude Opus 5.5 | 0.98 | 対象外 |
| Claude Sonnet 5.5 | 0.49 | 対象外 |

この表は同じトークン量の比較であり、同じ仕事を終える費用や定額枠の持続比ではない。
推論量やモデルを変えると、必要なトークン量も結果の品質も変わる。

## 費用と履歴の集計

API相当額は、通常入力、キャッシュ読取、短期書込、長期書込、出力を
それぞれの100万tokens単価で掛けて足す。入力合計から読取・書込を引いて
通常入力を求める。推論出力は出力に含まれるので、重ねて加算しない。
OpenAIはリクエストの入力合計が272,000を**超えた**場合、そのリクエスト全体の
入力・キャッシュを2倍、出力を1.5倍にする。期間合計に閾値を適用しない。
OpenAIの書込TTLは30分／12時間、Claudeは5分／1時間で別単価。
履歴にTTLがなければ短期〜長期の範囲、書込数が不明なら非読取入力を
すべて通常入力とした場合〜すべて長期書込とした場合の範囲を示す。
リクエスト境界が不明な集計量を扱う計算APIも、短文〜長文の範囲を明示する。
creditsは独立した料金表で算出し、APIの長文割増や書込割増を転用しない。

`compare` は `$CODEX_HOME`（未指定時 `~/.codex`）の `sessions` と
`archived_sessions` を読み、ローカルタイムゾーンの当日を含む直近7暦日を集計する。
会話本文を送信・表示しない。未知の本文フィールドはデシリアライズ時に捨て、
モデル・モード・推論量・時刻・使用量だけを使う。JSONには判明したservice tierと
context観測も含む。料金表示は常にStandard相当で、実請求額とは区別する。

リクエストID付き記録を優先し、同一threadとresponseの重複やforkへの継承記録を除く。
旧形式では累積値の差分を使い、resumeと重複を処理する。集計リセット、
継承境界の曖昧さ、子エージェントと重なり得る旧親集計、モデル等の帰属不明は
別枠に残す。ゼロや他モデルへ置き換えて費用を確定しない。公開テストは合成データのみ。

## 利用枠と残り作業日数

`account/rateLimits/read` の全bucket、時間窓、使用率、reset時刻、
`ordinaryUsageAllowed` を読む。この利用可否が不明なら、使用率が低くても
利用可能と断定しない。APIキー認証などで読めない場合は読取失敗として表示する。

履歴の作成アカウントIDはresume後も固定され、現在アカウントの証拠にはならない。
そのため `compare` 時のアカウントに結び付いたAPI観測だけを
`$XDG_STATE_HOME/codex-plan/rate-observations.jsonl`
（未指定時 `~/.local/state/codex-plan/`）に保存する。ディレクトリ0700、ファイル0600。
このローカル記録には照合用のアカウントIDを含み、比較出力にはIDを出さない。
既存履歴から導けない対応関係を保存するための記録であり、手で台帳を更新する必要はない。

同じaccount・bucket・時間窓・reset区間で、日付境界の前後5分以内に観測があり、
一日が完了したときだけ消費率を計算する。消費率の最小値と最大値から、
`(100 − 現在の使用率) / 一日の消費率` の範囲を出す。
当日、欠測日、消費ゼロの日、reset跨ぎは別に数える。データ不足なら推定不可。
比較コマンドを数回使うだけでは、一日分の連続観測にならない場合がある。
他端末やChatGPTとの共有分を含み得るうえ、推定日数より先にresetが来る場合もある。
別モデルの持続時間には、モデル別の同一作業・アカウント・bucketの消費観測が足りない。
API価格やcreditsから定額枠のドル上限や持続比を逆算しない。

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
対話入力中もSIGINT・SIGTERM・SIGHUPを受け、ペアを保存せず終了する。
Tokioのシグナル登録は受信側を破棄しても残るため、入力待ちは別workerで行う
（Tokio 1.54.1、[unix.rs:331](https://github.com/tokio-rs/tokio/blob/tokio-1.54.1/tokio/src/signal/unix.rs#L331)、取得2026-10-26）。
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
- `crates/codex-plan/src/catalog.rs`
- `crates/codex-plan/src/history.rs`
- `crates/codex-plan/src/comparison.rs`
- `crates/codex-plan/src/selection.rs`
- `crates/codex-plan/src/advice.rs`
- `home/modules/claude.nix`

## 検証

`cargo test -p codex-plan` は preset、未知フィールド、ID型、手動モデル・承認の
非改変、設定の一括保存、無効なモデルと推論量、20 MiB の往復、切断、0700と
並列 socket の独立、子プロセス回収、行の上限を検査する。
Unix socket を作れる実行環境が必要。

実機の検証結果は PR の検証欄に記録する。標準 TUI の表示に加えて保存履歴の
`turn_context` の model、effort、collaboration_mode、approval_policy、
sandbox_policy を読み、応答と対応づける。利用枠削減率は未測定。

## 比較機能の根拠

以下は2026-10-26取得。ページに公開日の記載がないものは不明として扱う。
モデル別の取得日・公開日・課金条件の正本は上記catalogの各行にある。

- D6: 既存configure・native discoveryを拡張する。OpenAI、
  [Codex 0.164.0 model/list](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/app-server-protocol/src/protocol/v2/model.rs#L54)。
  差分は番号選択と比較表。既存の検証・一括保存を再利用する（軸: 還元）。
- D7: API相当額、credits、定額枠、capacityを分ける。OpenAI、
  [API Pricing](https://developers.openai.com/api/docs/pricing)、
  [Codex Pricing](https://learn.chatgpt.com/docs/pricing)、
  [構造化エラー](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/protocol/src/error.rs#L491)。
  先行例と同じ単位・状態の区別を保ち、不明を枯渇やゼロに変換しない（軸: 表現不可能）。
- D8: 未設定時の両モードをSol 6.1 mediumにする。OpenAI、
  [GPT-6.1 Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol)、
  [Model selection](https://learn.chatgpt.com/docs/model-selection)。
  差分は一日継続を重視した初期値と手元の観測。品質や持続の保証は加えない（軸: 検出のみ）。
- D9: 既存rolloutのリクエスト記録と累積差分を採用する。OpenAI、
  [TokenUsageRecord](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/protocol/src/protocol.rs#L2393)、
  [作成アカウントは固定](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/protocol/src/protocol.rs#L3261)、
  [account-bound snapshot](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/app-server-protocol/src/protocol/v2/account.rs#L331)。
  差分はアカウント帰属がない過去の使用率から日数を出さず、読取APIの観測で補うこと（軸: 表現不可能）。

Claude比較はAnthropic、[Pricing](https://platform.claude.com/docs/en/about-claude/pricing)と
[Fable 5.1](https://platform.claude.com/docs/en/models/fable-5-1/overview)、
[Opus 5.5](https://platform.claude.com/docs/en/models/opus-5-5/overview)、
[Sonnet 5.5](https://platform.claude.com/docs/en/models/sonnet-5-5/overview)による。
このリポジトリのモデル選択・料金計算はこれらの公式データを使う。
既存手段: `crates/codex-plan` — 拡張 — モデル取得・保存・中継を再利用し、
新たなLLM判定・外部価格API・自動fallbackを追加しない。
