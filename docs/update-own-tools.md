# update-own-tools — 自作 pre-release CLI の更新

ADR-0025 の実装(#276)。コードは `crates/update-own-tools`(Rust、ADR-0024)で、
`~/.local/bin/update-own-tools` として配備する。

## レジストリ

置き場所は `${XDG_CONFIG_HOME:-~/.config}/update-own-tools/registry.toml`。
**このファイルはホストローカルで、dotfiles には書かない。** 対象リポジトリの名前は、
このリポジトリのソース・Issue・PR のどこにも現れない(ADR-0025 / ADR-0034)。

```toml
[[tool]]
name = "<tool>"                     # 表示名 / 引数での絞り込み / キャッシュ名
repo = "<local clone path or URL>"  # ローカルのクローン、または clone できる URL
crate = "<subdir>"                  # 省略時は "."(リポジトリ相対)
branch = "main"                     # 省略時は "main"
# 省略時は下の値。{dir} = crate の絶対パス、{cache} = ツールごとの target dir
install = ["cargo", "install", "--locked", "--path", "{dir}", "--target-dir", "{cache}"]
```

未知のキーはエラーにする(typo を黙って既定値に落とさないため)。

## 動作

```
update-own-tools [--dry-run] [--registry <path>] [<name>...]
```

各 `[[tool]]` について、次の順に実行する。

1. `repo` がローカルのディレクトリならそのまま使う。URL なら、
   `${XDG_CACHE_HOME:-~/.cache}/update-own-tools/repos/<name>.git` に bare
   ミラーを作る。ミラーを作るのは初回だけ。
2. `git fetch origin +refs/heads/<branch>:refs/remotes/origin/<branch>`
3. `git worktree add --detach <cache>/worktrees/<name>-<pid> refs/remotes/origin/<branch>`
4. worktree 内の crate ディレクトリで `install` を実行する。
5. `git worktree remove --force`。4 までが失敗しても必ず実行する。

- 対象リポジトリのチェックアウト(HEAD・ブランチ・未コミットの変更)には触れない。
  変わるのはリモート追跡 ref(`origin/<branch>`)だけ。
- `--dry-run` は実行予定のコマンドを出力するだけで、キャッシュも作らない。
- `cargo` が PATH に無いときは明示的に失敗する。ツールチェインは rustup など
  per-project runtime で用意する(ADR-0002)。
- on-demand でのみ実行する。systemd timer は張らない(ADR-0025 D3)。

## 退出手順(タグ付きリリースに到達したとき)

ADR-0025 D4 に従う。dotfiles 側のコードは変更しない。

1. ホストのレジストリから、該当する `[[tool]]` を削除する。
2. そのツールを `home.packages`(nixpkgs か flake input)に足す。
   どの層に足すかは `docs/operations.md` の判定フローに従う。
3. `hms` で適用し、`~/.cargo/bin/<tool>` に残った旧バイナリを
   `cargo uninstall <crate>` で削除する。PATH の優先順位(ADR-0029)上、
   `~/.cargo/bin` は nix より後ろにあるので、残っていても shadow はしない。
