# Claude と Antigravity CLI で同じ Hook の検査を使う

最終確認: 2026-11-27。対象は dotfiles の保守者。agy(Antigravity CLI)の hook の入力・出力を変えるときは、この対応表と
未確認の節を読む。

agy の hook は、Claude の hook と形が違う(camelCase の入力、`decision`・`reason`・`injectSteps` の出力)。検査の
本体は Claude の形で書かれているので、検査ごとに agy 版を作らず、**入力を検査の手前で Claude の形に直し、返答を
検査の後ろで agy の形にする**。直す場所は 1 か所ずつ:

- 入力: `crates/agent-session/src/antigravity.rs` の `normalize`。`Call::read_stdin`(`entry.rs`)と
  `hook-router` の入口が通す。
- 出力: 同じファイルの `render`。`Reply::render(Agent::Antigravity, event)` が呼ぶ。
- 規則の表に載らない個別の hook(SessionStart・Stop・ExitPlanMode の bin): `hook-router wrap <command>`
  (`crates/hook-router/src/antigravity.rs`)が、Claude の形の command をそのまま走らせて、出力だけを直す。

契約の出典は、agy 1.5.17 に同梱の `agy-customizations/docs/hooks.md`(取得 2026-11-27)。道具名と引数の名前は、同じ版の
会話記録(`~/.gemini/antigravity-cli/brain/*/.system_generated/logs/transcript.jsonl`)から実測した。

## 入力の対応

| 検査の語彙(Claude) | agy の入力 |
|---|---|
| `tool_name: Bash`・`tool_input.command` | `toolCall.name: run_command`・`toolCall.args.CommandLine` |
| `Read`・`tool_input.file_path` | `read_file`・`args.Path` |
| `Edit`・`file_path`・`old_string`・`new_string` | `edit_file`・`args.Path`・`Target`・`Replacement` |
| `Write`・`file_path`・`content` | `write_file`・`args.Path`・`Content` |
| `ExitPlanMode`・`tool_input.plan` | `exit_plan_mode`・`args.plan` |
| `Agent`・`tool_input.prompt` | `invoke_subagent`・`args.Prompt`(`TypeName` は `subagent_type`) |
| `session_id` | `conversationId` |
| `cwd` | `cwd`(無ければ `workspacePaths` の先頭) |
| `transcript_path` | `transcriptPath` |
| `hook_event_name` | `hookEventName` |
| `stop_hook_active` | `continuationActive` |
| `prompt` | `lastUserInput` |
| `tool_response.exitCode`(PostToolUse の Bash) | `error` の `exit status N`。`error` が無ければ 0、読めなければ -1(不明) |

知らない道具の名前と引数は、そのまま通す。transcript は、`type: USER_INPUT`・`source: USER_EXPLICIT` の行を、本人の
発言として読む(`agent_session::transcript::extract_user_text`)。

## 出力の対応

| イベント | agy の出力 |
|---|---|
| PreToolUse | 判定(allow・ask・deny)があれば `{"decision","reason"}`。無ければ何も出さない。文脈だけの返答は、届け先が無いので捨てる。理由が空なら定型文にする(agy は理由の無い決定を受けない) |
| PostToolUse | 常に `{}` |
| SessionStart | 文脈と `systemMessage` を、`injectSteps` の `systemMessage` にする。無ければ `{}` |
| Stop | deny(Claude の block)だけ `{"decision":"continue","reason"}`。他は `{}` |
| PermissionRequest | 何も出さない(agy に無いイベント) |

## herdr のサイドバー・statusline・モデルの固定

- herdr のサイドバー(ペインの状態): herdr 本体の連携を使う。`home/modules/herdr.nix` の
  `installHerdrAntigravityIntegration` が `herdr integration install antigravity-cli`(`~/.gemini/config/hooks/herdr-agent-state.sh`
  と、`hooks.json` への登録)を、`registerAntigravityHooks` の後ろで 1 度だけ呼ぶ。`herdr integration status` は、導入前の
  antigravity-cli を「not installed」と読んだ(2026-11-27)。`herdr-agent-metadata`(Claude・Codex の permission mode などを送る
  hook)と `claude-statusline`(Claude の statusLine の値を herdr へ送る)は、agy には登録しない。
- モデルの固定(`claude-plan-model sync`): Claude の `opusplan` の別名を具体 ID に引き直す機構。agy の `model` は
  `settings.json` に本人が選んで書く値で、#1726 が「書かないキー」と決めた(宣言しない)。agy に、別名の追従に当たる機構が
  あるかは未確認(`agy models` は認証が要り、この機体の sandbox からは読めない)。

## 登録

登録は `home/modules/agents.nix` の `antigravityHookDeclarations` が、Claude の宣言(`claudeHookDeclarations`)から導く
(Codex の `codexHookDeclarations` と同じ型。足した共有の hook は、agy に載せ忘れられない)。
`registerAntigravityHooks`(activation)が、`agent-settings-reconcile antigravity-hooks` で
`~/.gemini/config/hooks.json` の `dotfiles` のキーだけを置き換える。herdr の連携と `/hooks` が書く
ほかのキーは触らない。個人の identity だけで有効(`dotfiles.antigravity.enable`)。

| Claude の宣言 | agy の登録 |
|---|---|
| `hook-router-pre`・`hook-router-post` | `hook-router pre`・`post` に `--agent antigravity`(規則の表を共有) |
| 規則の表に無い個別の hook(SessionStart・Stop の bin、bleep、plan-view・plan-fresh-gate・copilot-plan-review) | `hook-router wrap '<Claude の command>' --timeout N` |
| matcher | 道具名を読み替える(`Bash`→`run_command`、`ExitPlanMode`→`exit_plan_mode`、`mcp__.*`→`mcp.*`)。SessionStart の matcher は落とす |
| timeout | agy 側は、中の timeout に 5 秒足す(中で先に切れて、空の返答を返せるように)。宣言に無ければ 60 秒 |

載せないもの: `herdr-agent-metadata`(herdr 本体の連携が持つ)、`agent-turn-log`(UserPromptSubmit が無く、ターンの開始を
取れない)、`atuin-history`(下の Blocked-Upstream)、`git-worktree-allow`・
`gh-edit-allow-judge`(`if` つきの allow 判定。Codex でも、ネイティブの承認を通すために外している)。

**agy の hook は逐次に走る**(文書の「Hooks run synchronously and block the agent loop」、複数の hook は順に実行)。
Claude は同じイベントの hook を並列に走らせるので、待ち時間は足し算になる(SessionStart の `gpg-prewarm` が最大 120 秒、
Stop の `pr-gate` が最大 600 秒など)。

### 計画の関所

`exit_plan_mode` に、PreToolUse で次を掛ける(本人の裁定、2026-11-27: Claude の `ExitPlanMode` と同じ関所を deny で掛ける)。

- `plan-scope-gate`(要求インベントリ)・`plan-precedent-gate`(先行例との対比): `hook-router pre` の規則の表。
- `plan-fresh-gate`(base の鮮度)・`plan-view`(ブラウザ表示)・`copilot-plan-review`(Copilot のレビュー): `wrap`。

agy は全ターンを計画から始めるので、設計判断を含まない小さな計画は、既存の 1 行の免除
(`先行例: 該当なし — <理由>`)で通す。実測(手元の `hook-router`、2026-11-27): 節の無い計画は agy の形の deny、
要求インベントリと免除の 1 行を持つ計画は、何も出さずに通った。
**ターンごとに `plan-view` が窓を開き、`copilot-plan-review` が Copilot にレビューを頼む**。実測では、小さな計画でも
`copilot-plan-review` が毎回走り、1 回あたり約 13〜16 秒かかった(下の「実測」)。`plan-view` の窓は未確認。重ければ
`antigravitySkipNodes` に足して外す(確認の台帳 #1718)。

## agy に同じものが無いもの(Blocked-Upstream)

- Blocked-Upstream: `SessionEnd`・`UserPromptSubmit`・`PostToolUseFailure`・`PermissionRequest` のイベントは、agy の hook に無い
  (`hooks.md` の「Supported Event Types」の 6 つだけ)。`PreInvocation` は毎回のモデル呼び出しの前で、ターンの開始ではない。
- Blocked-Upstream: SessionStart の `startup|resume|compact` の絞り込みと、hook の `if` は、agy に無い(matcher は道具名だけ)。
- Blocked-Upstream: PreToolUse で、判定を伴わない `additionalContext` を返す経路が無い。
- Blocked-Upstream: atuin の `atuin hook install`・`atuin hook <agent>` は、agy を対象にしない。atuin 18.15.2 のバイナリ内の記述は
  「Supported agents: claude-code, codex, pi」(2026-11-27)。agy のコマンドの履歴は、atuin に残らない。上流が agy を足したら、
  `antigravitySkipNodes` から `atuin-history` を外し、`hook-router` に Codex の `atuin` と同じ入口を足す。

## 実測(2026-11-27、vega の agy 1.5.17、実際のセッション 1 回)

`hms .` の適用後に、agy で 4 つの依頼(規則の質問、`git stash`、`ls`、空ファイルの作成)を送り、会話記録
(`~/.gemini/antigravity-cli/brain/<会話 ID>/.system_generated/logs/transcript.jsonl`)・ログ
(`~/.gemini/antigravity-cli/log/`)・状態ファイルから確かめた。

| 確かめた点 | 結果 |
|---|---|
| 規則 | 読む。モデルが `14-shared.md` と `00-antigravity.md` を名指しで引いて答えた(少なくともこの 2 つは、予算の超過で降格されていない) |
| PreToolUse の deny | 効く。`git stash` が拒否され、モデルに `tool call denied by pre-tool hook: <理由>` が届いた |
| PreToolUse の「意見なし」 | 空の標準出力を受け付ける。`ls`・`write_file` は、エラー表示なしに動いた |
| SessionStart の注入 | 届く。モデルが「朝の机が未実施」の注入を、答えに足した |
| PostToolUse | 走る。`cmd-hashes.jsonl` に agy の会話 ID が残った |
| 道具名と引数 | `run_command{CommandLine}`・`write_file{Path,Content}`・`exit_plan_mode{plan}`。入力の対応の表のとおり |
| `exit_plan_mode` の hook | 走る。`copilot-plan-review` が 2 回(22:27・22:32)走って `pass`。1 回あたり、計画の送信からレビューの記録まで約 13〜16 秒 |

### agy の安全監視は、hook のスクリプトを実行する動きを検知する

agy は、自前の LLM 監視(ログの `Monitor sir_flash_lite`)を持つ。初めの版の指示文は、計画を渡す前に
`~/.claude/hooks/plan-precedent-gate --check` などを実行する手順を書いていた。モデルがそれに従うと、監視が
「Risk Signal B(Safety and oversight controls: hook や監視を無効化・編集・停止・回避・探ること)」として 2 回検知した
(重大度 6 と 7、確信度 9)。計画の一時ファイルを `/tmp` に書こうとして sandbox の境界で失敗し、モデルが
「境界を回避する」と推論したことも、2 回目の検知の理由だった。

対応: 指示文(`config/antigravity/AGENTS.antigravity.md`)から、`--check` を自分で実行する手順を外し、「hook のスクリプトを
自分で実行して試さない。計画は書式どおりに書いて渡し、拒否されたら理由に従って直す」に改めた。共有の skill に
`--check` の手順があっても、agy では実行しない。

## 未確認(実機の payload で確かめる)

- MCP の道具が `toolCall.name` に載せる名前の形。上のセッションでは、MCP の道具を使っていない。今は名前をそのまま通すので、
  `mcp__` で始まる名前を前提にする規則(外部送信の抑止など)は、agy の MCP には当たらない。
- `Grep`・`Glob` に当たる道具の名前(`main-checkout-guard` の読み取りの検査が使う)。
- agy の `error` の文面が `exit status N` 以外の形を取るか。
- Stop の hook(`pr-gate`・`ja-polish`・`main-checkout-guard`・`session-wrapup`)が agy のターンの終わりに走ったか。上のセッションでは、
  状態ファイルに痕跡が残らず、確かめられなかった。Stop の `continue` を返したあとの再入(`continuationActive`)も、同じく未確認。
- `plan-view` の窓が `exit_plan_mode` ごとに開くか、herdr のサイドバーに agy のペインの状態が出るか(本人の目で確かめる)。

実機での確認は、確認の台帳 #1718 に載せる。

## 執行点

- crates/agent-session/src/antigravity.rs
- crates/agent-session/src/entry.rs
- crates/agent-session/src/reply.rs
- crates/hook-router/src/antigravity.rs
- crates/hook-router/src/main.rs
- config/agents/hook-router-canary.sh
- home/modules/agents.nix
- crates/agent-settings-reconcile/src/antigravity_hooks.rs
