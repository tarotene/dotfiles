# Rust workspace の実測(#391 の完了条件)

調査記録(ADR-0008 (1))。数値はツール版・マシンに依存して古くなるため、
ADR-0024 本体ではなくここに置く。

- 取得日: 2026-09-23
- 環境: x86_64-linux のクラウドコンテナ(Claude Code on the web)、
  rustc 1.94.1、nix 2.28.4、crane `73b9805`、hyperfine(nixos-26.05)
- 対象: `Cargo.toml` の workspace(`crates/hook-io` ほか)。
  `[profile.release]` は `lto = true` / `strip = true`

## 1. 1 メンバーの 1 行変更時の再ビルド秒数

### nix(crane)

`crates/hook-io/src/git.rs` の末尾に 1 行足してから `nix build .#dotfiles-tools`
を実行し、3 回計測した:

| 回 | 秒 |
|---|---:|
| 1 | 3.29 |
| 2 | 2.91 |
| 3 | 3.57 |

再ビルドされたのは `dotfiles-tools-0.1.0.drv` だけで、`dotfiles-tools-deps`
(`buildDepsOnly`、serde / serde_json / toml ほか)は再ビルドされなかった。
つまり、依存グラフのキャッシュは設計どおりメンバーの変更から切り離されている。

**限界**: この時点の workspace は lib メンバーだけで、bin を持たない。
bin を持つ構成(Stage 3 以降)の値は、下の cargo の計測で代替している。
Stage 3 の PR で bin 入りの値を取り直す。

### cargo(release、bin 2 本 + 共有 lib の模擬構成)

hyperfine(`--warmup 1 --runs 8`)。`--prepare` で対象ファイルに 1 行追記し、
`cargo build --release` の時間を測った:

| 構成 | 変更箇所 | 平均 [s] |
|---|---|---:|
| workspace members | bin メンバー(hook-a) | 6.884 ± 0.135 |
| 単一クレート `src/bin/` | `src/bin/hook-a.rs` | 6.581 ± 0.112 |
| workspace members | 共有 lib(hook-io) | 7.371 ± 0.172 |
| 単一クレート `src/bin/` | `src/git.rs` | 7.289 ± 0.139 |

## 2. `src/bin/` 単一クレート構成との差

上の表のとおり、差は 1〜5%(誤差の 2 倍程度)にとどまる。どちらの構成でも
時間の大半は `lto = true` のリンク段が占めており、レイアウトは支配項ではない。

members 分割を採る根拠は再ビルド時間ではない。次の 2 点による:

- 依存の閉包をメンバーごとに最小化できる(`cargo add` を個別に行う方針)
- `hook-io` の公開 API を境界として固定できる

## 3. hook 起動時間(ADR-0024 制約 2: 50ms 予算)

hyperfine(`-N`、`--input` で stdin JSON を与える)の結果:

| 対象 | 平均 [ms] | 最小 [ms] | 最大 [ms] |
|---|---:|---:|---:|
| Rust(`hook-io` を使う模擬 allow hook、release) | 1.4 ± 0.5 | 1.1 | 8.3 |
| 参考: `git-worktree-allow.sh`(bash、素通し経路) | 8.9 ± 0.7 | 8.0 | 11.3 |

Rust の最大値 8.3ms でも予算の 1/6 に収まる。ADR-0024 の PoC 値(1.2ms)とも
整合する。

## 判断

どの数値も許容範囲内だった。

- 再ビルドは nix で約 3 秒、cargo release で約 7 秒。
- 起動は 50ms 予算を大きく下回った。

したがって #205(herdr ローカルビルドキャッシュの Cachix 昇格)は再オープンしない。

未計測なのは darwin(altair、ADR-0018)。darwin ホストで同じ手順を
再実行するまで、この結論は x86_64-linux に限る。
