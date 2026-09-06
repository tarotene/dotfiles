# ADR-0007 — 命名・配置規約

- Status: Accepted
- Date: 2026
- Issue: #98

## Context

リポジトリが継ぎ足しで成長し、命名・レイヤ分けの一貫性が徐々に失われつつある
という懸念から、著名な Nix dotfiles(mitchellh/nixos-config、
Misterio77/nix-starter-configs、hlissner/dotfiles)と Claude Code hook
エコシステムを比較対象に Research → 敵対的レビューを実施した(#98)。

確定した事実:

- `scripts/` 内の `.sh` 拡張子・shebang が2〜3系統混在していたが、Google
  Shell Style Guide の「PATH経由でコマンドとして呼ぶものは拡張子なし、
  source されるライブラリのみ `.sh`」という慣行とはおおむね整合しており、
  実害があったのは `sops-secrets-env.sh` の配備名1箇所のみだった(#104 で是正)。
- `docs/claude/*.md` と実体ファイル名のズレは6件報告されたが、実害(参照が
  完全に壊れている)があったのは1件のみで、残りは1実体に複数docが対応する
  正当なケースだった(#104 で是正)。
- 環境変数接尾辞 `_ROOT`/`_DIR`/`_STATE_DIR` の混在は、外部に権威的な規範が
  ないため「自前定義」と割り切ることにした(#104 で一部是正)。
- hook の役割語彙(`-gate`/`-guard`/`-allow`/無印)は、外部調査で「支配的な
  業界慣行が存在しない」ことが判明した。安易な全面リネームは
  `docs/claude/*.md` 6件以上・`config/herdr/config.toml` のハードコード
  実行パスまで波及するのに対し、得られる外部整合性の利得はゼロだった
  (#98 敵対的レビューの結論)。
- `scripts/` を「escape-hatch and diagnostic scripts」と規定していた
  `CLAUDE.md` の記述自体が、実態(15本中10本が home-manager 経由で恒常配備
  される正規ユーザ環境ツール)と乖離しており、逸脱していたのは実装ではなく
  規定文言の側だった(#105 で是正)。
- `~/.local/bin` 配備定義がモジュール横断で分散している点、SOPS が複数箇所に
  またがっている点は、実際には各モジュールが自分のサブシステムの範囲で完結
  しており、意図的な分割だった(#98 敵対的レビューで REJECTED)。

この ADR は、これらの確定事実を踏まえ、**今後の新規ファイルが従うべき最小限の
規約**を成文化する。既存ファイルの遡及的な一括リネームはしない
(壊れる参照のコストが、得られる一貫性の利得を上回るケースが大半だった)。

## Decision

### 1. 拡張子ポリシー

- `~/.local/bin` や `git-<subcommand>` として PATH 経由で直接呼び出す実行
  ファイルは拡張子なしで配備する。リポジトリ内のソースファイル名が `.sh` を
  持っていてもよいが、`home.file` 等の配備定義側で拡張子を落とす。
- `source`/`.` されるだけのライブラリスクリプトは `.sh` を保持する。

### 2. shebang ポリシー

- bash 前提のスクリプトは `#!/usr/bin/env bash`(ShellCheck 推奨)。
- 生成される標準出力のみを消費する真の POSIX sh スクリプトは `#!/bin/sh` を
  維持してよい(`shellcheck -s sh` でクリーンであることが条件)。
- zsh スクリプトは `#!/usr/bin/env zsh`(shellcheck 非対応、対象外)。

### 3. hook の役割語彙

| 接尾辞 | 意味 |
|---|---|
| `-guard` | PreToolUse で遮断(`permissionDecision: "deny"`)する hook |
| `-gate` | Stop の完了バリア hook |
| `-allow` | PreToolUse で検証つき `permissionDecision: "allow"` を返す hook。`-guard` とは意味的に別物(遮断ではなく許可の裏付け) |
| 接尾辞なし | SessionStart 注入系などイベント名を含まない機能名 |

既存ファイルはこの表に従って遡及的にリネームしない。新規に hook を追加する
ときにのみ適用する。

### 4. `config/claude/hooks/` ソースツリーの純度

`config/claude/hooks/` 直下は hook 本体スクリプトのみとする。同梱アセット
(`*.schema.json`、`*.css`)は `config/claude/assets/` へ、statusline のような
「hook でない」仕組みは `config/claude/statusline/` へ分離する(#106 実施分)。

これは**ソースツリー側の規約**であり、配備先(`~/.claude/hooks/`)の規約では
ない。`plan-view.sh` の `find_css()` や `config/herdr/config.toml` の実行
パスハードコードなど、配備先パスに依存する消費者が存在するため、配備先では
アセットが hook と同居し続けてよい。

### 5. `docs/claude/*.md` 対応原則

「1 hook = 1 doc」の厳密対応は強制しない。機能的にひとまとまりの複数 hook
(例: `wrapup-stop-gate.sh` + `wrapup-session-start.sh` を1つの
`wrapup-inbox.md` でカバーする)を1 doc にまとめることを許容する。ただし:

- doc を持たない hook(孤立 hook)を作らない
- 実体を持たない doc(孤立 doc)を作らない
- doc 名は主たる実体のファイル名または機能名を反映する

### 6. 環境変数接尾辞

- ディレクトリを指す環境変数は `<SUBSYSTEM>_DIR` を基本形とする。
- 永続状態(XDG state 配下)のみ `<SUBSYSTEM>_STATE_DIR`。
- `_ROOT` は新規に使わない。既存の `pr-gate.sh` の `STATE_ROOT`(親子関係を
  表す意図的命名)と `42-dev-ros.zsh` の `ROS_ROOT`(ROS 本家の標準変数名)
  は、それぞれ理由があるため本 ADR の対象外(grandfather)とする。

### 7. `scripts/` の位置づけ

`scripts/` は「escape-hatch and diagnostic scripts」専用ディレクトリでは
ない。home-manager 経由で `~/.local/bin` 等へ恒常配備される正規のユーザー
環境ツール(`hms.sh`、`git-shelve`/`git-unshelve`、`git-prune-branches`、
`git-audit-worktrees`/`git-prune-worktrees`、`git-worktree-create-guard`、
`sops-secrets-env.sh`)と、真の escape-hatch/diagnostic スクリプト
(`install-packages.sh`、`install-falcon-sensor.sh`、`fix-ssh-permissions.sh`、
`setup-sops-secrets.sh`、`fcitx5-key-trace.pl`)が同居する。どちらに属するか
は `CLAUDE.md` の Project Structure セクションが個々に明記する。

### 8. モジュール分割の軸

新規 `home/modules/*.nix` を追加するときは「1機能 = 1モジュール、単独で
意味が完結する」ことを第一原則とする。ツール名モジュール(`git.nix`、
`gpg.nix`、`claude.nix`、`herdr.nix`)は「ツール = 機能が一致する縮退形」
として許容する。

既存の分割(worktree 関連スクリプトの Claude 向け登録が `claude.nix`、
Codex 向け登録が `worktree.nix` にあること、git 系 `~/.local/bin` 配備が
`packages.nix`/`worktree.nix` に分かれていること)は、#98 の敵対的レビューで
「各モジュールが自サブシステムの範囲で完結した意図的な分割」と確認済みの
ため、本 ADR は遡及的な統合を求めない。

## Alternatives considered

### hook 語彙の全面リネーム(棄却)

外部調査で "gate"/"guard" に業界の支配的慣行がないと判明した時点で、リネーム
によって得られる外部整合性はゼロになった。`docs/claude/*.md` 6件以上と
`config/herdr/config.toml` のハードコード実行パスへの波及に見合わないため
見送った。

### `scripts/` の `bin/`/`scripts/` 物理分割(棄却)

hlissner/dotfiles の `bin/` のような正規レイヤとしての物理分離も検討したが、
既存の home-manager 配備定義(4モジュールに分散)の書き換えコストに見合う
利得が薄いため、今回は規定文言の修正(#105)のみに留めた。物理分割が必要に
なった場合はあらためて ADR を起こす。

## Consequences

- 新規 hook・スクリプト・環境変数・モジュールは本 ADR の規約に従う。
- 既存ファイルは対象外。命名の逸脱を見つけても、改修のついでに直す程度に
  留め、この ADR を根拠に一括リネーム PR を起こさない。
- `registerHooks` の自作 jq マージ(公式 `hooks.json` スキーマとの重複)、
  1,000行超 hook の分割方針は本 ADR のスコープ外。それぞれ #100 / #102 で
  追跡する。

## Verification

ドキュメントのみの変更のため実行時検証はなし。`CLAUDE.md` の Architecture
Decision Records 一覧にこの ADR への参照を追加したことを確認する。
