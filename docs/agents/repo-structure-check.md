# repo-structure-check — リポジトリ構造の不変条件を、`cargo test` で検査する

実装: `crates/repo-structure-check`(Rust のライブラリ。bin は持たない)
走る場所: CI の `Rust workspace` ジョブの `cargo test --workspace`。手元は `cargo test -p repo-structure-check`
Issue: #1434(manifest)、#1435(hook の入出力)、#1436(Stop の block と再入)、#1437(逃げ道)(親の tracking Issue は #1433)

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
- crates/repo-structure-check/tests/tree.rs
- Cargo.toml
