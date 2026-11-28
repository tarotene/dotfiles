# agent-settings-reconcile — 宣言を settings 系ファイルへ反映する

`~/.claude/settings.json`・`~/.codex/hooks.json`・`~/.copilot/settings.json`・`~/.claude.json`・`~/.gemini/antigravity-cli/settings.json` はツール自身が実行時に書き換えるため、Nix store の symlink にできない。
home-manager は activation のたびに、**宣言したものだけ**をこれらへ冪等に反映する。
実装は `crates/agent-settings-reconcile`(Rust、ADR-0024、#414)であり、かつて `home/modules/claude.nix` 等の `writeShellScript` に埋め込んでいた jq と `scripts/register-{codex,copilot}-hooks` に分かれていた 6 本のスクリプトを 1 つに統合したものである。

| サブコマンド | 書き先 | 宣言の渡し方 |
|---|---|---|
| `claude-hooks` | `~/.claude/settings.json` の `.hooks` | `builtins.toJSON` の spec(`claudeHookDeclarations` + `retiredHookEntries`) |
| `claude-permissions` | 同 `.permissions.allow` / `.ask` | spec(`permissionRules` ほか) |
| `claude-statusline` | 同 `.statusLine` | spec(`statusLineCmd` + `retiredStatusLineCommands`) |
| `claude-mcp-servers` | `~/.claude.json` の `.mcpServers` | `dotfiles.agents.mcpServers`(旧名 `dotfiles.claude.mcpServers`)の JSON |
| `codex-hooks-spec` | `~/.codex/hooks.json` | spec(`codexHookDeclarations` + 退役リスト + `state`) |
| `copilot-hooks-spec` | `~/.copilot/settings.json` | spec(`copilotHookDeclarations` + 退役リスト + `state`) |
| `codex-config` | `~/.codex/config.toml` | spec(`home/modules/codex.nix` の `codexConfigSpec`) |
| `codex-permissions` | 同 `default_permissions` の権限 profile と MCP 許可 | `{settings, mcp_allow, state}` |
| `antigravity-permissions` | `~/.gemini/antigravity-cli/settings.json` の `.permissions.{allow,ask,deny}`・`.toolPermission`・`.enableTerminalSandbox` | spec(`home/modules/antigravity.nix`。`state` は 3 種の規則を記録) |
| `antigravity-hooks` | `~/.gemini/config/hooks.json` の自分の名前のキー(`dotfiles`) | spec(`antigravityHooksSpec`: `{name, hooks}`。所有の単位はキーなので、`state` は持たない。agy が捨てる形の宣言は書く前に止める) |
| `antigravity-mcp-servers` | `~/.gemini/config/mcp_config.json` の `.mcpServers` のうち、所有の記録に載せた名前 | spec(`home/modules/antigravity.nix` の `mcpSpec`: `{servers, state}`。agy が捨てるキーを持つ宣言は書く前に止める。空のファイルは `{}` として読む) |

## hook は宣言を正とする

hook は `(event, command)` をキーに reconcile する。旧実装は「command が完全一致するエントリが既にあれば何もしない」という挙動だったため、**`matcher` / `if` / `timeout` を宣言側で変えても既存エントリが更新されなかった**(#414 以前の既知の罠)。現在は command が一致するハンドラの `matcher` / `if` / `timeout` を宣言に合わせる:

- 宣言で省略した `matcher` / `if` / `timeout` は、既存エントリからも削除する。
- 同じ command の重複エントリは 1 つに統合する。
- 他ツールのハンドラと同じグループに相乗りしていて matcher が変わる場合は、グループ自体の matcher を変えず(相手の発火条件まで変わるのを防ぐため)、該当ハンドラだけを新しいグループへ移す。
- ハンドラが持つ未知のキー(ユーザーや他ツールが追加したもの)は保持する。
- 宣言内で `(event, command)` が重複した場合は最初のエントリだけを採用し、stderr に警告を出す。

`retire`(旧 command の完全一致削除)の意味は旧実装と同じく、一致したハンドラだけを削除し、空になったグループとイベントキーを取り除く。retire を先に、register を後に適用する。
command 文字列そのものを変える場合は別キー扱いになるため、旧文字列を `retiredHookEntries`(Claude)や `--retire`(Codex / Copilot)へ移す運用は従来どおりとする。

## 所有の記録(`claude-hooks`・`codex-hooks-spec`・`copilot-hooks-spec`)

宣言から外した hook を消すために退役リストへ command を書き足す方式は、書き忘れると旧エントリが残り続けた(存在しないパスを指してイベントごとに ENOENT になる、#44)。そこで `claude-hooks` の spec に `state`(記録ファイル、`~/.local/state/dotfiles/managed-hooks-claude.json`)を渡すと、`crates/agent-settings-reconcile/src/managed.rs` が**前回書き込んだ `(event, command)` の集合**を保存し、次回の適用時に「前回書き込んだが今回の宣言にはないもの」を削除する。

- 所有権は記録だけで判定し、command の接頭辞(`~/.claude/hooks/`)では判断しない。同じディレクトリに別ツールが独自に追加したエントリ(herdr の `herdr-agent-state.sh`)があるが、記録にないため変更しない(`tests/managed_record.rs` の `entries_we_did_not_write_…`)。
- 退役リストは凍結した。新しい `legacy*Cmd` は追加しない。現在効くのは記録のないホストでの初回適用時のみであり、全ホストに記録ができたら一覧を削除できる。
- 記録がない場合や壊れている場合は初回として扱い(stderr に警告を出す)、適用自体は止めない。
- 記録は settings.json の書き込みが成功した後にだけ更新する。内容が同値なら書き換えない。
- `state` を渡さなければ従来どおり記録を作らない。
- Codex と Copilot も同じ spec と記録で登録する(`managed-hooks-codex.json`・`managed-hooks-copilot.json`)。以前は 1 つのファイルに対して複数モジュール(`agents.nix`・`herdr.nix`・`worktree.nix`)の 10 の呼び出し(argv 形式 9・spec 形式 1)が別々の部分集合を登録していたため、記録を 1 つにすると別の呼び出しが登録した分を「宣言から外れた」と誤認して削除してしまった(Codex の共通分だけが記録を持ち、herdr と worktree の分は持たなかった)。現在は宣言を `agents.nix` の 1 か所に集約し、1 回の activation で 1 ファイル全体を書き込む。
- Codex の `/hooks` の信頼は command・timeout・`statusMessage` から生成する識別子に基づくため、herdr の 5 行は従来の表示名 `herdr agent metadata` のまま登録する(ノードの label に揃えると 3 機体すべてで再信頼が必要になるため)。

## 書き込みの性質

- 宣言と実体が同値なら書き換えない(定常状態で mtime を汚さない)。
- 書き込むときは同じディレクトリ内の一時ファイルへ書いてから rename する(別ファイルシステムへの mv は非原子的)。パーミッションは元ファイルに合わせる。
- キーの並び順とインデントは jq の既定出力と同じに保つ(順序を保持する自前 Value 型を使用。`serde_json` の `preserve_order` はワークスペース全体の feature unification で他クレートの出力順を変えるため使わない)。
- ファイルがなければ親ディレクトリごと `{}` として作成する。JSON として壊れている場合は何も書かず exit 1 で終了する。usage エラー(引数の個数誤りなど)は exit 2 で終了する。

## テスト

`crates/agent-settings-reconcile/tests/fixtures/<case>/` に `before.json` / `steps.json` / `after.json` を配置し、`tests/reconcile.rs` が全ケースについて「期待どおりの結果になること」および「同じ宣言を再度適用しても変化しないこと(冪等性)」を検証する。挙動を変えていないケースの `after.json` は旧実装(bash + jq)を正解として生成し、意図的に変更した挙動(上記の宣言を正とする更新)は手書きで用意した。
