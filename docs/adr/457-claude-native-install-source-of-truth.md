# ADR-457 — Claude Code 本体は native installer を正本にする

- Status: Accepted
- Date: 2026-09-24
- Issue: No-Issue(grill-me セッションで裁定)
- Amends: ADR-0001 の「home-manager が user environment の source of truth
  である」という決定を*変更しない*。その決定への scoped exception を新設
  する(ADR-0025 と同型)。

## Context

`home/modules/packages.nix` は unfree パッケージとして nixpkgs の
`claude-code` を宣言していた一方、`docs/cutover-runbook.md` の
「Removing an ad-hoc native Claude Code install」節は「native install
(`~/.local/bin/claude`)を見つけたら消せ」と書いていた。ところが
`config/shell/profile` の PATH 順序コメントと `docs/operations.md`(#313 /
PR #317 の裁定)は「`.local/bin` の `claude` は意図的な shadow で、消して
はいけない」と正反対のことを言っていた。同じリポジトリの中で 2 つの文書が
矛盾したまま共存していた。

矛盾の発端は #313 だった。`scripts/claude-plan-model` は
`command -v claude` で解決した*インストール済みバイナリ*に焼き込まれた
`latest_per_family` カタログを読んで Opus Plan Mode の具体モデル ID を
引き直す。`~/.local/bin` が `~/.nix-profile/bin` より PATH 上で先に来る
(ADR-0029)ため、native installer の自己更新シムが常に勝つ。#313 は
この事実だけを受けて「native 側が正」と裁定し、`config/shell/profile` /
`docs/operations.md` にその理由を書いた(PR #317)。しかし当時
`packages.nix` の `claude-code` 宣言と `cutover-runbook.md` の「消せ」節は
反転させず、そのまま残した。

矛盾が実害を生んだのが今回のセッションである。実測:

```
23:08  hms(activation)実行 → claude-plan-model sync 発火(旧カタログで pin)
23:13  native claude が自己更新: 2.1.220 → 2.1.281
       (新カタログ: opus = claude-opus-5-5、旧: claude-opus-5)
23:17  ユーザーが手で claude-plan-model を叩くまで、opusplan の Plan 側は
       旧世代の Opus モデルのままだった
```

`claude-plan-model sync` の発火点は `hms` とトグル実行時の 2 つだけ
(`docs/claude/opusplan-model-aliases.md`)。native の自己更新は `hms` を
経由しないため、根本原因は「claude 本体の更新経路が sync の発火点と
一致していない」ことだった。加えて `~/.claude/settings.json` の
`env.DISABLE_AUTOUPDATER=1` は手書きで宣言のどこにも無く、次に同じホストを
`hms` した際に何が起きるか(nixpkgs の `claude-code` 2.1.223 — 58 パッチ
遅れ — が `~/.nix-profile/bin/claude` に現れても、PATH 順序により実害は
無いはずだが、宣言と実体が一致していない状態そのものが ADR-0029 の趣旨に
反する)が誰にも分かる形で書かれていなかった。

## Decision

1. **`claude` バイナリは全ホストで native installer
   (`curl -fsSL https://claude.ai/install.sh | bash`)を正本とする。**
   `home/modules/packages.nix` から nixpkgs の `claude-code` を削除する。
   nixpkgs pin は upstream リリースに数十パッチ遅れており(実測: 2.1.223
   vs 2.1.281)、「新モデルが出た当日に使う」という運用と構造的に噛み
   合わない。`claude-plan-model` が読む「インストール済みバイナリ」は
   1 つでなければならず、native と nix の二重管理は #313 が指摘した
   「宣言側が勝つと実害が起きる」パターンをそのまま再現する。
2. **自動更新は宣言で OFF に固定する。** `home.sessionVariables` に
   `DISABLE_AUTOUPDATER = "1"` を追加する(`home/modules/claude.nix`)。
   公式ドキュメントによれば、これは background check のみを止め、
   `claude update` 自体は動く。更新経路を「手動 `claude update` の 1 つ」
   に絞ることで、次の項目が確実にそれを捕まえられるようにする。
3. **更新直後に `claude-plan-model sync` を実行する。** 対話 zsh の関数
   `claude()`(`config/zsh/modules/53-tools-claude.zsh`、新規)が
   `update`/`upgrade` サブコマンドを横取りし、実行後に
   `claude-plan-model sync` を呼ぶ。`hms`・トグル実行時と並ぶ 3 つ目の
   発火点になる。sync は steady state で無言・mtime 不変(`apply_mode` の
   rc=2 経路)なので、up to date のときの見た目は変わらない。
4. **exit 条件**: Claude Code 自身が更新完了を検知できる hook を提供する
   ようになったら、zsh 関数での横取りをその hook に置き換える。nixpkgs の
   `claude-code` がリリース追従を数日以内にするようになったら、この ADR
   全体を見直す。

## Alternatives considered

- **`nixpkgs-unstable` overlay で追従する**(herdr の前例、ADR-0001
  Amendment #42)。更新が必ず `hms` を通るため sync の発火点を増やさずに
  済むが、unstable でも数日〜数週遅れており、「新モデルが出た日に使う」
  用途には合わない。herdr は `exec` されるだけで `dlopen` されないため
  overlay が安全という判断だったが、claude はそもそも上流が高頻度で
  自己更新するツールであり、overlay を挟んでも native の自己更新シムとの
  競合(#313 の実測どおり `.local/bin` が勝つ)は解消しない。
- **SessionStart hook で毎起動 sync する**。更新経路を問わずに追従できるが、
  settings.json の `env` は起動時に process.env へ焼き込まれるため、
  SessionStart で sync しても効くのは次のセッションから
  (`docs/claude/opusplan-model-aliases.md` が同じ理由で既に棄却済み)。
  毎起動でバイナリを読むコストに対して得るものが薄い。
- **両方(zsh 関数 + SessionStart)**: 冗長性は増すが、同じ仕事を 2 機構が
  担うことになり還元性に反する。

## Consequences

- `home/modules/packages.nix` から `claude-code` が消え、`nix flake check`
  の評価対象が 1 パッケージ減る。全ホストが native installer に一本化
  される(vega・arcturus・altair、いずれも既に native で稼働中と確認済み)。
- `docs/cutover-runbook.md` の該当節が「消す」から「入れる」に反転する。
  greenfield ホストのセットアップ手順に native installer の実行が
  明示的に加わる(従来は nix パッケージが暗黙に入れていた)。
- `claude update` を打つ習慣さえあれば、モデル pin の追従を忘れることが
  構造的に無くなる(対話シェルでは wrapper が必ず割り込むため)。非対話
  経路(cron 等)での更新は自動更新 OFF により起きないため、この ADR の
  射程外のまま安全。
- ADR-0029 の「`.local/bin` は nix より前」という PATH 順序自体には手を
  加えない。この ADR はその順序が生む shadow の意味(「意図的か drift か」)
  を明文化するだけである。

## 執行点

- `home/modules/packages.nix` — nixpkgs `claude-code` の宣言を削除
- `home/modules/claude.nix` — `home.sessionVariables.DISABLE_AUTOUPDATER`
- `config/zsh/modules/53-tools-claude.zsh` — 新規。`claude update` 直後に
  `claude-plan-model sync` を実行する関数 `claude()`
- `docs/cutover-runbook.md` — 「Removing an ad-hoc native Claude Code
  install」を「Installing Claude Code (native installer)」に反転

## Amendment (2026-09-28 — codex を同型の scoped exception にする, #531)

Claude 高額利用者が 1 週間 Codex CLI 代替で仕事をする社内検証への備えで
Codex CLI のホスト管理方針を確認した結果、home-manager は `codex` バイナリ
も `~/.codex/config.toml` も管理していないことが判明した。この ADR と同型の
ADR-0001 scoped exception として、codex についても明文化する。

- **バイナリ**: `~/.local/bin/codex`(native installer)が正本。更新は
  `codex update`。claude と異なり `claude-plan-model sync` のような
  インストール直後追従作業を持たないため、zsh wrapper 関数は不要
  (還元性: 同じ仕事を担う機構が無いところに機構を増やさない)。
- **`~/.codex/config.toml`**: home-manager では**一切管理しない**。
  Codex 自身が hook trust hash(`hooks.state`)・project trust・モデル
  選択を実行時に書き戻す runtime 所有ファイルであり、`home.file` で
  read-only symlink 配備すると、Codex が新しい hook を信頼するたびに
  書き込みが失敗し、hook trust が毎回リセットされる不正な状態を作って
  しまう。対抗馬として検討した nixpkgs の `codex` パッケージでの管理は
  pin 固定という表現不可能性の利点はあるが、stable channel の遅れが
  claude と同じ理由(ADR-457 Context 参照)で検証週の最新版要件に直撃する
  ため還元性で劣る。
- 新規登録した hook(`config/codex/hooks/` 配下)を Codex に認識させるには、
  home-manager 適用後に一度だけ Codex TUI の `/hooks` で手動 trust する
  必要がある(`config.toml` の `hooks.state.*.trusted_hash` に記録される)。
  手順は docs/operations.md の Codex 切替節に記載。

### 執行点

- `AGENTS.md` — 「Note on codex」で codex を home-manager で管理しない旨を明文化
- `home/modules/claude.nix` — codex 向けに home-manager が実際に配備するのは
  hooks.json への登録と `~/.codex/AGENTS.md` のみで、`config.toml` は
  含まれないことの実体
- `docs/operations.md` — Codex 切替手順(`/hooks` trust 含む、段4で追記予定)

## Amendment (2026-10-20 — codex の config.toml は宣言したキーだけを合わせる)

上の Amendment の「`~/.codex/config.toml`: home-manager では**一切管理しない**」を
改める。**ファイル全体は引き続き管理しない**(read-only symlink にしない)。変わるのは
次の 1 点だけで、宣言したキーに限り、activation が書式を保ったまま冪等に宣言値へ
合わせる。宣言していないキー・コメント・並び・空行には触らない。

理由: herdr のサイドバーが Codex ペインの context 使用率・reasoning effort・5h/週の
rate limit を出すには、Codex 自身に端末タイトルとして描かせる設定(`tui.terminal_title`)
が要る。この値は Codex の hook からも documented な API からも取れない(Codex 0.160.0、
`docs/agents/herdr-sidebar-metadata.md` の「Codex / Copilot ペイン」節)。設定が各ホスト
に手で入っている前提にすると、ゼロ手作業を憲章にしたこのリポジトリの前提(README)に
反する。

- **宣言**: `home/modules/herdr.nix` の `codexConfigSpec`(「ドット区切りのキー → 値」の
  JSON)が唯一の正本。現在の宣言は `tui.terminal_title` の 1 キー。
- **適用**: `settings-reconcile codex-config`(`crates/settings-reconcile/src/codex_config.rs`)
  が `toml_edit` で読み、宣言値と違うときだけ同じディレクトリの一時ファイル経由で
  rename する。値が意味として同じなら(空白・クォートの違いは無視)書かず、mtime も
  変えない。ファイルが無ければ宣言キーだけで作る(mode 0600)。
- **宣言値と違うとき**: 適用のたびに宣言値へ戻す。TUI の `/title` での変更は次の
  `home-manager switch` で戻る。変えたいときはリポジトリを編集する(herdr の
  `config.toml` と同じ運用)。

### 対抗馬

- **zsh の `codex()` ラッパーが `-c tui.terminal_title=…` を渡す**: ADR-457 本文の
  「claude のようなラッパーは要らない」(還元性)を保てる。ただし効くのは zsh から
  起動したときだけで、herdr がペインを復元して起動する経路で効くかは未確認。経路を問わず
  効くのは activation 案。
- **`~/.codex/managed_config.toml` を `home.file` で配る**: 上流で legacy 扱い
  (openai/codex rust-v0.160.0、`codex-rs/config/src/loader/layer_io.rs:231`、取得
  2026-10-20)。しかも強制層なので TUI からの変更が効かなくなる。
- **nixpkgs の `toml-cli` を activation から呼ぶ**: 結果を標準出力に出すだけなので、
  同値なら書かない判定と原子的な置き換えがシェル側の手続きになる。型付きの spec と
  fixture テストで固定できる `settings-reconcile` の拡張を採った。

### 残るリスク

Codex が動作中に `config.toml` を書き戻す瞬間と、activation の読み書きが重なると、
どちらかの書き込みが失われうる(read-modify-write の窓)。書き込みは rename で原子的なので
ファイルが壊れることはない。`hooks.json` を `settings-reconcile codex-hooks` が書くときと
同じ種類のリスクで、窓は数 ms。Codex の hook trust(`hooks.state.*`)を失った場合は、
Codex の `/hooks` で信頼し直す。

### 執行点

- `home/modules/herdr.nix` — 宣言(`codexConfigSpec`)と activation(`reconcileCodexTerminalTitle`)
- `crates/settings-reconcile/src/codex_config.rs` — 宣言キーだけを合わせる実装
- `crates/settings-reconcile/tests/codex_config.rs` — 書式保持・冪等・不正入力の試験
- `AGENTS.md` — 「Note on `codex`」を本 Amendment に合わせた記述

## Amendment (2026-10-26 — use Auto-review for routine approval requests)

The session used Codex CLI 0.164.0 with `approval_policy = "on-request"`,
`approvals_reviewer = "user"`, and a workspace-write sandbox without network
access. The user config did not declare an approval reviewer. This explains
the manual approval route, but does not establish how much of the perceived
latency came from approvals rather than inference or tool execution.

### Decision

Declare `approval_policy = "on-request"` and `approvals_reviewer = "auto_review"`
through home-manager. Eligible approval requests go to Codex's built-in reviewer
in both Default and Plan modes. Plan continues to allow investigation and
planning only. The user approved automatic review for Plan-mode investigation
as well as ordinary execution.

Keep the current sandbox, network restrictions, command rules, hook
registrations, and built-in reviewer policy. A denial must be reported rather
than worked around; Auto-review does not promise that every action will run.
This change targets manual approval waits, not model inference latency.

Move the existing terminal-title declaration and the single config activation
from `home/modules/herdr.nix` to `home/modules/codex.nix`. That activation now
owns all three declared keys. It keeps the writable file and preserves every
undeclared key, including project trust, hook trust, and model selection.
Subsequent switches restore the declared values. Running sessions are not
reconfigured by rewriting the file; verify newly started sessions separately.

### Precedents and alternatives

- D1: Use the built-in reviewer, not a custom approval hook or wrapper.
  OpenAI, *Codex*, `rust-v0.164.0`, commit
  `6aaac694803004a63c6f393d153c96d22521a6b9`,
  [reviewer enum](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/protocol/src/config_types.rs#L175-L189)
  and [default selection](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/config/mod.rs#L3914-L3926),
  retrieved 2026-10-26. OpenAI, *Auto-review*, publication date not stated,
  <https://learn.chatgpt.com/docs/sandboxing/auto-review>, retrieved 2026-10-26.
  Difference: select the existing `auto_review` option instead of the `user`
  default. Axes: one upstream implementation avoids duplicate policy; full
  access or `approval_policy = "never"` would remove the boundary review we want.
- D2: Keep approval review independent of Plan/Default switching.
  OpenAI, *Codex*, same tag and retrieval date,
  [mode update](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/tui/src/chatwidget/settings.rs#L751-L768)
  and [Plan rules](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/collaboration-mode-templates/templates/plan.md#L17-L39).
  Difference: none; the upstream mode update leaves approval settings intact.
  Axes: no duplicate mode tracking or custom transition hook is necessary.
- D3: Extend the declared-key reconciliation introduced by this ADR's
  2026-10-20 Amendment; move its single writer into the Codex module.
  Difference: two additional scalar keys, no new settings writer.
  Axes: a single declaration avoids competing writers; a shell wrapper would
  only affect launches through that shell, and a whole-file symlink would
  prevent Codex from persisting its own state.

Existing mechanism: `settings-reconcile codex-config` — reuse. No new CLI,
approval engine, or hook is needed.

### Verification boundary

Before changing the live config, the installed CLI accepted both settings via
`-c` overrides with `app-server --print-startup-config` (exit 0, version 0.164.0,
`configRequirements.requirements = null`). This validates startup configuration,
not the reviewer's actual decisions or elapsed time. Configuration readback and
new-session behavior are separate checks; do not infer them from startup success.

After `hms .` on 2026-10-26, both declared approval values were read back from
the live config. A hash of the parsed undeclared settings matched before and
after activation. An app-server launched without config overrides returned
`auto_review` / `on-request` from both `config/read` and an ephemeral
`thread/start`, with `workspaceWrite` and `networkAccess = false`.

Two non-mutating live checks, one in Default and one with `turn/start` selecting
Plan, requested `/usr/bin/true` with `require_escalated`. Each emitted
`item/autoApprovalReview/completed` with `decisionSource = "agent"` and
`review.status = "approved"`; the matching command item then exited 0.
Review elapsed times were 2.153 s and 3.742 s respectively. These are individual
routing checks, not a latency benchmark or evidence that other actions will
be approved.

### 執行点

- `home/modules/codex.nix`
- `home/modules/herdr.nix`
- `home/common.nix`
- `AGENTS.md`

## Amendment (2026-10-27 — 共有ポリシーの `mcp_allow` を、Codex の config.toml に書く宣言として足す, #1051)

共有ポリシー(`home/modules/agent-permissions.nix`)の宣言は通信先だけだった。承認された許可を昇格候補のキューへ
回収する仕組み(#1043)の昇格先が通信先に限られ、MCP ツールの「Always allow」は記録にとどまっていた。
`mcp_allow`(`mcp__<server>__<tool>` の列、`*` は書けない)を足し、本人の裁定で、規則「個別許可は原則その実行限り」
の例外を 1 つ認める: 宣言した MCP ツール 1 つずつは、セッションを越えて常に許可する。

- **Claude**: 同じ文字列を `~/.claude/settings.json` の `permissions.allow` に書き、PreToolUse の hook は宣言済みの
  ツールに ask を返さない。
- **Codex**: ADR-457 の Amendment(2026-10-20)が許した「宣言したキーだけを合わせる」の範囲で、
  `mcp_servers.<server>.tools.<tool>.approval_mode = "approve"` を `config.toml` に書く。`config.toml` に**定義済みの**
  サーバーにだけ書く。`command` も `url` も無い `[mcp_servers.<名前>]` は Codex が `invalid transport` で拒否する
  (openai/codex rust-v0.164.0 `codex-rs/config/src/mcp_types.rs` L548-549、取得 2026-10-27)ので、宣言のためにサーバーを
  作らない。ファイル全体の管理(read-only symlink)は引き続きしない。
- **審査**: 追加は権限の拡大なので `inspect` → `migrate` を要求し、削除は縮小として自動で反映する(Codex は `prompt` に戻る)。
- **宣言しないもの**: Bash・`prefix_rule`・WebSearch。Claude の自動モードの `classifyAllShell` は Bash の allow を効かせず
  (Anthropic, "Configure auto mode", <https://code.claude.com/docs/en/auto-mode-config>、取得 2026-10-27)、
  コマンドの前方一致は同じ前置きの別の引数まで許す。MCP ツールは名前が 1 つに決まり、両 CLI に 1 対 1 で写せる。

### 対抗馬

- **何も足さない(キューは記録のみ)**: 還元性では最小だが、毎回の承認が減らず、昇格の経路が通信先にしか効かない。
- **`mcp__<server>__*` のサーバー単位の宣言**: 同じサーバーの書き込み系のツールまで許すので採らない(`*` を禁止した理由)。
- **Bash の前方一致(`readonly_commands`)**: 上の理由で、今は宣言しない。

### 執行点

- `home/modules/agent-permissions.nix`
- `crates/agent-settings-reconcile/src/agent_permissions/mod.rs`
- `crates/agent-settings-reconcile/tests/agent_permissions.rs`
- `crates/hook-router/tests/cli.rs`

## Amendment (2026-10-27 — 共有権限の所有管理を撤去し、native 設定へ戻す)

本 Amendment は、上の共有ポリシーと `mcp_allow` の運用を置き換える。
Claude と Codex の権限は、それぞれの native 設定として宣言・適用する。
共有管理の所有記録・hash・移行済み状態を、通常のツール実行の条件にしない。
所有検査の router 登録、SessionStart の許可回収、共有 apply activation を撤去する。
認証 launcher は維持し、資格情報を渡す前の native 保護検査を残す。共有の所有記録への依存だけを除去する。

### 裁定と先行例との対比

- D1: Claude 専用の `claude-permissions` reconciler を通常配備へ戻す。
  先行例は本リポジトリの `home/modules/claude.nix` と
  `crates/agent-settings-reconcile/src/settings.rs`、取得 2026-10-27。
  差分は共有管理中の activation 抑止を外し、native の allow / ask と撤回リストを使うこと。
  軸: 還元性。別の実行時所有検査を重ねない。
- D2: Codex は `approval_policy = "on-request"`、`approvals_reviewer = "auto_review"` と
  名前付き `dotfiles` 権限 profile を `codex-permissions` で合わせる。
  proxy と秘密パスの masking、個別 MCP 設定を native の宣言として残す。
  OpenAI, *Codex*, `rust-v0.164.0`、commit
  `6aaac694803004a63c6f393d153c96d22521a6b9`、公開日未確認、取得 2026-10-27。
  [権限 profile の実装](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/config/src/permissions_toml.rs)、
  [承認経路 L484–505](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/tools/approvals.rs#L484-L505)。
  差分は認証 launcher から dotfiles の所有記録への依存を外すこと。
  native の proxy・masking・承認設定と設定上書きの検査は資格情報を渡す前に行う。
  軸: 還元性。CLI 自身の sandbox と reviewer が執行する。
- D3: PermissionRequest hook の allow / deny は reviewer より先に適用される。
  Apps の reviewer は link / app / app default の指定を使いうるが、custom MCP は
  その上書きを使わない。両者を一律の共有許可へ変換しない。
  OpenAI, *Codex*、同じ版・取得日、
  [`tools/approvals.rs` L484–505](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/tools/approvals.rs#L484-L505)、
  [`mcp_tool_call.rs` L219–229](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/mcp_tool_call.rs#L219-L229)、
  [`connectors.rs` L514–550](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/connectors.rs#L514-L550)。
  差分は native 設定ごとの意味を保持すること。軸: 還元性。
- D4: 撤去は既存 CLI に `agent-permissions retire --home PATH [--dry-run]` を足し、
  transaction journal の変更前・変更後と現在値を照合して共有変更だけを戻す。
  先行例は `crates/agent-settings-reconcile/src/agent_permissions/transaction.rs`、取得 2026-10-27。
  差分は変更後と一致する箇所だけの逆適用。後からの編集、Claude の Plan、
  `autoMode.environment`、個別 MCP の許可を保持する。
  journal が無いときは共有 selector/profile の撤去に限り、由来が不明な設定は削除しない。
  軸: 表現不可能性。現在値と記録が一致しない変更を復元対象にしない。

既存手段: `claude-permissions` / `codex-permissions` / `agent-permissions` — 拡張。
独立した移行ツールや承認 engine は追加しない。
旧 `dotfiles.agentPermissions` の option は既存 3 ホストの互換入力として残し、
新しい入力は `dotfiles.claudePermissions.extraAllow` と
`dotfiles.codexPermissions.{secretPaths,domains,mcpAllow,apps}` に置く。
比較可能な native spec を `~/.config/agent-native/` に配備する。

### 検証の境界

今回の nested sandbox では reviewer の end-to-end 実行は未検証である。
設定の生成・parse・読み戻しを、実際の自動審査の成功や待ち時間の実測として扱わない。
旧 Amendment の実測は当時の構成についての記録として残す。
撤去手順と承認経路の詳細は [運用文書](../agents/agent-permissions.md) に集約する。

### 執行点

- `home/modules/claude.nix`
- `home/modules/codex.nix`
- `home/modules/agent-permissions.nix`
- `crates/agent-settings-reconcile/src/agent_permissions/retire.rs`
- `crates/hook-router/src/rules.rs`
- `crates/codex-launcher/src/main.rs`
- `config/claude/commands/promote-permissions.md`

## Amendment (2026-10-27 — Claude の sandbox・自動モード・秘密パスを native 宣言として合わせる)

上の撤去 Amendment の後も、Claude Code は「sandbox 有効・通信先空・自動許可なし・除外コマンドなし」の壊れた状態で
残った。最初の共有移行(2026-10-26)の journal が `transaction.rs` の `prune` で消えており、撤去が戻せなかったため
である。`gh` の認証(鍵束の D-Bus)・署名付き commit(gpg-agent)が sandbox 内で失敗し、sandbox を外した再実行は
自動モードの判定が拒否した。本人の裁定(2026-10-27)は、先週の「sandbox 無し」へ戻さず、Codex の `dotfiles` 権限
profile と同じ層構造に、`claude-permissions` が宣言で揃えること。

### 裁定と先行例との対比

- D1: Claude の sandbox・自動モードの `classifyAllShell`・秘密パスの Read deny を、`claude-permissions` が宣言したキーだけ
  合わせる(sandbox は残す)。
  先行例: Anthropic, "Sandboxing", <https://code.claude.com/docs/en/sandboxing>、取得 2026-10-27
  (権限ルールと sandbox は別の層で、OS が強制する。推奨の直し方は `allowedDomains`・狭い `excludedCommands`)。
  差分: 一致。通信先・秘密パスは Codex の `dotfiles` profile と同じ元(`agentPermissions`)を既定値にする。
  軸: 表現不可能性。sandbox は OS が強制するので、許可していない到達を「検出」でなく「できない」状態にする。
- D2: `autoMode.classifyAllShell = true` を残し、広い Bash 前方一致は宣言しない。
  先行例: Anthropic, "Configure permissions", <https://code.claude.com/docs/en/permissions>、取得 2026-10-27
  (「Bash のルールはプログラムの周りの境界ではない」「引数を絞る Bash パターンは壊れやすい」)と、本 ADR の
  2026-10-27 Amendment(Bash の前方一致は宣言しない)。
  差分: 一致。Codex の `auto_review`(全承認要求を審査)に当たる役割を `classifyAllShell` が担う。
  軸: 表現不可能性。
- D3: 失われた「常に許可」ルールのうち読み取り系だけを、本人のレビューを経て手順で戻す。宣言には入れない。
  先行例: `docs/agents/agent-permissions.md` の「autoMode.environment は機体ローカルの設定で、宣言しない」(#1087)。
  差分: 一致。
  軸: 還元性。

既存手段: `claude-permissions` — 拡張。`PermissionsSpec` に `deny`・`sandbox`・`auto_mode_classify_all_shell` を足す。
新しい reconciler・移行ツールは作らない。journal の無い変更は、戻さずに宣言で収束させる。

### 検証の境界

`excludedCommands` の既定(`gh *`・`git commit *`・`git push *`)は暫定で、vega の実機での実測は未取得である
(2026-10-27)。2026-10-28 の実測(#1164)で、単体の `gh`・`git push` が除外で通ること、呼び出し全体が除外に
合わないと sandbox に残ること(`git -C` の形・`nix`)が分かり、既定を追加した。署名付き commit の成功は未測定
(追加した `git -C * commit *` を適用したあとに再測定する)。実測の表と手順は
[運用文書](../agents/agent-permissions.md#claude-と-codex-の層構造を揃える) に置く。
`nix flake check` は、この Amendment を書いた環境の sandbox が nix daemon のソケットを開けないため手元で実行して
いない(CI の結果で確かめる)。

### 執行点

- `home/modules/claude.nix`
- `crates/agent-settings-reconcile/src/settings.rs`
- `crates/agent-settings-reconcile/tests/reconcile.rs`
- `crates/agent-settings-reconcile/tests/fixtures/permissions-sandbox-converges-broken-state/after.json`
- `crates/agent-settings-reconcile/tests/fixtures/permissions-sandbox-keeps-user-keys/after.json`
- `crates/agent-settings-reconcile/tests/fixtures/permissions-sandbox-from-empty/after.json`
