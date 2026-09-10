# public-publish-guard — 上流リポジトリへの配線(dotfiles 固有部分)

設計根拠・判定ロジック・denylist/allowlist の仕組み・security boundary
ではないことの説明は、上流リポジトリ
[tarotene/publish-guard](https://github.com/tarotene/publish-guard) の
README が正本(ADR-0008 決定1: 実装物の利用者向け説明は実装物のリポジトリに
置く)。ここに書くのは **dotfiles 側の配線だけ**。

なぜ別リポジトリに切り出したかの決定は `docs/adr/0009-publish-guard-
upstream-split.md` を参照。

## dotfiles での配線

- `flake.nix` が `publish-guard` を `flake = false` の入力として commit
  SHA に pin する(`tarotene/publish-guard` は独立に semver/tag を持てるが、
  `tarotene/dotfiles` 自身は ADR-0004 の「No semver releases」のまま —
  だからこそ別リポジトリにした)。
- `home/modules/claude.nix` は上流のツリー全体を `home.file
  ".claude/hooks/publish-guard"` として**1つのディレクトリ symlink**で配る
  (個々のファイルを列挙しない)。上流の Claude adapter
  (`hooks/claude-adapter.sh`)は「自分の2階層上に `publish-guard` 本体が
  ある」という plugin 配布時のレイアウトを前提に自己解決するため、ツリー
  構造をそのまま保つ必要がある。
- 登録する command は `CLAUDE_PLUGIN_ROOT` を明示的に前置する:
  ```
  CLAUDE_PLUGIN_ROOT='$HOME/.claude/hooks/publish-guard' \
    bash '$HOME/.claude/hooks/publish-guard/hooks/claude-adapter.sh'
  ```
  plugin マーケットプレイス経由のインストールでは Claude Code がこの env
  var を自動的に設定するが、`home.file` による直接配備ではその機構を
  経由しないため、ここで明示的に渡す(adapter 自身のパス自己解決フォール
  バックには頼らず、上流が文書化している経路を使う)。
- matcher は複合1本 `"Bash|mcp__.*"`。`register()`(この nix ファイル内)の
  存在判定は command 文字列の完全一致だけで matcher を見ないため、Bash と
  MCP を2つの hook エントリに分けると2回目の登録が早期 return し、MCP
  経路が無検査のまま残る(旧実装が matcher `"Bash"` 単体だったために持って
  いた最大の機能欠陥そのもの)。
- 旧 command(`bash '$HOME/.claude/hooks/public-publish-guard.sh'`)は
  `retiredHookEntries` で完全一致削除してから新 command を登録する —
  matcher/timeout は既存エントリの command が変わらない限り更新されない
  (この nix ファイル内 `register()` のコメント参照)。

## 未検証の前提

- Codex CLI / Copilot CLI 向けの adapter(上流リポジトリが提供)を
  `~/.codex/hooks.json` / `~/.copilot/settings.json` に配線するかどうかは
  ADR-0009 の対象外(未実施)。
- Claude Code hook の `deny` が `bypassPermissions` 下でも効くかは公式
  ドキュメントで断定できていない(上流 README・ADR-0009 と同じ記録)。
