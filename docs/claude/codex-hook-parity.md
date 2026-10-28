# Claude と Codex で同じ Hook の検査を使う

最終確認: 2026-10-26。対象は dotfiles の保守者。登録方法を変えるときは対応表と
先行例を読み、適用するときは末尾の手順を使う。

Claude と Codex の登録は `home/modules/claude.nix` の同じ宣言から生成する。
ツール呼び出しの検査も `crates/hook-router/src/rules.rs` の同じ表を使う。
Codex 固有の入力変換を境界に集め、各検査を別実装にしない。登録後は Codex
自身の信頼確認が必要であり、設定に存在するだけでは発火しない。

## 要求インベントリ

- R1: 「Claude Code 向けに仕掛けた Hook が Codex に対しても全部有効になっているかどうかを確かめて欲しい」— 調査済み。変更前の実機は31登録すべて信頼済みだったが、下表の機能差があった。
- R2: 「気合で差分を閉じて欲しい．」— 同じPRで実装する。下表に全機能群の処分を示す。上流から情報が来ない残条件は `Blocked-Upstream:` として別記する。
- Reference-Only: #531 — 過去の展開範囲を確認する資料。今回の実装対象Issueではない。

## 各機能の接続先

| 機能 | Codex での接続・処分 |
|---|---|
| stash、main checkout、worktree作成、生成元表示、stacked PR、PRタイトル・本文確認、rulesets、決定と執行点の同時変更 | 実装済み。共通 router の PreToolUse |
| 外部送信、フィードバックの対象指定、リポジトリ作成前の確認 | 実装済み。同じ規則表。Connected Apps の Slack/Gmail 名も判定 |
| 新しい道具を作る前の既存手段検討 | 実装済み。apply_patch の Add/Move を Write に変換。検討記録は検査済み Plan から共通台帳へ登録 |
| ADR改番、作成済みPR/Issueの記録、コマンドハッシュ | 実装済み。共通 router の PostToolUse |
| git-worktree-allow、gh-edit-allow | 実装済み。PermissionRequest で既存判定エンジンを呼ぶ。範囲外は通常の確認へ戻す |
| 要求インベントリ、先行例、Copilotレビュー、base鮮度、Plan表示 | 実装済み。codex-plan-gate の Stop。子の拒否を Stop の block に変換 |
| issue-index、esa-mirror-context、wrap-up案内、署名準備、PR一覧、worktree鮮度、herdr警告、fail-open警告、daily-desk-reminder、worklog、worktree監査 | 実装済み。共通宣言の SessionStart |
| PR完了確認、wrap-up、main checkout事後検査、ターンログ | 実装済み。共通宣言の Stop/UserPromptSubmit |
| Herdr metadata | 実装済み。従来の4イベントに PreToolUse を追加。model と bypass 状態を更新。Plan mode を推測しない |
| Atuin | 実装済み。既存の `atuin hook codex` を利用。終了コードが無いときは -1（不明）を渡す |
| routines-write-guard | 実装済みの規則を共通 router に保持。RemoteTrigger が来れば検査する。現版Codexに同じサービスの操作ツールはないため、現在の入力経路は該当なし |
| 個別に追加されたローカルHook | `hook-router compat '<既存command>'` で同じスクリプトを呼ぶ。私有のルールや宛先は公開ソースへコピーしない。ask は理由を保った deny、allow は通常の確認フローへ戻す |

`new-tool-guard` の台帳が示すのは「既存手段を検討した」という事実である。
Codex の Plan 検査通過を本人の承認とは扱わない。Default mode では従来の
`new-tool-guard register` も使える。Plan の表示はレビュー結果を待たずに起動する。

## 入力の差は境界で変換する

Codex の `apply_patch` は、matcher に Write/Edit を指定しても payload の名前は
`apply_patch` のままである。router はパッチのヘッダーから全変更先を取り出す。
移動では元と先の両方を検査する。上流と同じく更新中の本文では行頭空白を保ち、
それ以外ではヘッダー前後の空白を除く。パッチの適用と妥当性検査は Codex が引き続き担う。

許可は PreToolUse に返しても Codex では効かない。そこで PermissionRequest
に接続し、既存の狭い許可条件をそのまま使う。拒否を出す共通 router は
PreToolUse に残す。MCP の GitHub 投稿も Codex の署名で判定する。

## 上流から渡らない情報は「不明」を保つ

残条件の追跡先: [#1021 — stdin・workdir・終了コードの入力不足](https://github.com/tarotene/dotfiles/issues/1021)。

- Blocked-Upstream: Codex 0.164.0 の `write_stdin` は入力前の Hook を呼ばない。
  起動済みの対話プロセスへ後から送った文字列は、この Hook 構成では検査できない。
- Blocked-Upstream: `exec_command` の workdir は PreToolUse 入力へ渡らない。
  Hook の cwd はセッション側のものなので、workdir だけで別 checkout を指す操作を
  完全には把握できない。明示パスのパッチ・git操作と、記録した checkout の事後検査は残る。
- Blocked-Upstream: unified exec の PostToolUse は出力文字列だけを渡し、終了コードを
  含めない。Atuin の既定値0を使うと失敗が成功として記録されるため、-1を渡す。
  PostToolUseFailure は現版のイベント語彙にない。正確な終了コードは上流の入力拡張待ち。

これらを解消するために Codex の fork や別の実行ラッパーを導入しない。
上流が各情報を Hook に公開した時点で、境界の変換と回帰テストを更新する。

## 先行例との対比

既存手段: crates/hook-router — 拡張 — 共通検査表を両エージェントで使い、Codex の入力・イベントだけを変換する。
既存手段: crates/codex-plan-gate — 拡張 — 同じ Plan エンジンを呼び、Stop の出力へ変換する。
既存手段: atuin hook codex — 採用 — 終了コード不明だけを補正する。

- D1: 登録と規則表を共通化 / 先行例は同repo `crates/hook-router`、変更前df5893c8、2026-10-26確認 / 先行例との差分は Agent 引数と入力境界のみ / 軸: 表現不可能性（片方への登録漏れを防ぐ）。
- D2: apply_patch をファイルごとの検査入力へ変換 / OpenAI, Codex rust-v0.164.0, [hook_names.rs L28–38](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/tools/hook_names.rs#L28)、[hooks.rs L5090以降](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/tests/suite/hooks.rs#L5090)、取得2026-10-26 / matcher aliasだけに頼らずpayloadを変換 / 軸: 還元性（既存ガードの再利用）。
- D3: 許可は PermissionRequest / OpenAI, Codex rust-v0.164.0, [pre_tool_use.rs L443以降](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/hooks/src/events/pre_tool_use.rs#L443)、[permission_request.rs L149以降](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/hooks/src/events/permission_request.rs#L149)、取得2026-10-26 / upstreamの公開イベントに合わせる / 軸: 還元性。
- D4: Atuinの既存Codex処理を利用 / Atuin contributors, v18.15.2, [hook.rs L42以降](https://github.com/atuinsh/atuin/blob/v18.15.2/crates/atuin/src/command/client/hook.rs#L42)、取得2026-10-26 / 隔離DBでauthor=codex・command・durationを読み戻した。Codexの出力形式に合わせ不明な終了コードだけを補正 / 軸: 還元性。
- D5: Planエンジンを再利用 / 同repo `hook-io/src/plan.rs`、`codex-plan-gate`、変更前df5893c8、2026-10-26確認 / Stopの拒否形式へ変換。表示は実pandoc、レビューは通信しない偽CopilotでPoC済み / 軸: 還元性。

上流の残条件の出典（OpenAI、Codex rust-v0.164.0、取得2026-10-26）:
[write_stdin.rs L132以降](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/tools/handlers/unified_exec/write_stdin.rs#L132)、
[exec_command.rs L499以降](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/tools/handlers/unified_exec/exec_command.rs#L499)、
[hook_runtime.rs L187以降](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/hook_runtime.rs#L187)、
[context.rs L409–447](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/tools/context.rs#L409)。

## 適用後は Codex の信頼状態を読み戻す

`hms .`（作業ツリーの検証）またはマージ後の `hms` で配備する。Codexを起動して
`/hooks` を入力して `Hooks` を開く。イベントを選び Enter で開き、変更された
ユーザーHookを選ぶ。command・イベントを確認し、`t trust` の `t` を押す。
詳細の `Trust` が `New hook - review required` または
`Modified since last trusted - review required` から `Trusted` になれば完了。
信頼後の Space/Enter は有効・無効の切替なので、信頼の操作には使わない。
これは OpenAI, Codex rust-v0.164.0 の
[hooks_browser_view.rs L533–619](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/tui/src/bottom_pane/hooks_browser_view.rs#L533)
（取得2026-10-26）で確認した名称とキーである。機械的に
`trusted_hash` を書き換えたり、信頼確認を無効化したりしない。

完了確認は Codex app-server の `hooks/list` に対象cwdを渡し、各Hookの
`enabled=true`・`trustStatus=trusted`、warnings/errorsが空であることを読む。
信頼済みでも実際の検査とは別なので、拒否例・許可例・パッチ編集・Planを
テストする。詳細は OpenAI, [Hooks](https://learn.chatgpt.com/docs/hooks)
（取得2026-10-26）、固定版の [hooks_list.rs](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/app-server/tests/suite/v2/hooks_list.rs#L190) を参照。

## `/hooks` の表示名（statusMessage）

Codex の `/hooks` は、hook の `statusMessage` を表示名にし、無い（空白だけを含む）hook を
すべて `Unnamed hook` と表示する。信頼の操作で、どの検査を信頼するのかが一覧から判断できない
（#1122）。そこで、Codex に登録する hook には `statusMessage` を付ける。

- 共有の宣言（`codexHookDeclarations`）: `pipeline-map.toml` のノードの `label` を表示名にする。
  `label` は ASCII で、`home/modules/pipeline.nix` の `nodeLabels` が nix の評価時に引く。
  node の無い宣言は、評価の時点で落ちる（表示名の無い hook を登録できない）。
- herdr の統合と worktree の context: `agent-settings-reconcile codex-hooks` の
  `--status-message <text>` で、その呼び出しで登録するすべての hook に同じ表示名を付ける。
- Claude の宣言には付けない。Claude は `statusMessage` を hook の実行中の表示にも使うため、
  共有の宣言から導く対象は Codex だけにしている。宣言から外すと、ハンドラの欄からも消える。

根拠: OpenAI, Codex `rust-v0.164.0`、取得 2026-10-28。
[hooks_browser_view.rs](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/tui/src/bottom_pane/hooks_browser_view.rs#L420)
は `hook.status_message` を trim し、空なら `Unnamed hook` とする。
[hook_config.rs](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/config/src/hook_config.rs#L190)
は JSON キー `statusMessage`（`#[serde(default, rename = "statusMessage")]`）を受け付ける。

疑問や上流更新による不一致は dotfiles の Issue に、Codexの版と上記の読み戻し結果を
添えて報告する。Hook はイベントで起動する検査、router は検査の振り分け役、
trust はそのcommandを実行してよいという本人の許可を指す。

## 執行点

- home/modules/claude.nix
- home/modules/herdr.nix
- home/modules/worktree.nix
- crates/hook-router/src/codex.rs
- crates/hook-router/src/rules.rs
- crates/codex-plan-gate/src/main.rs
- crates/agent-settings-reconcile/src/lib.rs
- crates/agent-settings-reconcile/src/hooks.rs
- home/modules/pipeline.nix
- home/modules/agents.nix
