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
codex-plan experiment start --execute-efforts low,medium --until 2026-11-01
codex-plan experiment stop         # 抽選を止める。期間の記録は残る
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

公開仕様は `crates/model-catalog/catalog.toml`（ベンダー中立のモデルカタログ。同等性の判定も同じ正本を読む）にモデルID、単価、
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

## 実行推論量の観測

公開ベンチは「依頼から PR ができるまでの時間」を測らない。そこで普段の仕事を
そのまま実験にする。Plan は固定のまま、実行（Auto）の推論量だけを抽選する。

`experiment start` は、候補の推論量（2 つ以上）と最終日を
`settings.toml` の `[experiment]` に書く。候補は、実行モデルが native の
`model/list` で対応している値に限る。開始日はその日のローカル日付が入る。
実験が走っている間は、**新規起動のたび**に候補から一様に 1 つを選び、起動時に
「観測中: 今回の実行は gpt-6.1-sol low（〜日付）」を 1 行出す。選んだ値は
そのセッションの実行ペアとして使う（TUI の起動引数と、モード切替で戻すペアの
両方）。`resume` は抽選せず、保存済みのスレッドの値を使う。保存済みのペア自体は
書き換えない。`/model` による手動変更は従来どおり尊重する。
走っている実験に同じ候補で `start` を実行すると、最終日だけが延びる（開始日と割当て済みの
期間は変わらない）。候補が違うときは拒否する。
`experiment stop` は抽選を止め、`ended` に日付を書く。`[experiment]` を消さない
のは、あとから「抽選で割り当てた期間」を導けるようにするため。割当ての台帳は
作らない。選ばれた値は Codex 自身の履歴（各ターンの `effort`）に残る。
実験中は `/fast` を使わない。使うとそのスレッドは別枠に分かれる。

`compare` は、直近の暦日に作られたスレッドを実行モードの推論量で分け、次を出す。
各値は中央値と p90。

- 割当てあり: スレッドの開始が実験期間（`start`〜`ended`、停止前は `until`）に入る
- 観察のみ: それ以外。設定を自分で選んだので、仕事の偏りが混ざる
- PR まで: エージェントの稼働時間、最初のターン開始からの壁時計、人が割り込んだ往復数
- 1 ターン: 所要時間と最初のトークンまでの時間（native の `task_complete`）
- 別枠で数えるだけ: 実行モードの推論量が途中で変わったスレッド、service tier が
  既定以外のターンを含むスレッド、実行モードのターンが無いスレッド。腕には混ぜない
- PR 到達が 5 件未満の腕は「件数不足」とだけ表示し、比較に使わない

「PR ができた」は、`gh pr create` を含む呼び出しの結果に、PR の URL が 1 行として
出たときだけ数える。`--help` や失敗は URL を出さないので数えず、その後の再試行が
手戻りとして計測に残る。URL の規則は `crates/gh-edit-allow/src/gh.rs` の
`parse_url`（PR のみ）と同じで、テストで同じ入力を照合している。呼び出しと結果の
本文は照合したらすぐ捨て、保存も表示もしない。同じコマンドで PR の URL を echo
するような場合は、既存の hook と同じく区別できない。子エージェントの履歴は親の
仕事に含まれるので、スレッドとして数えない。

被験者は 1 人で期間も短いので、有意性は主張しない。分かるのは、各腕の値と件数まで。

## 通信と責務

```text
標準 Codex TUI --remote unix:///tmp/codex-plan-*/rpc.sock
    ↕ WebSocket / Unix socket
codex-plan
    ↕ JSONL / stdin・stdout
native codex app-server --listen stdio://
```

起動時は短命の app-server で initialize、作業ディレクトリを指定した config/read、
全ページの model/list、collaborationMode/list を照合する。
config/read の `default_permissions` が空でない文字列であることを確認し、
起動時の権限名として保持する。欠落・型不正・旧 sandbox 設定との併存では
理由を示して終了する。互換性のないプロトコル・モデルを検出したら
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
`thread/start`・`thread/resume`・`thread/fork` の `sandbox` を除き、
起動前に取得した名前を `permissions` に指定する。再開・派生でも現在の権限宣言を
選び、保存済みの履歴・モデル・推論量は書き換えない。要求の `config` に
`sandbox_mode`・`sandbox_workspace_write`（ドット区切りの子キーを含む）が
あれば、競合した要求を転送せず終了する。権限名やポリシーは中継に固定しない。
開始後の明示的な権限変更はそのまま native に渡す。
未知のフィールドは保持する。それ以外の要求・応答・
通知は JSON とサイズを検証して元の文字列を転送する。
会話・モード判定・Plan 指示・権限ポリシーの解決と強制・承認・hook・推論は native が担当する。

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
  承認・指示の補正や native 本体の fork は不要。

既存手段: `crates/codex-plan` — 自前 — native 設定だけでは Plan 専用の
モデルを指定できず、観測用 hook は要求の制御に使えないため中継を追加する。
TUI・推論・履歴・フレーム処理は既存手段を採用する。

## 執行点

- `crates/codex-plan/src/lib.rs`
- `crates/codex-plan/src/runtime.rs`
- `crates/codex-plan/src/main.rs`
- `crates/codex-plan/src/catalog.rs`
- `crates/model-catalog/catalog.toml`
- `crates/codex-plan/src/history.rs`
- `crates/codex-plan/src/comparison.rs`
- `crates/codex-plan/src/selection.rs`
- `crates/codex-plan/src/advice.rs`
- `crates/codex-plan/src/experiment.rs`
- `crates/codex-plan/src/threads.rs`
- `home/modules/claude.nix`
- `home/modules/codex.nix`

## 検証

`cargo test -p codex-plan` は preset、未知フィールド、ID型、手動モデル・承認の
非改変、設定の一括保存、無効なモデルと推論量、20 MiB の往復、切断、0700と
並列 socket の独立、子プロセス回収、行の上限、実験の期間・抽選・PR 完了の判定を検査する。
Unix socket を作れる実行環境が必要。

### 起動時の警告と資格情報保護

2026-10-27、Codex 0.164.0 で確認。remote TUI は名前付き権限を送らず、
旧形式の `sandbox=workspace-write` に変換する。この形式では今回の設定の
ネットワーク許可が失われ、資格情報のマスク処理が無効になっていた。
警告を取り除くため、上記の起動要求で native の権限名を選び直す。

開発中機能の注意書きは、home-manager の既存の設定反映で
`suppress_unstable_features_warning=true` を宣言する。
中継は警告通知を削除せず、不動作・実行時の警告もそのまま転送する。
この設定は開発中機能全般の注意書きを抑止するため、機能を追加したときも
その注意書きは表示されない。

既存手段: `crates/codex-plan` と `home/modules/codex.nix` — 拡張 —
既存の起動前検査・中継・設定反映を使う。ポリシーの複写や独自の表示フィルタは増やさない。

根拠（OpenAI, Codex rust-v0.164.0、取得2026-10-27）:

- D14: native の設定から権限名を取得し、起動要求に選択名だけを渡す。
  [config/read の作業ディレクトリ](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/app-server-protocol/src/protocol/v2/config.rs#L389-L407)、
  [remote の権限選択と旧形式への変換](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/tui/src/app_server_session.rs#L2160-L2199)、
  [名前付き指定と sandbox の排他](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/app-server-protocol/src/protocol/v2/thread.rs#L94-L100)。
  差分: remote の変換を中継で補正する（軸: 表現不可能 — 単一正本と排他的な選択）。
- D15: 開発中機能の注意書きは native の設定で抑止する。
  [設定定義](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/config/mod.rs#L1162-L1163)、
  [Configuration Reference](https://learn.chatgpt.com/docs/config-file/config-reference)（公開日不明）。
  差分: 一致。本人了承済みの標準設定を使う（軸: 還元 — 独自フィルタを作らない）。
- D16: 合成資格情報で実際のツール実行とモデルへの送信内容を検査する。
  [upstream の資格情報マスク試験](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/tests/suite/scenarios_credential_masking.rs#L128-L193)、
  [ネットワーク無効時の警告](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/config/mod.rs#L4916-L4951)。
  差分: 中継を含む native app-server の実行で検査する（軸: 検出のみ — 外部バイナリの動作確認）。

`tests/native_permissions.rs` は一時HOME、合成GitHub資格情報、ループバックの
模擬Responsesを使う opt-in 試験。実トークンや有料推論は使わない。
インストール済みのCodex 0.164.0、zsh、native sandboxを利用できるLinux上で実行する:

```sh
cargo test -p codex-plan --test native_permissions -- --ignored --nocapture
```

他のCodex sandbox内ではホストのdaemon socketが見えないため、通常のホスト権限で実行する。
一時HOMEに対するPATH補助バイナリの作成拒否は、nativeが意図して出す診断として
起動警告の通知とは区別する。試験は新規・再開・派生の実効権限名・通信許可と履歴・
モデル・推論量を読み戻し、通常の `exec_command` のshell起動で資格情報が置換値になり、
秘密パスが拒否されることを検査する。旧形式と通信無効の負の対照も含む。

同日の実行結果:

| 指定 | 実効権限名 | 通信許可 | 起動警告 |
|---|---|---|---|
| 旧 sandbox・注意書き表示 | なし | false | マスク無効・開発中機能の2件 |
| 中継の名前付き権限・注意書き抑止 | synthetic-check | true | 0件 |
| 名前付き権限・通信無効・注意書き抑止 | synthetic-check | false | マスク無効が残る |

再開・派生でも権限名と通信許可を読み戻し、履歴・モデル・推論量の保持を確認した。
模擬Responsesへの2回の要求には元の合成資格情報が含まれず、ツール出力は置換値と
秘密パスの `Permission denied` を含んだ。標準TUIの新規セッションも一時HOMEで
起動し、変更前の `2 warnings` 表示が変更後には出ないことを確認した。
現在ホストでは `hms .` の適用後にTOMLを読み、注意書き抑止・名前付き権限・
network_proxy・credential_maskingの設定値を確認した。

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

実行推論量の観測の根拠。取得日は 2026-10-27（D10 の上流ソースは 2026-10-26 に確認、
2026-10-27 時点で最新リリースは rust-v0.164.0 のまま）。

- D10: Plan は gpt-6.1-sol medium に固定し、観測の対象から外す。OpenAI、
  [Plan プリセット](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/models-manager/src/collaboration_mode_presets.rs#L13)
  は Plan の推論量を medium に固定し、Default はモデルの既定に従う。
  差分は一致。Plan は計画の質が手戻りを決めるので試さない（軸: 検出のみ）。
- D11: 実行の推論量は、新規起動ごとに一様に抽選する。Kohavi, Tang, Xu,
  [Trustworthy Online Controlled Experiments](https://experimentguide.com/)（2020）。
  差分は、割当ての単位が利用者でなく起動であること。被験者が 1 人なので、
  曜日や作業の偏りを無作為化で均す目的に絞り、有意性を主張しない（軸: 還元）。
- D12: 時間は native が保存する `task_complete` から導く。OpenAI、
  [TurnCompleteEvent](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/protocol/src/protocol.rs#L2273)
  の `duration_ms`・`time_to_first_token_ms`、および
  [常に保存する判定](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/rollout/src/policy.rs#L137)。
  差分は一致。独自の時刻記録は足さない（軸: 還元）。
- D13: 観測中は Fast を使わない。OpenAI、
  [Speed](https://learn.chatgpt.com/docs/agent-configuration/speed)（公開日の記載なし）は
  Fast を「利用枠を 2.5 倍の速さで消費」と説明する。差分は一致。観測の外に置く
  （軸: 検出のみ）。

実行側の既定値を決める入力（観測の前の手元の根拠。値は取得時点のもの）:

| 項目 | 値 | 出典 |
|---|---|---|
| gpt-6.1-sol の既定推論量（Codex） | low | OpenAI, [models.json L213](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/models-manager/models.json#L213) |
| gpt-6.1-sol の既定推論量（API） | medium | OpenAI, [GPT-6.1 Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol)。Codex と食い違うので、どちらも断定せず観測で決める |
| 知能指数（low / medium） | 42 / 48 | Artificial Analysis, [low](https://artificialanalysis.ai/models/gpt-6-1-sol-low)・[medium](https://artificialanalysis.ai/models/gpt-6-1-sol-medium)（ページに公開日の記載なし） |
| 出力速度（low / medium） | 49.9 / 53.0 tokens/s | 同上。出力速度は推論量でほぼ変わらない |
| 最初の回答トークンまで（low、medium プロンプト / long / 100k） | 1.47 / 2.71 / 6.10 秒 | 同上。プロンプト種別は同ページの区分で、各区分の入力量は未確認 |
| 同（medium） | 3.39 / 4.38 / 21.8 秒 | 同上。文脈が長いほど、推論量による待ちの差が開く |

Claude比較はAnthropic、[Pricing](https://platform.claude.com/docs/en/about-claude/pricing)と
[Fable 5.1](https://platform.claude.com/docs/en/models/fable-5-1/overview)、
[Opus 5.5](https://platform.claude.com/docs/en/models/opus-5-5/overview)、
[Sonnet 5.5](https://platform.claude.com/docs/en/models/sonnet-5-5/overview)による。
このリポジトリのモデル選択・料金計算はこれらの公式データを使う。
既存手段: `crates/codex-plan` — 拡張 — モデル取得・保存・中継を再利用し、
新たなLLM判定・外部価格API・自動fallbackを追加しない。

## GitHub 資格情報の起動経路

既存手段: 拡張 — `codex-launcher` の保護検査・資格情報取得をライブラリとして共有する。
`codex-plan` は情報表示とモデル探索の app-server、実セッションの app-server、
remote TUI を区別する。GitHub 資格情報を取得するのは実セッション用の app-server だけである。
通常 CLI と同じ版確認・native 保護設定検査を終えてから取得し、失敗時には
実セッションを起動しない。TUI と探索用プロセスからは継承した GitHub token も除去する。

端末で `gh auth login --hostname github.com` を実行した後は、新しい `codex-plan`
を起動するか、`codex-plan resume <ID>` で再開起動する。端末の認証成功だけでは
起動中セッションの復旧を示さない。新セッションの tool で次を実行し、
ログイン名と対象 Issue の本文を読み戻して確かめる。

```sh
gh api user --jq .login
gh issue view <NUMBER> -R <OWNER/REPO>
issue-inbox --check-dup '<TITLE>' <OWNER/REPO>
```

資格情報を引数・設定・会話へコピーせず、broker が tool 用の置換値へ変換する。
合成資格情報の `codex-launcher` 試験は両起動入口の取得順序、失敗時の未起動、
探索・TUI の資格情報除去を検査する。実際の native broker とモデル送信の確認は
上の opt-in 試験で行う。コードの根拠は
[GitHub provider](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/network-proxy/src/credential_broker/providers/github.rs#L14-L51)
（OpenAI *Codex*、公開日不明、取得日 2026-10-27）。

認証情報を持つ中継では、起動時だけでなく、新規・再開・派生と各 turn の明示的な
作業ディレクトリを再検査する。設定の上書きで masking・proxy・承認方式を弱める要求、
turn 中の権限 profile の切替、native の直接コマンド実行は拒否する。モデルと推論量は
引き続き変更できる。cwd を持たない `PreserveExistingThread` の特殊な再接続は、
保存先を推測して起動せず拒否する。有効な要求IDがある拒否は、その要求だけに
JSON-RPC エラーを返し、接続と他の要求は継続する。TUI が Git などの補助情報を
取得する `command/exec` もこの拒否の対象なので、補助情報は取得できない場合がある。
拒否を理由に TUI 全体を終了させず、成功結果も偽造しない。不正なJSON・ID、
応答できない禁止通知、通信障害では引き続き終了する。その場合は意図したディレクトリから
`codex-plan resume <ID>` で起動し直す。

これは、固定版 app-server が要求の config を CLI override の後ろへ追加し、再開時には
保存された cwd の設定も読み直すためである。OpenAI *Codex* `rust-v0.164.0`、公開日不明、
取得日 2026-10-27:
[config_manager.rs L565–577](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/app-server/src/config_manager.rs#L565-L577)、
[thread_processor.rs L4119–4121](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/app-server/src/request_processors/thread_processor.rs#L4119-L4121)、
[masking 無効時の扱い L4915–4951](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/core/src/config/mod.rs#L4915-L4951)。
通常の remote TUI の起動・turn は cwd を送り、モデル操作はこの保護の対象外である。

### 起動ラッパーの版固定（#1162）

`codex-launcher` は、GitHub 資格情報を渡す前に `codex --version` の出力を確かめ、
`crates/codex-launcher/reviewed.toml` の release と
完全に一致しなければ起動しない。引数の語彙と設定の検査が、その版のソースを読んで
作ったものだからである。範囲を宣言して通すと、読んでいない版の新しい subcommand・
権限 flag に資格情報を渡すことになるので、固定は残す。`codex update` は固定 commit の
公式 installer を使ってレビュー済みの具体版を選び、更新後に実際の版を読み戻す。

止まったときの stderr は、検出した版・レビュー済みの版・直し方を示す。直し方は次の順である。

1. 検出した版の OpenAI *Codex* のソース（`codex-rs/core/src/config/mod.rs` の設定と、
   起動引数の定義）を読み、`lib.rs` の語彙の表（`// Closed vocabulary` 以下）との差を確かめる。
2. `crates/codex-launcher/tests/` の受け入れ試験を、新しい版の出力で通す。
3. `reviewed.toml` の release と commit、および `lib.rs` 冒頭の版・取得日を更新する。

資格情報を渡さない起動（保護なしの通常のサブコマンド）は、この確認の対象外で止まらない。
Renovate は新しい release と commit を PR に提案するが、自動マージはしない。
