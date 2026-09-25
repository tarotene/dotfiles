# public-publish-guard — 上流リポジトリへの配線(dotfiles 固有部分)

ファイル名は当時(ADR-0009)の名残のまま残す(ADR-0007 の遡及的な一括
リネーム禁止と同型 — このドキュメント自身の識別子として、19 ファイル
以上から相対パスで参照されている)。上流リポジトリは 2026-09-24 に
`publish-guard` から `bleep` へ改名した(tarotene/publish-guard#28)。
以下は改名後の現状を記述する。

設計根拠・判定ロジック・denylist/allowlist の仕組み・security boundary
ではないことの説明は、上流リポジトリ
[tarotene/bleep](https://github.com/tarotene/bleep)(旧
`tarotene/publish-guard`)の README が正本(ADR-0008 決定1: 実装物の
利用者向け説明は実装物のリポジトリに置く)。ここに書くのは **dotfiles
側の配線だけ**。

なぜ別リポジトリに切り出したかの決定は `docs/adr/0009-publish-guard-
upstream-split.md` を参照(ADR ファイル名・本文は改名に追従させない、
D7 と同型)。

## dotfiles での配線

- `flake.nix` が `bleep` を `flake = false` の入力として commit SHA に
  pin する(`tarotene/bleep` は独立に semver/tag を持てるが、
  `tarotene/dotfiles` 自身は ADR-0004 の「No semver releases」のまま —
  だからこそ別リポジトリにした)。
- `home/modules/claude.nix` は上流のツリー全体を `home.file
  ".claude/hooks/bleep"` として**1つのディレクトリ symlink**で配る
  (個々のファイルを列挙しない)。上流の shim(`hooks/bleep.sh`)は
  `realpath "$0"` で自己解決するため(旧 `claude-adapter.sh` と違い
  `CLAUDE_PLUGIN_ROOT` の明示注入は不要)、ツリー構造をそのまま保てば
  それ以上の配慮は要らない。
- 登録する command:
  ```
  bash '$HOME/.claude/hooks/bleep/hooks/bleep.sh' --host=claude
  ```
  旧 `claude-adapter.sh` は plugin マーケットプレイス経由のインストール
  時に Claude Code が自動設定する `CLAUDE_PLUGIN_ROOT` に依存していたため、
  `home.file` による直接配備ではこの env var を明示的に前置する必要が
  あった。`hooks/bleep.sh`(#25-28 の Rust hook cutover で旧 3 adapter を
  統合した単一 shim)は自己解決するため、この配慮そのものが不要になった。
- matcher は複合1本 `"Bash|mcp__.*"`。`register()`(この nix ファイル内)の
  存在判定は command 文字列の完全一致だけで matcher を見ないため、Bash と
  MCP を2つの hook エントリに分けると2回目の登録が早期 return し、MCP
  経路が無検査のまま残る(旧実装が matcher `"Bash"` 単体だったために持って
  いた最大の機能欠陥そのもの)。
- 旧 command 3 世代分が `retiredHookEntries` で完全一致削除される:
  `bash '$HOME/.claude/hooks/public-publish-guard.sh'`(ADR-0009 分離前)、
  `CLAUDE_PLUGIN_ROOT=... bash '.../publish-guard/hooks/claude-adapter.sh'`
  (#25-28 の Rust hook cutover前)。matcher/timeout は既存エントリの
  command が変わらない限り更新されない(この nix ファイル内 `register()`
  のコメント参照)。
- Codex CLI(`~/.codex/hooks.json`)/ Copilot CLI(`~/.copilot/settings.json`)
  にも同じ shim を配線する(#160)。旧 `adapters/{codex,copilot}-adapter.sh`
  の command は register-{codex,copilot}-hooks に追加した `--retire` で
  完全一致削除してから新 command(`hooks/bleep.sh --host={codex,copilot}`)
  を登録する — Claude 側の `retiredHookEntries` に相当する仕組みが元々
  無かったため、改名 companion PR でこの2スクリプトに追加した。
- `config/git/hooks/pre-push`(`core.hooksPath` で全リポジトリ共通、
  `docs/git-sync.md`)が `scan-push` を呼ぶ(#196)。エンジン不在は黙って
  スキップ(ADR-0005)。exit 1(ask)/2(deny)のどちらも push を block する
  (pre-push に対話の経路が無いため)。回避は `BLEEP_ALLOW=1 git
  push`。PRIVATE/INTERNAL リポジトリの自動スキップは engine 側の判定。

## `~/.config/bleep/` の実体(dotfiles には無い)

`bleep` の判定語彙(`orgs.txt`・`repos.txt`)と、CLI ツール名と衝突する
private リポジトリ名を通す `allow-stopwords.txt`(bleep README
「Marking a private repo as "prospectively public"」節が正本、
tarotene/bleep#15 → PR #19 で決着)は、いずれも実値を含むため PUBLIC な
この dotfiles リポジトリには置かない(ADR-0034)。3 ファイルとも private
wrapper flake が `~/.config/dotfiles/private-hub` マーカー経由で生成する
(`scripts/hms.sh` の `resolve_default_ref()` と同じ間接参照。ADR-0034
Decision 6 と同型 — 正本は wrapper flake 側、ここはスキーマ・手順だけを
記す)。`allow-stopwords.txt` の各行は「実際には CLI ツール名としての
言及であり private リポジトリ名との衝突ではない」と判断した文字列を、
コメントで理由を添えて 1 行ずつ追記する(#431)。

## 未検証の前提

- Claude Code hook の `deny` が `bypassPermissions` 下でも効くかは公式
  ドキュメントで断定できていない(上流 README・ADR-0009 と同じ記録)。
