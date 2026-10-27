# Claude と Codex はそれぞれの native 権限設定を使う

共有権限ポリシーの実行時管理は撤去した。Claude は Claude 専用 reconciler、
Codex は native 設定を使う。所有記録が無いことや、設定を本人が変更したことを
理由に router が通常のツール実行を止める仕組みは無い。裁定は
[ADR-457 の 2026-10-27 Amendment](../adr/457-claude-native-install-source-of-truth.md) に残す。

## 設定の正本と適用後の比較対象

| 対象 | 宣言・執行点 | 適用する内容 |
|---|---|---|
| Claude | `home/modules/claude.nix`、`agent-settings-reconcile claude-permissions` | native の allow / ask と、それぞれの撤回リスト |
| Codex | `home/modules/codex.nix`、`agent-settings-reconcile codex-permissions` | `on-request`、`auto_review`、名前付き `dotfiles` 権限 profile、通信 proxy、秘密パスの masking、個別 MCP 設定 |

Claude の追加許可は `dotfiles.claudePermissions.extraAllow`、Codex の入力は
`dotfiles.codexPermissions.secretPaths` / `domains` / `mcpAllow` / `apps` に置く。
非公開の実値は ADR-0034 に従い private wrapper に置く。
既存 3 ホストとの互換性のため `dotfiles.agentPermissions` の option は残るが、
旧共有 policy ファイル・所有検査・SessionStart の回収は配備しない。
認証 launcher は維持し、資格情報を渡す前に native 設定を検査する。共有の所有記録は参照しない。
旧入力は native option の既定値に使い、以後の宣言は native option に書く。

生成した比較対象は `~/.config/agent-native/claude-permissions.json` と
`~/.config/agent-native/codex-permissions.json`。前者は Claude reconciler の spec、
後者は `settings` と `mcp_allow` を持つ Codex の spec である。
両 CLI の設定全体を symlink にしないため、モデル・trust などの本人の設定は書き込み可能なまま残る。
Claude の `permissions.defaultMode = "plan"` と `autoMode.environment` は撤去時にも保持する。

Codex の native writer は宣言した名前の profile だけを置き換え、古い通信先や書き込み許可を残さない。
ほかの profile・モデル・trust は保持する。`default_permissions` を宣言するときは競合する旧
`sandbox_mode` / `sandbox_workspace_write` を取り除く。これらの置換前には同じディレクトリに
権限 0600 の `config.toml.native-permissions-backup.<内容のSHA-256>` を作り、現在値の一致を確かめて更新する。


## 認証 launcher は native の保護を確かめる

`crates/codex-launcher/src/main.rs` の `validate_config` は、認証付き起動の前に
`on-request` / `auto_review`、proxy と credential masking、選択した workspace profile を検査する。
保護を上書きする profile・project 設定や別設定ホームは拒否し、確認した profile を起動時に指定する。
CLI の版・引数も確認してから資格情報を渡す。共有の移行済み記録や hash への依存は無い。
この検査は認証情報の引き渡し条件であり、全ツール呼び出しを共有所有検査へ通すものではない。

## 旧共有管理を撤去する

既存手段: `agent-permissions` — 拡張。既存の transaction journal の変更前・変更後を使う
`retire` を追加し、別の移行ツールは作らない。通常の activation はこの撤去を native writer より先に行う。
手元で変更候補だけを見る場合は、次の順で行う。

```sh
agent-permissions retire --home "$HOME" --dry-run
agent-permissions retire --home "$HOME"
```

`--home` は撤去対象ホームの絶対パス。dry-run は変更候補を JSON で返し、書き込まない。
実行は現在の所有記録からたどれる journal だけを使い、取り消された移行は除外する。
証明できる共有変更だけを逆適用し、現在値が記録後の値と違う箇所は本人の編集として保持する。
後から加えた許可や個別 MCP の許可も保持する。完了は transaction と読み戻しで確かめ、
撤去済みの印を残す。同じホームへ再実行しても共有設定を作り直さない。

journal が無い場合は保守的に扱う。共有管理の名前付き selector とその profile の撤去だけを対象にし、
由来を証明できないローカル設定・project 設定・rules は削除しない。
未完了の transaction がある場合は、その回復を先に行い、撤去で上書きしない。
旧 `inspect` / `migrate` / `apply` / `check` は互換 CLI の入口として残るが、通常実行や activation の前提ではない。
旧 promotion queue は自動で再適用しない。必要な許可は `/promote-permissions` から native 宣言として提案する。

## 承認経路は upstream のコードに基づいて区別する

以下は OpenAI, *Codex*, `rust-v0.164.0`、commit
`6aaac694803004a63c6f393d153c96d22521a6b9` のコードを読んだ範囲である。
公開日未確認、取得日 2026-10-27。

- 承認要求は PermissionRequest hook の allow / deny を先に適用し、判定が無いときに reviewer へ進む。
  `auto_review` を宣言しても既存 hook の拒否を迂回しない。
  [`tools/approvals.rs` L484–505](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/tools/approvals.rs#L484-L505)。
- reviewer は guardian が応答すればその結果を採り、応答が無い経路は user へ進む。
  全ツールが必ず自動承認されるという意味ではない。
  [`tools/approvals.rs` L552–583](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/tools/approvals.rs#L552-L583)。
- Apps は app policy、custom MCP は server/tool の approval mode を使う。
  [`mcp_tool_call.rs` L219–229](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/mcp_tool_call.rs#L219-L229)。
  Apps の reviewer は link → app → app default の順に見て、requirements が許す指定を使う。
  custom MCP はその app 固有の上書きを使わず、既定 reviewer を使う。
  [`connectors.rs` L514–550](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/connectors.rs#L514-L550)。

設定の parse・読み戻しと、reviewer が実際に審査したことは区別する。
今回の nested sandbox 内では reviewer の end-to-end 実行は未検証。
既存の過去の実測を、今回の設定で実測した結果として流用しない。
