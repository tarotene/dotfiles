# ADR-0011 — ローカル活動ログの採取(atuin + Claude Code ターンログ)

- Status: Accepted
- Date: 2026-09-14
- Issue: No-Issue(事前対応。消費側は別リポジトリの `daily-report` — ここでは
  名前を挙げるだけで、本文で参照しない)

## Context

社内の活動記録・自己評価ツール一式(このリポジトリの管轄外にある、別の
private リポジトリ)は Slack・GitHub のイベントログから「業務時間の証跡」
を再構成している。しかしターミナル上のコマンド操作や Claude Code との
セッションのようなローカルのみで起きる作業は、Slack/GitHub のどちらにも
痕跡が残らず、この再構成の外側に落ちる。

このリポジトリ(`tarotene/dotfiles`)は home-manager 経由でその開発者の
ローカル環境そのものを宣言している以上、**採取層**をここに置くのが自然
である。一方、採取したログを解釈・集計・射影する側(消費側)は home-manager
の管轄外であり、別リポジトリの CLI がファイルとして直接読む契約にする —
ADR-0001(home-manager is the source of truth for *this* environment,
not for downstream tooling)と同じ切り分け方。

## Decision

1. **採取点は 2 つ、いずれもプレーンテキスト・非同期(un-synced)。**
   - **atuin**(`home/modules/atuin.nix`、`programs.atuin`): シェルコマンド
     履歴を SQLite の `history.db`(既定 `~/.local/share/atuin/history.db`)
     に記録する。`update_check = false` / `auto_sync = false` を設定し、
     `atuin register`/`atuin login` は一度も呼ばない。Atuin 自身のドキュメント
     — Atuin, "Config"
     (<https://docs.atuin.sh/latest/configuration/config/>、2026-09-14 取得)
     — は「With the update check turned off and sync not set up, Atuin
     makes no network requests of its own」と明記しており、`auto_sync`
     単体の無効化だけでは不十分(`update_check` は sync 状態と無関係に
     `https://api.atuin.sh` を叩く)であることも同ページで確認した。
   - **agent-turn-log**(`config/claude/hooks/agent-turn-log.sh`、
     `home/modules/claude.nix` 経由で `UserPromptSubmit`/`Stop` に登録):
     Claude Code の 1 ターンの境界(プロンプト送信・応答終了)を JSONL
     1 行として `${XDG_STATE_HOME:-$HOME/.local/state}/daily-report/
     agent-events.jsonl` に追記する。1 スクリプトが両イベントに同一
     `command` で登録され、`.hook_event_name` で分岐する
     (`herdr-claude-metadata.sh` と同じ形)。
2. **出力契約(byte-for-byte、消費側が直接依存する)。**
   - パス: `${XDG_STATE_HOME:-$HOME/.local/state}/daily-report/agent-events.jsonl`
     (ディレクトリ mode 700、ファイル mode 600、なければ作成)。
   - `UserPromptSubmit` 1 行:
     ```json
     {"kind":"prompt","agent":"claude-code","ts":"<ISO8601 UTC>","session_id":"<...>","prompt_id":"<...>","cwd":"<...>","prompt":"<...>"}
     ```
     `prompt_id` は hook 入力 JSON の `prompt_id` フィールド(Claude Code の
     フック共通入力に載る UUID — Anthropic, "Hooks reference",
     <https://code.claude.com/docs/en/hooks>、2026-09-14 取得)をそのまま使う。
     欠けている場合のみ `$(date +%s%N)-$$` 形式でフォールバック生成する。
   - `Stop` 1 行:
     ```json
     {"kind":"turn_end","agent":"claude-code","ts":"<ISO8601 UTC>","session_id":"<...>"}
     ```
   - フィールド名・`kind` の値・ファイルパスは消費側(別リポジトリ)が直接
     依存する契約であり、このリポジトリ単独の判断で変更しない。
3. **atuin 自身のエージェントフックも同じ配線経路に乗せる。** atuin は
   Bash tool 呼び出しの command/cwd/duration/exit code を独自に記録する
   `atuin hook claude-code` を提供している —
   Atuin, "Agent Hooks" (<https://docs.atuin.sh/latest/guide/agent-hooks/>、
   2026-09-14 取得): 「Claude Code calls `atuin hook claude-code` on each
   `Bash` tool use, passing the event as JSON on `stdin`」。`atuin hook
   install claude-code` は `~/.claude/settings.json` を直接書き換え、この
   リポジトリの宣言的マージ(`register()`)の外側に手書き状態を作るため
   採らない。代わりに既存の `register()` 経由で `PreToolUse`/`PostToolUse`/
   `PostToolUseFailure`(いずれも matcher `Bash`)の 3 イベントすべてに
   `atuin hook claude-code` を登録する(成功/失敗どちらの exit code も
   拾うため `PostToolUse` だけでは足りない)。
4. **両方とも「採取のみ」であり、解釈・集計・射影は行わない。** JSONL への
   追記も `history.db` への記録も一方向の事実の蓄積であり、このリポジトリは
   それらを読み返したり加工したりしない。ingestion(誰が・いつ・どう読むか)
   は完全に downstream の責務(このリポジトリの範囲外)。

## Alternatives considered

- **atuin sync を有効にする**(自前サーバまたは atuin.sh)— アカウント登録
  という運用コストと、家庭外のサーバへ生コマンド履歴が渡る面(secrets_filter
  はあるが完全ではない)を天秤にかけ、単一マシンのローカル評価用途では
  不要と判断。棄却。
- **Claude Code の hook を使わず transcript ファイル(`transcript_path`)を
  後から解析する** — 1 ターン = 1 行という単純な事実だけを消費側に渡せば
  足り、transcript 全文を都度パースする責務を downstream に負わせるのは
  過剰。フックでイベント境界だけを薄く記録する方を採る。
- **`atuin hook install claude-code` を使う** — 導入は簡単だが、
  `~/.claude/settings.json` を直接書き換えるため home-manager の宣言的
  管理(ADR-0001)と衝突する。既存の `register()`/`retire()` 冪等マージに
  乗せる方を採る。

## Consequences

- `home/modules/atuin.nix`(新規)を `home/common.nix` の import に追加。
- `config/claude/hooks/agent-turn-log.sh`(新規)を追加し、
  `home/modules/claude.nix` の `register()` 経由で
  `UserPromptSubmit`/`Stop` に配線する。
- `home/modules/claude.nix` の `register()` 経由で `atuin hook
  claude-code` を `PreToolUse`/`PostToolUse`/`PostToolUseFailure`
  (matcher `Bash`)に配線する。
- どちらの出力もプレーンテキストで平文のままローカルに残る
  (暗号化しない、同期しない)。このリポジトリはそれ以上のライフサイクル
  管理(ローテーション・削除)を持たない — 必要になれば別 ADR で扱う。
- 消費側(別リポジトリ)がこの契約を読む実装は、このリポジトリの管轄外。
  契約を破る変更(パス・フィールド名の変更)をする場合は、消費側との
  互換性検討が先に必要になる — この ADR 自体は immutable なので、破壊的
  変更は新しい ADR を起こして supersede すること(ADR-0008 の規約)。

## Verification

- `nix flake check` — 3 ホストとも green
  (`checks.x86_64-linux.{personal-pop,company-pop-old,company-pop-new}`)。
- `shellcheck config/claude/hooks/agent-turn-log.sh` — clean。
- `register-claude-hooks` を実際にビルドし、テスト用 `settings.json` に対して
  実行して確認: `UserPromptSubmit`/`Stop` に agent-turn-log の command が
  1 件ずつ、`PreToolUse`/`PostToolUse`/`PostToolUseFailure`(matcher
  `Bash`)に `atuin hook claude-code` が 1 件ずつ登録されること。2 回連続
  実行しても重複登録されないこと(冪等性)。
- `agent-turn-log.sh` にサンプルの hook 入力 JSON(埋め込み改行・ダブル
  クオート・トークン様文字列を含むプロンプト、`prompt_id` 欠落ケース)を
  標準入力から渡し、`agent-events.jsonl` に 1 行 = 1 JSON(jq でパース可能)
  で追記されること、ディレクトリ/ファイルの権限が 700/600 になること、
  `prompt_id` 欠落時にフォールバック生成されることを確認。
- `nix build .#checks.x86_64-linux.personal-pop` で `atuin-config.drv` が
  生成する TOML の値が `home/modules/atuin.nix` の宣言と一致することを
  derivation の `__json.value` で確認(`update_check`/`auto_sync` が
  `false`、`secrets_filter`/`store_failed` が `true`)。
