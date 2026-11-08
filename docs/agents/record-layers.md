# record-layers — 記録の層(L0〜L4)の宣言と、Write の拒否・日次のずれ検出

設計の正本は `docs/adr/1358-record-layers.md`(層の定義、宣言の形、拒否の範囲、検出のしきい値)。この文書は、運用の手順をまとめる。
判定は `crates/record-layer`(lib)が 1 か所に持ち、`hook-router` の規則 `record-layer-guard` と `promotion-detect` が同じ関数を呼ぶ。

## 層

| 層 | 効いている期間 | 置き場 |
|---|---|---|
| L0 不変 | 月〜年(追記のみ。ADR、裁定ログ) | Git |
| L1 年〜月 | 月〜年(規約・スキーマ・スクリプト・定義・手順書) | Git |
| L2 週 | 週(計画、週次以下の digest) | Git |
| L3 日〜都度 | 1 週未満(状態、済んだことの記録、観測の行、台帳の行) | Git の外(Obsidian Vault が第一候補) |
| L4 機械生成 | 生成のたび(外部の写し、キャッシュ、生成物、スナップショット) | Git の外(vault の外) |

層は記録の種類に当てる。迷ったら、「その記録が現在の事実として効いているのは 1 週未満か」で決める。

## 宣言ファイル

リポジトリのルートの `.config/record-layers.toml`。

```toml
[[path]]
glob = "docs/adr/**"
layer = "L0"
```

- `glob` はリポジトリのルートからの相対パス(`/` や `./` で始めない、`..` を含めない)。`*` は `/` を越えないので、ディレクトリの下は
  `dir/**` と書く。リポジトリ直下のファイルは `*`。先頭が `.` のファイルにも `*` は当たる。
- `layer` は `L0`〜`L4` のどれか。小文字・未知の層・未知のキーは、宣言を読めない扱いになる。
- 1 パス 1 層。`git ls-files` の各ファイルに当たる glob がちょうど 1 つであること。dotfiles では
  `cargo test -p record-layer`(`tests/dotfiles_declaration.rs`)が検査する: 漏れ(0 個)・重なり(2 個以上)・どのファイルにも当たらない
  glob(消えたパスの宣言の残り)で落ちる。
- 新しいトップレベルのディレクトリや、`docs/` の新しい階層を足す PR は、宣言に 1 行足す。

宣言を置かないリポジトリでは、何も発火しない(opt-in)。会社・org 共有のリポジトリには置かない。

## Write の拒否(`record-layer-guard`)

PreToolUse の `Write` で、宣言のあるリポジトリ(書き込み先の git toplevel に `.config/record-layers.toml` がある)について拒否する:

| 書き込み先 | 判定 |
|---|---|
| どの glob にも当たらない | 拒否。宣言に `[[path]]` を足してから Write し直す |
| L3・L4 の glob に当たる | 拒否。Git の外に置く(L3 は Obsidian Vault、L4 は vault の外) |
| L0〜L2 の glob に当たる | 通す |

素通りにする(fail-open)もの: 宣言が無い・壊れている(読めない・`[[path]]` が空・glob の誤り)、2 つ以上の glob に当たる(宣言の欠陥)、
`.gitignore` の対象(`target/` など)、`.git/` の中、git の外やリポジトリの外のパス。壊れた宣言で拒否すると宣言自身を直す Write まで
止めうるので、壊れた宣言は拒否の根拠にしない。壊れたことは日次のずれ検出が拾う。

拒否されるのは `Write` だけ。Edit と Bash の書き込みは見ない(日次のずれ検出で補う)。skip の仕組みは無い: 層が誤りなら、宣言を直す
(宣言ファイル `.config/record-layers.toml` への Write は L1 として通る)。

拒否されたときの直し方:

1. 作ろうとしている記録の層を決める(上の表)。
2. L0〜L2 なら、`.config/record-layers.toml` に `[[path]]` を足し、他の glob と重ならないことを確かめて(`cargo test -p record-layer`)、Write し直す。
3. L3・L4 なら、Git の外に置く。

`hms` で hook-router を入れ直すまでは、新しい規則は効かない(`hms .` で現在の worktree を当てて確かめる)。

## 日次のずれ検出(`promotion-detect`)

[promotion-detect.md](promotion-detect.md) の 7 番。`PROMOTION_DETECT_GHR_DIR`(既定 `~/.ghr`)の下の `<host>/<owner>/<repo>` で、
宣言を持つチェックアウトだけを読む。

- **宣言の欠陥**: 宣言を読めない、または追跡ファイルに当たる glob が 0 個・2 個以上。
- **実測のずれ**: 直近 7 日に書き換わった日の数が、L0 は 3 日以上、L1 は 5 日以上、L2 は毎日のファイル。L3・L4 は Git に置かない層なので、速さを測らない。

候補は wrap-up inbox に流れ、`/my-cleanup` で本人が裁定する。実測のずれは「宣言か置き場かを直す兆候」であって、確定ではない:
L1 のファイルが一時的に頻繁に書き換わっただけなら、宣言はそのままでよい。記録が日〜都度の速さで入れ替わり続けるなら、L3 として Git の外へ移し、宣言を直す。
候補の本文は、リポジトリを `owner/repo` で名指しするので、公開 Issue にするときは、非公開のリポジトリ名を書き写さない(ADR-0034)。

しきい値は `crates/record-layer/src/drift.rs` の `WINDOW_DAYS`・`drift_threshold`。

## 手動確認

```bash
PROMOTION_DETECT_GHR_DIR=~/.ghr promotion-detect --dry-run
cargo test -p record-layer
```

## 試験

- `crates/record-layer/src/lib.rs`・`hook.rs`・`drift.rs` の単体テスト: 閉語彙、`*` の境界、重なりと漏れ、Write の判定(一時ディレクトリの git リポジトリ、
  linked worktree、`.gitignore`、リポジトリの外のパス、壊れた宣言)、変化の速さの数え方としきい値。
- `crates/record-layer/tests/dotfiles_declaration.rs`: dotfiles 自身の宣言が全ファイルをちょうど 1 つの glob で覆うこと。
- `crates/hook-router/tests/cli.rs`: ルータ経由で、未宣言・L3 の Write が拒否され、宣言の無いリポジトリでは判定されないこと。
- `crates/promotion-detect/tests/cli.rs`: 宣言の欠陥・実測のずれが候補になり、宣言の無いチェックアウトは読まれないこと。

## 範囲外

他のリポジトリへの宣言ファイルの配布、Git に残った L3 の移行、`crates/model-catalog/catalog.toml`(手書きと GENERATED ブロックの混在)の分割、
L4 の生成図(`docs/assets/pipeline-map/`)の置き場の決定は、それぞれ別の作業で扱う(ADR の D5・D7)。
