# settings-reconcile — 宣言を settings 系ファイルへ反映する

`~/.claude/settings.json`・`~/.codex/hooks.json`・`~/.copilot/settings.json`・
`~/.claude.json` はツール自身が実行時に書き換えるので、store symlink にできない。
home-manager は activation のたびに、**宣言したものだけ**をこれらへ冪等に反映する。
実装は `crates/settings-reconcile`(Rust、ADR-0024、#414)で、かつて
`home/modules/claude.nix` 等の `writeShellScript` に埋め込んだ jq と
`scripts/register-{codex,copilot}-hooks` に分かれていた 6 本を 1 つにまとめたもの。

| サブコマンド | 書き先 | 宣言の渡し方 |
|---|---|---|
| `claude-hooks` | `~/.claude/settings.json` の `.hooks` | `builtins.toJSON` の spec(`claudeHookDeclarations` + `retiredHookEntries`) |
| `claude-permissions` | 同 `.permissions.allow` / `.ask` | spec(`permissionRules` ほか) |
| `claude-statusline` | 同 `.statusLine` | spec(`statusLineCmd` + `retiredStatusLineCommands`) |
| `claude-mcp-servers` | `~/.claude.json` の `.mcpServers` | `dotfiles.claude.mcpServers` の JSON |
| `codex-hooks` | `~/.codex/hooks.json` | argv(`--retire <event> <command>`… `--register` `<event> <matcher> <command> <timeout>`…) |
| `copilot-hooks` | `~/.copilot/settings.json` | argv(`<event> <command> <timeoutSec>`) |

## hook は宣言を正とする

hook は `(event, command)` をキーに reconcile する。旧実装は「command が完全一致する
エントリが既にあれば何もしない」だったので、**`matcher` / `if` / `timeout` を宣言側で
変えても既存エントリが更新されなかった**(#414 以前の既知の罠)。今は command が
一致するハンドラの `matcher` / `if` / `timeout` を宣言に合わせる:

- 宣言で省略した `matcher` / `if` / `timeout` は、既存エントリからも外す。
- 同じ command の重複エントリは 1 つに畳む。
- 他ツールのハンドラと同じグループに相乗りしていて matcher が変わるときは、グループ
  自体の matcher を変えず(相手の発火条件まで変わる)、そのハンドラだけ新グループへ移す。
- ハンドラが持つ未知のキー(ユーザーや他ツールが足したもの)は残す。
- 宣言の中で `(event, command)` が重複したら最初のものだけ採り、stderr に警告する。

`retire`(旧 command の完全一致削除)の意味は旧実装のまま: 一致したハンドラだけ外し、
空になったグループとイベントキーを畳む。retire を先に、register を後に適用する。
command 文字列そのものを変える場合は別キーなので、旧文字列を `retiredHookEntries`
(Claude)/ `--retire`(Codex / Copilot)に移す運用は従来どおり。

## 書き込みの性質

- 宣言と実体が同値なら書かない(定常状態で mtime を汚さない)。
- 書くときは同じディレクトリの一時ファイルへ書いて rename(別 fs への mv は非原子的)。
  mode は元ファイルに合わせる。
- キーの並びと字下げは jq の既定出力と同じに保つ(順序保存の自前 Value 型。
  `serde_json` の `preserve_order` はワークスペース全体の feature unification で
  他クレートの出力順を変えるため使わない)。
- ファイルが無ければ親ディレクトリごと `{}` で作る。JSON として壊れていれば
  何も書かず exit 1。usage エラー(引数の個数など)は exit 2。

## テスト

`crates/settings-reconcile/tests/fixtures/<case>/` に `before.json` / `steps.json` /
`after.json` を置き、`tests/reconcile.rs` が全 case を「期待どおりになる」「同じ宣言を
もう一度流しても変わらない(冪等)」で検査する。挙動を変えていない case の
`after.json` は旧実装(bash + jq)を oracle にして生成した。意図的に変えた挙動
(上記の宣言を正とする更新)は手書き。
