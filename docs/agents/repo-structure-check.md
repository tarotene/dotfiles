# repo-structure-check — リポジトリ構造の不変条件を、`cargo test` で検査する

実装: `crates/repo-structure-check`(Rust のライブラリ。bin は持たない)
走る場所: CI の `Rust workspace` ジョブの `cargo test --workspace`。手元は `cargo test -p repo-structure-check`
Issue: #1434(manifest)、#1435(hook の入出力)、#1436(Stop の block と再入)、#1437(逃げ道)、#1440(ホスト名)(親の tracking Issue は #1433)

## 動機

このリポジトリの規約は、文書に書いたまま検査が無いものが多い。外部依存の版は、97 の crate が各自の manifest に
書いていて、同じ依存の版と feature が crate ごとに分かれていた(`tempfile` は `^3.27.0` が 77、`^3` が 2、
`serde` は `^1.0.229` が 17、`^1` が 4、`toml` は 7 crate が `1.1.6`、4 crate が `1.1.8`)。依存を 1 つ上げるたびに、
全 crate の manifest を探して揃える必要があり、揃え忘れても何も落ちなかった。

構造の不変条件を `cargo test` で検査する型は、すでに 2 つある(`crates/record-layer/tests/dotfiles_declaration.rs`、
`crates/pipeline-map/tests/coverage.rs`)。検査が 3 つ以上になるので、責務のちがう既存の crate に足さず、1 つの
crate に集める。

## 検査

### manifest(`manifest::check_tree`)

| 検査 | 内容 |
|---|---|
| 継承 | メンバーの `[dependencies]`・`[dev-dependencies]`・`[build-dependencies]`(`[target.<cfg>.…]` も)の外部依存は、`name.workspace = true` で継承する。`path` の依存(workspace 内の crate)は対象外 |
| root の表 | root の `[workspace.dependencies]` の各項目は `version` と `default-features` だけを持つ(`path` は許す)。feature はメンバーが足す。どのメンバーも使っていない項目は残さない |
| 依存ゼロの例外 | `pr-title-check` と `pr-contract-check` は、`[dependencies]` と `[build-dependencies]` を空に保つ。`[dev-dependencies]` の明示の指定は許す |

依存ゼロの 2 つは、他リポジトリの CI(`.github/actions/pr-title/action.yml`)が、root の `Cargo.toml` とこの 2 crate
だけを sparse-checkout して `cargo build` する。依存を足すと、他リポジトリの build が重く、壊れやすくなる。
`戻せない: 外部契約` に当たるので、例外は名前で列挙する(`manifest::ZERO_DEPENDENCY_CRATES`)。

### hook の入出力(`hook_io::check_tree`)

`crates/*/src/**/*.rs` のうち、コメントの行と、ファイル末尾のテストの節(`#[cfg(test)]` から後)を除いた行を読む。

| 検査 | 内容 |
|---|---|
| 返答の封筒 | `hookSpecificOutput` の文字列は `agent-session`(`Reply::render`)だけが書く。ほかの crate の `src/` に書いたら落ちる |
| 標準入力 | hook の bin(`src/main.rs`・`src/entry.rs`・`src/bin/*.rs`)は、`std::io::stdin()` を直接呼ばず、`agent_session::entry::read_stdin` を呼ぶ。端末かどうかを調べるだけの行(`is_terminal`)は読みではない |
| Stop の block と再入 | Stop の block(`"decision"` と `"block"` が同じ行)を手で書く行と、`stop_hook_active` を読む行は、`agent-session`(`Reply::render` と `StopInput`)だけが持つ。例外は無い(ADR-1472) |
| 例外の鮮度 | 例外のファイルが、違反の形をもう持たない(または、ファイルが無い)と、例外のほうが落ちる |

例外は、どれも**恒久**の例外で、`hook_io::ENVELOPE_EXCEPTIONS`(返答を書かず、外から来た JSON を読む・読み替える 3 ファイル)と
`hook_io::STDIN_EXCEPTIONS`(hook でない CLI の 4 つと、遅延して読む `adr-number`)に、理由つきで列挙する。違反を先送りする
一覧には使わない。

### 逃げ道(`escape::check_tree`)

名前・スコープ・真偽・案内は `agent_session::escape` の表が唯一の正本である(ADR-1040 の 2026-11-09 の Amendment)。

| 検査 | 内容 |
|---|---|
| 表の外の綴り | guard の `src/`(コメントとテストの節を除く)に、逃げ道の名前の形(`SKIP_` で始まる、または `_ALLOW`・`_BYPASS`・`_SKIP` で終わる全大文字の文字列)を直書きしない。拒否文の `NAME=1` の直書きも同じ。名前と案内は `Hatch` から導く |
| skip ファイル | `join("skip")` を直書きしない。ディレクトリだけを `Hatch::active` に渡す |
| 退役した名前 | 改名した旧名 8 個と、廃止した旧フラグが、凍結する ADR(`docs/adr/`)とこの検査のほかに、ツリーのどこにも残っていない(テストとコメントも読む)。旧名は別名として読まない |
| 表と文書の対(`tests/tree.rs`) | `escape::ALL` の各項目の `doc` に、自分の名前(セッション単位を持つなら `SKIP_<STEM>`、呼び出し単位を持つなら `<STEM>_ALLOW`)が書かれている |

例外は無い。退役した名前の一覧は `escape::RETIRED`(後継の名前つき)。

### ホスト名(`host_names::check_tree`)

`home/**/*.nix` を Nix の字句解析(コメントを読まず、文字列の中身と `${…}` の中を区別する)で読む。ホスト名の正本は
`home/hosts/*.nix` のファイル名(`vega`・`arcturus`・`altair`)。module は機体の名前を、`home/modules/host.nix` が宣言する
option `dotfiles.host.name` で受け取る。

| 検査 | 内容 |
|---|---|
| 文字列のホスト名 | Nix の**文字列**に、ほかの機体のホスト名が単語として現れたら落ちる(自分の機体のファイル、`home/hosts/<host>.nix` と `home/hosts/<host>/**`、の中は自分の名前を持ってよい)。コメントは読まない。説明は `<host>` のように書く |
| 自分の名前の宣言 | `home/hosts/<host>.nix` は `dotfiles.host.name = "<host>";` を持つ。ファイル名と宣言がずれると、`~/.config/dotfiles/host`(`hms` が機体を決める元)が別の機体を指す |
| マーカーの書き手 | `~/.config/dotfiles/host` を書く(`xdg.configFile."dotfiles/host"`)のは `home/modules/host.nix` だけ。`xdg.configFile.<名前>.text` は文字列を連結する型なので、書き手が 2 つあると評価は通り、マーカーが `vega\n\nvega\n` のように壊れる(`lines` の合成を評価して確かめた) |

`dotfiles.host.name` は既定値を持たない。名前を宣言しない機体は、評価が「option を使っているが定義が無い」で落ち、別の機体を
指すマーカーを配備しない。綴りは `[a-z][a-z0-9-]*`(型で縛る)。例外は無い。

### 環境変数・ホーム・XDG(`process_env::check_tree`)

`crates/*/src/**/*.rs` のうち、コメントの行と、ファイル末尾のテストの節(`#[cfg(test)]` から後)を除いた行を読む。環境変数が
空のときを未設定として扱う規則(bash の `${VAR:-}`)と、`${XDG_*_HOME:-$HOME/<既定>}` の形が、約 20 の crate に手で
複写され、複写ごとに空の扱い(`XDG_STATE_HOME=""` を未設定とするか)と `HOME` が無いときの扱い(空のパス・エラー・panic)が
違っていた。解決は `crates/process-env`(依存ゼロ)が持つ。

| 検査 | 内容 |
|---|---|
| XDG の変数名 | `XDG_CONFIG_HOME`・`XDG_STATE_HOME`・`XDG_CACHE_HOME`・`XDG_DATA_HOME` の名前の文字列は、`process-env` だけが持つ。ほかの crate の `src/` は、`process_env::Xdg::<種類>.resolve()`・`resolve_lossy()`・`resolve_under(home)` で基準を解決する |
| 補助関数の複写 | `env_nonempty`・`non_empty_env`・`nonempty_env`・`xdg_dir`・`xdg_state_home`・`config_home`・`env_path`・`home()` という名前の関数(と、前の 3 つのクロージャ)を、`process-env` の外に定義しない。非空の環境変数は `process_env::var_nonempty`、`HOME` は `process_env::home` |

例外は、どれも**恒久**で、理由つきで `process_env.rs` の定数に列挙する。`XDG_EXCEPTIONS`: `local-state-backup` は XDG の変数名を
子プロセスへ引き渡す名前の一覧として持つ(解決ではない)。`HELPER_EXCEPTIONS`: `backup-runtime` の `nonempty_env` は、値を返さず、
空なら失敗にする検証で、非空の値を読む仕事ではない。例外のファイルが違反の形を失ったら、例外のほうが落ちる。

`HOME` の 1 行読み(`std::env::var_os("HOME")`)は、論理を持たない定型なので対象にしない。基準ディレクトリの**解決**と、非空の判定の
**複写**が対象。

#### 寄せたときに挙動が揃った点

複写ごとに違っていた 3 点を、`process-env` の規則(bash の `${VAR:-}` と同じ)に揃えた。いずれも、通常の環境(`HOME` が
設定されていて、XDG の変数は未設定か非空)では変わらない。

- `XDG_STATE_HOME=""` や `XDG_CONFIG_HOME=""`(空)を、**未設定として扱う**。`due-remind`・`feedback-trial`(設定ファイルの場所)・
  `model-equivalence`(codex-plan の設定の場所)・`local-state-backup`・`promotion-detect`(状態の場所)は、これまで空を設定済みとして、
  相対パスを作っていた。
- `HOME` が無いときの結果は、`resolve_lossy` では空の `HOME` として相対パス(`.local/state` など)に揃った。これまでは
  `/.local/state`(絶対)と `.local/state`(相対)が混ざっていた。`HOME` を要る呼び出し側(`git-prune`・`git-audit-worktrees`・
  `due-remind`・`codex-plan`)は、これまでどおり失敗にする。
- `HOME=""`(空)は、`verdict-escalate`・`esa-mirror-context`・`daily-desk-reminder` で、これまで「無い」(`None`)としていたが、
  相対パスを作るようになった(シェルの `${HOME}/…` と同じ)。

### 暦の変換(`civil_time::check_tree`)

`crates/*/src/**/*.rs` を、`process_env` と同じく(コメントの行と、末尾のテストの節を除いて)読む。Howard Hinnant の
`days_from_civil` / `civil_from_days` が、8 つの crate(`agent-session`・`issue-inbox`・`worklog`・`detect-drift`・`github-audit`・
`daily-desk-reminder`・`model-catalog`・`agent-turn-log`)に手で複写され、さらに 3 つの crate(`agent-session` の transcript、
`due-remind`、`verdict-escalate`)が `days_from_civil` の本体を持っていた。月日の型も `i64` と `u32` が混ざっていた。変換は
`crates/civil-time`(依存ゼロ)が持つ。

| 検査 | 内容 |
|---|---|
| 暦の定数 | `719468` と `146097`(数のかたまり単位で比べる。`1_460_970` のような別の数には当たらない)を、`civil-time` の外の `crates/*/src` に書かない。`tests/` の独立な実装(逆変換を手で書いて突き合わせる)は対象にしない |

例外は無い。

### 規則の置き方

- root の表に `default-features = false` を書いた依存(`jiff`、`toml`、`toml_edit`)は、メンバーが `features` で
  必要なものを足す。メンバーは `default-features` を書かない(cargo が無視して警告する)。
- 既定の feature が要るメンバーは、`features = ["default"]` で足す(`toml` の 4 crate)。
- 依存を足すときは、root の表に版を書き、メンバーは `name.workspace = true` にする。Renovate は root の表だけを更新する。

## 検査を足すとき

検査は純粋関数(文字列 → 違反の一覧)で書き、`manifest.rs` の単体テストに、違反を作る fixture(負例)を置く。実際の
ツリーへの適用は `tests/tree.rs` が行う。負例が落ちることを見ずに「違反なし」で通る検査は、検査していない
(`tests/tree.rs` の番兵は、空のツリーを読んで通ってしまうのを防ぐ)。

## 既存手段と代替

- 既存手段: 自前。却下した候補は、manifest を書き換える `cargo-autoinherit`(README に、継承を検査して失敗させる
  モードの記載を見つけられなかった。最終コミットの日付は未確認)と、`cargo-deny` の `bans`(版の重複と wildcard は
  見るが、メンバーに `workspace = true` を要求する項目の記載はない)。出典は、それぞれの README と設定の文書(取得
  2026-11-09)。
- 対抗馬: `crates/pipeline-map/tests/coverage.rs` に足す案(責務が自動発火機構の地図で、manifest や文書の検査とは別)、
  nix の flake check で `cargo metadata` を `jq` で読む案。
- 軸: 検出のみ。manifest の継承は cargo が強制しないので、継承漏れを表現不可能にはできない。root の表に版を 1 回
  書くだけで済む形にして、書き忘れる場所を 1 つに減らした。

## 執行点

- crates/repo-structure-check/src/manifest.rs
- crates/repo-structure-check/src/hook_io.rs
- crates/repo-structure-check/src/escape.rs
- crates/repo-structure-check/src/host_names.rs
- crates/repo-structure-check/src/process_env.rs
- crates/repo-structure-check/src/civil_time.rs
- crates/civil-time/src/lib.rs
- crates/process-env/src/lib.rs
- crates/repo-structure-check/tests/tree.rs
- home/modules/host.nix
- Cargo.toml
