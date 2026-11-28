# ci-consumers — 編集するパスを入力とする CI の検査を編集前に確認する

## なぜ必要か

PR #872 で `close-linked-issues.yml` に `pipeline-node` のコメントを 1 行追加したところ、その配布元テンプレートとのバイト一致を要求する CI の step(`ci.yml` の `close-linked-issues template is in sync`、`Escape-hatch dry-run` job)が失敗した。ファイルを一括編集する前に、**そのファイルを入力とする CI の検査を調べていなかった**ためである。失敗した後に修正すると、stacked PR では全段の rebase と force-push のやり直しになる。

## 何をするか

```
scripts/ci-consumers <編集するパス>...
```

引数のパスを参照している箇所を CI に関わるファイルから検索し、`file:line` と直近の step 名付きで列挙する。

| 探す場所 | 拾うもの |
|---|---|
| `.github/workflows/*.yml` | workflow の step(`run:` の中のパスも対象) |
| `.github/actions/*/action.yml` | composite action(`BUILD_INPUTS` などの宣言も対象) |
| `scripts/*` | workflow が呼ぶスクリプトとその selftest |
| `crates/*/tests/**/*.rs\|sh\|toml` | `cargo test` が読み込むファイル(fixture の JSON は除く) |

一致判定はパス文字列そのもの(先頭の `./` は除く)の固定文字列検索で行う。ディレクトリ指定時は末尾の `/` を補い、配下を指す参照も拾う。引数自身のファイルとこのスクリプト自身は除外する。参照元が 1 つもなければ終了コード 1 を返す。

```console
$ scripts/ci-consumers .github/workflows/close-linked-issues.yml
== .github/workflows/close-linked-issues.yml
.github/workflows/ci.yml:248  [step: close-linked-issues template is in sync]  <(grep -v '^# pipeline-node:' config/agents/skills/...
.github/workflows/ci.yml:249  [step: close-linked-issues template is in sync]  <(grep -v '^# pipeline-node:' .github/workflows/close-linked-issues.yml)
```

出力された step は手元で同じ入力を与えて実行し、結果を PR 本文の「検証」に記載する(`pr-description` スキル §6a、`stacked-pr` スキル §3 の push 前の確認)。

## 限界

- 固定文字列検索のため、パスを変数・連結・glob で組み立てて参照する箇所は拾えない。見つからないことと存在しないことは別である。該当 0 件でも、`.github/workflows/ci.yml` の `paths` フィルタや glob でディレクトリごと読み込む検査がないかは手で確かめる。
- 拾うのは「そのパスが書かれている箇所」であり、「その編集で検査が失敗するか」までは判定しない。失敗するかどうかは該当 step を実際に実行して確かめる。
- CI に影響しない場所(`docs/`、`config/` の中身など)は探さない。

## 既存手段と代替

- 既存手段: 自前 — 探索: `scripts/`・`.github/workflows/ci.yml`・`crates/` を `consumers`・`BUILD_INPUTS` で grep した。パスの消費者を検索するツールはなく、類例は `ci.yml` の「生成物の同期」検査(render して `git status` の差分を見る)と `.github/actions/stack-position/action.yml` の `BUILD_INPUTS` 宣言だけだったが、いずれも事前検索用ではなかった。
- 対抗馬: Rust の小さな crate(ADR-0024 の既定では新規 hook と CLI は Rust)。このツールは `scripts/` に置きつつ `~/.local/bin/ci-consumers` にも配布するため(下の「どのリポジトリでも呼べる」)、配布先で見れば ADR-0024 対象の「`~/.local/bin` の CLI」にも当たる。bash に残す理由: 判定は固定文字列の grep と直前の `- name:` 行の読み取りだけで、型で表現不可能にすべき不正な状態がない(表現不可能性の軸で差が出ない)。
- 棄却した代替: 手順への追記だけ(本人が選ばなかった)、pre-push hook で同期検査を走らせる案(1 つの検査の複製にしかならず他の step に効かない)、生成化(GitHub Actions がコミット済みのコピーを要求するためバイト一致そのものを表現不可能にはできない)。
- 軸: 検出のみ。
- かつての `rust-migration.toml` の監査(`migration-audit`)は、`scripts/` の新規ファイルを Rust 移植対象か理由付き `[[excluded]]` かのどちらかに分類させていた(初版ではこの分類を忘れて CI の `Rust workspace` が失敗した)。移植完了に伴い 2026-10-24 に撤去した(ADR-0024 Amendment 5)。新規ツールの既定が Rust である点は `docs/rust-migration.md` の「新しい道具の既定」を参照。

## どのリポジトリでも呼べる

`home/modules/packages.nix` がこのスクリプトを `~/.local/bin/ci-consumers` に配置する(`adr-number-check` と同じ配置)。スクリプトは cwd の git ルート(`git rev-parse --show-toplevel`)を見るため、`scripts/ci-consumers` を持たないリポジトリのルートでも `ci-consumers <path>...` として呼べる。そのリポジトリの `.github/`・`scripts/`・`crates/*/tests/` のうち、存在するものだけを探す。

配置前は `pr-description` スキル §6a が指す `scripts/ci-consumers` がないリポジトリで終了コード 127 になり、参照元の確認を手作業で代替していた(別リポジトリで 4 回記録された)。PATH にない機体(`hms` 適用前)向けの代替は、同スキル §6a の `git grep -n -F -- '<path>' -- .github/` である。

## 登録

- `scripts/ci-consumers --selftest` は `.github/workflows/ci.yml`(`Escape-hatch dry-run` job)が実行する。ネットワーク不使用で、一時ディレクトリに作った fixture を使い、step 名付きの列挙、参照元なし(終了コード 1)、自己参照の除外、先頭 `./` の除去、ディレクトリ接頭辞、fixture JSON の除外を確かめる。
