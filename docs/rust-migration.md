# bash → Rust 移植の記録(ADR-0024、#389)

**移行は完了した(2026-10-24)。** hook・CLI は Rust の workspace(`crates/*`)になり、
残る bash は、escape hatch(`bootstrap.sh`・`scripts/install-*.sh` など、Nix や Rust が
無い段階で動くもの)と、governance テンプレートとして各リポジトリへ複製される
`scripts/adr-number-check` だけ(ADR-0024 Amendment 3)。この文書は、移植の
記録と、新しい道具の既定がどう執行されるかを残す。

## workspace の構成

| パス | 役割 |
|---|---|
| `Cargo.toml` | workspace ルート(members 分割、#391)。`[profile.release]` の LTO は off(`rust-workspace-measurements.md` §5) |
| `crates/hook-io` | 共通入出力。stdin JSON / `permissionDecision` の emit / `default_branch` / plan テキスト解決 / session 台帳 / jq 互換の順序保持 JSON など(#391 のクラスタ A〜M) |
| `crates/guard-core` | `gh` を呼ぶ PreToolUse guard の共有エンジン(#415) |
| `crates/pr-title-check`・`crates/pr-contract-check` | 他リポジトリの required check の実体(`.github/actions/pr-title`)。依存ゼロで、sparse-checkout して `cargo build` する |
| `crates/kiriban` | 他リポジトリの workflow の実体(`.github/actions/kiriban`)。Issue / PR 番号のキリ番の判定と祝福の文面の単一正本(`docs/kiriban.md`)。依存ゼロで、sparse-checkout して `cargo build` する |
| `flake.nix` の `rustWorkspace` | crane でのビルド。`pkgs.dotfiles-tools` として overlay され、clippy / rustfmt / 依存の derivation は `checks` に入る |

- `nix develop` で cargo / clippy / rustfmt / rust-analyzer / jq / hyperfine が揃う。
- CI は `ci.yml` の `rust` ジョブが担う。crane の clippy・rustfmt をビルドし、
  `nix develop --command cargo test --workspace` を実行する。release の package は、
  build-host の activationPackage が作る。

## かつての 4 段の手順(履歴)

hook/CLI を 1 本移植するときは、先行例(toml-test の言語非依存コーパス、uutils/coreutils の
GNU テスト乗っ取り、Meszaros の Data-Driven Test、Feathers の characterization test、出典は #391)に
倣い、4 段で行っていた。

1. 対象 bash の振る舞いを trycmd の fixture に抽出する(期待値は bash 版から生成する)。
2. fixture を bash に向けて緑にする。
3. 同じ fixture を Rust(`crates/<bin>/tests/cmd/`)に向けて緑にする。ケースを手で書き写さない。
   同じファイルを両方の実装に向けることで、転記ミスというバグ源を持ち込まない。
4. bash を削除する。

最後の bash を移したので、この手順を支えていた `crates/fixture-oracle`(bash に向けた
characterization fixture)は空になり、`crates/migration-audit` と `rust-migration.toml`
(移植対象の allowlist と実体の突き合わせ)は、残件 0 のまま、走査対象のディレクトリの大半が
無くなった。2026-10-24 に 3 つとも撤去した(ADR-0024 Amendment 5)。

## 検討して外した方法: Parallel Run

GitHub Scientist 型の Parallel Run は、bash と Rust を両方実行して差分をログに残す。
これは採らなかった。状態を書く hook(wrap-up inbox writer、plan-file mutator)では
副作用が二重になるためで、Scientist 自身も "should not have side effects"
と明記している(#391)。

## 新しい道具の既定

新しい hook・CLI・検査の実装は、既定で Rust(ADR-0024)。対象外は ADR-0024 の Context が
列挙するもの: escape-hatch(`bootstrap.sh` 等)、他リポジトリへ配布する skills の
templates/scripts、git hooks、zsh モジュール(ADR-0007 の性質分類、可搬性維持のため)。

撤去した `migration-audit` は、「分類されていない bash を見つけたら CI を落とす」**検出**だった。
撤去したあとは、**言語そのものを機械で強制する検査は無い**(ADR-0024 Amendment 5 で、検出を
1 つ手放すと決めた)。残るのは、新しい道具を作る時点で理由を書かせる仕組みで、どちらも
言語を直接は問わない:

- 計画の段階: 新しい道具・単位の実装言語を選ぶ `Dn` には、対抗馬を立てて白紙で比べた記録
  (`本命:` / `対抗馬:` / `既存手段:`)を書く規約がある(`technology-selection` スキル、ADR-0035、
  ADR-751)。`plan-precedent-gate` が見るのは `## 先行例との対比` 節と `軸:` などの書式で、
  言語選択の `Dn` に重い欄が書かれているかまでは判定しない。内容は `copilot-plan-review` の
  lens A が監査する。
- 書く段階: `new-tool-guard`(ADR-543)が、新しい道具・単位(`bin/`・`scripts/`・`hooks/`・
  `cmd/` 配下の新規ファイル、パッケージマニフェストの新設など)を Write する前に、`既存手段:` の
  記録を求める。これは「既存手段で足りないか」を問うもので、言語は問わない。

bash が生え直したことが観測されたら、検査を戻す(ADR-543 D2: 散文から検査への昇格は、同じ違反が
繰り返されたとき)。

## 実測

再ビルド秒数と起動時間は `rust-workspace-measurements.md` を参照。

現在の共通層は[責務別ライブラリ](agents/agent-libraries.md)へ移行済み。上の旧 crate 名は移植時点の記録。
