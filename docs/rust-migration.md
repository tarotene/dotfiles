# bash → Rust 移植の手順(ADR-0024、#389)

hook/CLI を 1 本移植するときの手順と、それを支える workspace の構成をまとめる。
移植の順序と対象は `rust-migration.toml`(allowlist)の `stage` と、
#389 配下の Stage 4 Issue が決める。

## workspace の構成

| パス | 役割 |
|---|---|
| `Cargo.toml` | workspace ルート(members 分割、#391) |
| `crates/hook-io` | 共通入出力。stdin JSON / `permissionDecision` の emit / `default_branch` / plan テキスト解決 / session 台帳など(#391 のクラスタ A〜G) |
| `crates/migration-audit` | `rust-migration.toml` を実体と突き合わせる(未分類ファイル・inline シェル・残件上限の検査) |
| `crates/fixture-oracle` | まだ bash の hook/CLI に向けた characterization fixture |
| `flake.nix` の `rustWorkspace` | crane でのビルド。`pkgs.dotfiles-tools` として overlay され、clippy / rustfmt は `checks` に入る |

- `nix develop` で cargo / clippy / rustfmt / rust-analyzer / jq / hyperfine が揃う。
- CI は nix.yml の `rust` ジョブが担う。crane の checks をビルドし、
  `nix develop --command cargo test --workspace` を実行する。

## 1 本移植する手順

先行例に倣い、4 段で行う。先行例は toml-test の言語非依存コーパス、
uutils/coreutils の GNU テスト乗っ取り、Meszaros の Data-Driven Test、
Feathers の characterization test(出典は #391)。

1. **fixture を抽出する。**
   - 対象 bash の振る舞いを `crates/fixture-oracle/tests/cases/<bin>/*.toml`
     (trycmd 形式)に書く。
   - 既存 `--selftest` の各ケースを stdin JSON に言い換えて書く。
   - 期待値は `TRYCMD=overwrite cargo test -p fixture-oracle` で bash 版から
     生成し、差分を目で確認する。
2. **bash に対して緑にする。**
   - `crates/fixture-oracle/src/lib.rs` の `BASH_ORACLES` に
     `(<bin>, <リポジトリ相対パス>)` を足す。
   - `cargo test -p fixture-oracle` が緑になることを確認する。
3. **Rust に対して緑にする。**
   - `crates/<bin>` を作る。依存は `cargo add` で 1 つずつ、features は最小にする。
   - fixture ディレクトリを `crates/<bin>/tests/cmd/` へ `git mv` する。
   - `tests/cli.rs` から `trycmd::cargo::cargo_bin!("<bin>")` に向ける。
   - ケースを手で書き写さない。同じファイルを両方の実装に向けることで、
     転記ミスというバグ源を持ち込まない。
4. **bash を削除する。**
   - bash 実装と、その `--selftest` の CI 配線を削除する。
   - `BASH_ORACLES` の行を削除する。
   - `rust-migration.toml` の `[[target]]` 行を削除し、`max_remaining` を
     同じ数だけ下げる。
   - home module の hook コマンドを `"${pkgs.dotfiles-tools}/bin/<bin>"` に替える。

## 検討して外した方法: Parallel Run

GitHub Scientist 型の Parallel Run は、bash と Rust を両方実行して差分をログに残す。
これは採らない。状態を書く hook(wrap-up inbox writer、plan-file mutator)では
副作用が二重になるためで、Scientist 自身も "should not have side effects"
と明記している(#391)。

## 実測

再ビルド秒数と起動時間は `rust-workspace-measurements.md` を参照。
