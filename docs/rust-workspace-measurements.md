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

#### bin 入りの再計測(Stage 3、#392)

bin メンバーが 2 本(`gh-edit-allow` / `update-own-tools`)になった時点で、
`crates/gh-edit-allow/src/main.rs` に 1 行足して取り直した:

| 回 | 秒 |
|---|---:|
| 1 | 12.68 |
| 2 | 12.04 |
| 3 | 12.25 |

deps drv は今回も再ビルドされなかった。ただし `dotfiles-tools` は workspace 全体を
1 つの derivation でビルドする。そのため、どのメンバーを変更しても全 bin の
コンパイルと LTO リンクが走る。bin が増えるとこの値はほぼ線形に伸びる見込み。
許容できなくなった時点で、メンバーごとに `buildPackage`(`cargoExtraArgs = "-p <bin>"`)
へ分ける。分けても deps drv は共有されたままなので、その変更は flake.nix の中だけで閉じる。

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
| `gh-edit-allow`(実物、allow 経路、`git config` 子プロセス込み、Stage 3) | 3.1 ± 0.3 | 2.5 | 5.5 |

Rust の最大値 8.3ms でも予算の 1/6 に収まる。ADR-0024 の PoC 値(1.2ms)とも
整合する。

## 4. statusline / claude-usage の起動時間(Stage 4d、#413)

statusline はストリーミング中 ~300ms 毎、claude-usage は herdr のタブバーから
60 秒毎に起動される。bash 版と Rust 版を同じ入力で比べた。

- 取得日: 2026-10-01
- 環境: x86_64-linux(Pop!_OS、Intel Core Ultra 7 268V、8 スレッド)、
  rustc 1.95.0、Determinate Nix 3.21.5(nix 2.34.8)、hyperfine 1.20.0、
  jq 1.8.2(bash 版が使う)
- 手順: `cargo build --release` の bin と bash 版を
  `hyperfine -N --warmup 5 --runs 200` で比べた。statusline は `--input` で
  statusline JSON(model / ctx / cost / effort / project_dir あり)を与え、
  bash 版は配備と同じく `bash <path>` で、claude-usage は shebang
  (`/bin/sh` = dash)で起動した

| 対象 | 経路 | 平均 [ms] | 最小 [ms] | 最大 [ms] |
|---|---|---:|---:|---:|
| `claude-statusline.sh`(bash) | Herdr 外、リポ名キャッシュ温 | 14.9 ± 1.9 | 10.4 | 22.1 |
| `claude-statusline`(Rust) | Herdr 外、リポ名キャッシュ温 | 0.9 ± 0.3 | 0.5 | 2.0 |
| `claude-statusline.sh`(bash) | Herdr 内、同値で送信抑止 | 17.8 ± 4.1 | 11.8 | 39.7 |
| `claude-statusline`(Rust) | Herdr 内、同値で送信抑止 | 1.0 ± 0.3 | 0.5 | 2.1 |
| `claude-usage.sh`(sh) | `__render`(limits 2 件) | 46.3 ± 6.8 | 33.1 | 96.5 |
| `claude-usage`(Rust) | `__render`(limits 2 件) | 1.9 ± 0.8 | 1.0 | 7.4 |
| `claude-usage.sh`(sh) | 30 秒ガード(state が新しい) | 15.6 ± 2.6 | 10.7 | 26.3 |
| `claude-usage`(Rust) | 30 秒ガード(state が新しい) | 1.1 ± 0.4 | 0.5 | 3.0 |

- 「Herdr 内」の bash 版の数値は本体の終了までで、`( ... ) &` で切り離した
  python3(送信抑止の判定を含む)の時間は入っていない。Rust 版は抑止判定を
  本体で行い、送るときだけ子プロセスを切り離す。
- claude-usage の通常経路は curl による fetch(`--max-time 5`)が支配的で、
  ネットワークに依存するため計測していない。

## 判断

どの数値も許容範囲内だった。

- 再ビルドは nix で約 3 秒(lib のみ)〜約 12 秒(bin 2 本)、cargo release で約 7 秒。
- 起動は 50ms 予算を大きく下回った。

したがって #205(herdr ローカルビルドキャッシュの Cachix 昇格)は再オープンしない。

未計測なのは darwin(altair、ADR-0018)。darwin ホストで同じ手順を
再実行するまで、この結論は x86_64-linux に限る。
