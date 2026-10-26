# ci-consumers — 編集するパスを入力にしている CI の検査を、編集の前に引く

## なぜ必要か

PR #872 で、`close-linked-issues.yml` に pipeline-node のコメントを 1 行足したところ、その配布元
テンプレートとのバイト一致を要求する CI の step(`ci.yml` の `close-linked-issues template is in sync`、
`Escape-hatch dry-run` job)が落ちた。ファイルを一括編集する前に、**そのファイルを入力にする CI の検査を
探していなかった**。落ちた後に直すと、積み重ねた PR(stacked PR)では、全段の rebase と force-push の
やり直しになる。

## 何をするか

```
scripts/ci-consumers <編集するパス>...
```

引数のパスを読む箇所を、CI に効く場所だけから、`file:line` と直近の step 名つきで列挙する。

| 探す場所 | 拾うもの |
|---|---|
| `.github/workflows/*.yml` | workflow の step(`run:` の中のパスも) |
| `.github/actions/*/action.yml` | composite action(`BUILD_INPUTS` などの宣言も) |
| `scripts/*` | workflow が呼ぶスクリプトと、その selftest |
| `crates/*/tests/**/*.rs\|sh\|toml` | `cargo test` が読むファイル(fixture の JSON は除く) |

一致は、パスの文字列そのもの(先頭の `./` は落とす)の固定文字列検索。ディレクトリは末尾の `/` を補い、
配下を指す参照も拾う。引数のパス自身のファイルと、この script 自身は除く。消費者が 1 つも無ければ終了コード 1。

```console
$ scripts/ci-consumers .github/workflows/close-linked-issues.yml
== .github/workflows/close-linked-issues.yml
.github/workflows/ci.yml:248  [step: close-linked-issues template is in sync]  <(grep -v '^# pipeline-node:' config/agents/skills/...
.github/workflows/ci.yml:249  [step: close-linked-issues template is in sync]  <(grep -v '^# pipeline-node:' .github/workflows/close-linked-issues.yml)
```

出た step は、手元で同じ入力で走らせる。結果は PR 本文の「検証」に書く(`pr-description` スキル §6a、
`stacked-pr` スキル §3 の push 前の確認)。

## 限界

- 固定文字列の検索なので、パスを組み立てて(変数・連結・glob)参照する消費者は拾えない。拾えないことと、
  無いことは別。消費者が 0 件でも、`.github/workflows/ci.yml` の `paths` フィルタや、検査が glob で
  ディレクトリごと読む場合は、手で確かめる。
- 拾うのは「そのパスが書かれている」箇所で、「その編集で検査が落ちる」ことまでは判定しない。落ちるかは、
  出た step を走らせて確かめる。
- CI に効かない場所(`docs/`、`config/` の中身など)は探さない。

## 既存手段と代替

- 既存手段: 自前 — 探索: `scripts/`・`.github/workflows/ci.yml`・`crates/` を `consumers`・`BUILD_INPUTS` で grep した。
  パスの消費者を引く道具は無く、類例は `ci.yml` の「生成物の同期」の検査(render して `git status` の差分を見る)と、
  `.github/actions/stack-position/action.yml` の `BUILD_INPUTS` の宣言だけで、どちらも消費者を事前に引くものではない。
- 対抗馬: Rust の小さな crate(ADR-0024 の既定は新規の hook と CLI に Rust)。ただし ADR-0024 の対象は hook と
  `~/.local/bin` の CLI で、リポジトリ内の escape-hatch の `scripts/` は対象外。YAML の step 名は直前の
  `- name:` 行で足りるので、bash で済む。
- 棄却した代替: 手順への追記だけ(本人が選ばなかった)、pre-push hook で同期の検査を走らせる案(1 つの検査の
  複製で、ほかの step には効かない)、生成化(GitHub Actions が committed な複写を要求するので、バイト一致
  そのものを表現不可能にはできない)。
- 軸: 検出のみ。
- かつての `rust-migration.toml` の監査(`migration-audit`)は、`scripts/` の新しいファイルを、Rust への
  移植の対象か `[[excluded]]` に理由つきで載せたものかのどちらかに分類させていた(最初の版は、この分類を
  忘れて CI の `Rust workspace` が落ちた)。移植の完了に伴い、2026-10-24 に撤去した(ADR-0024
  Amendment 5)。新しい道具の既定が Rust であることは、`docs/rust-migration.md` の「新しい道具の既定」を参照。

## 登録

- `scripts/ci-consumers --selftest` は `.github/workflows/ci.yml`(`Escape-hatch dry-run` job)が実行する。
  ネットワーク不使用で、一時のディレクトリに作った fixture で、step 名つきの列挙、消費者なし(終了コード 1)、
  自己参照の除外、先頭の `./`、ディレクトリの接頭辞、fixture の JSON を拾わないこと、を確かめる。
