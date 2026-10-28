# ADR-1228 — システム層の宣言と適用を分ける

- Status: Accepted
- Date: 2026-10-28
- Issue: #274
- Amends: ADR-0001、ADR-0018 Decision 1

## Context

home-manager の管理対象外であるシステム層にも、Linux の scheduler 設定と macOS の Homebrew アプリという宣言できる設定がある。従来は apt と Brewfile の手順から適用していた。対象を限定して宣言へ集めつつ、OS 固有のドライバーや ABI を持つパッケージは各 OS の管理に残す。

## Decision

Linux の vega・arcturus は system-manager の構成を公開する。既存の scheduler KDL を正本とし、既存ファイルがある場合は system-manager の退避・復元機能を使う。userborn、SUID wrapper、Nix 管理と `/run/current-system` の互換リンクは有効化しない。両機体の Pop!_OS 構成だけで未検証 distro の許可を明示する。

macOS の altair は nix-darwin を使い、既存 Brewfile をそのまま Homebrew の追加設定として読み込む。cleanup と自動更新・upgrade は行わない。これにより ADR-0018 Decision 1 の nix-darwin 不採用を変更するが、Homebrew の対象と standalone home-manager の責務は維持する。

`hms` はユーザー環境の適用のままとし、root の system-manager / nix-darwin 適用を混ぜない。この PR では構成の評価・build を行い、機体への適用は行わない。apt と既存 installer は、実機への移管まで動く形で残す。

## 先行例との対比

- Linux: Numtide, *system-manager*, commit `8c7a4b84eabbae3236a423ca6f16c142ef3a91b9`, [configuration and defaults](https://github.com/numtide/system-manager/blob/8c7a4b84eabbae3236a423ca6f16c142ef3a91b9/nix/modules/default.nix)・[existing file backup](https://github.com/numtide/system-manager/blob/8c7a4b84eabbae3236a423ca6f16c142ef3a91b9/nix/modules/etc.nix#L122-L132)、公開日未確認、取得 2026-10-28。差分: 既定で有効な userborn 等を無効にし、既存 OS の所有物を管理しない。
- macOS: nix-darwin contributors, *nix-darwin*, commit `d31953cf55f9027a56e9be938daf5db8c5655663`, [Homebrew module](https://github.com/nix-darwin/nix-darwin/blob/d31953cf55f9027a56e9be938daf5db8c5655663/modules/homebrew.nix#L921-L964)、公開日未確認、取得 2026-10-28。差分: `extraConfig` に既存の literal Brewfile を渡す。
- 既存手段: `scripts/install-packages.sh` と `scripts/install-packages-darwin.sh` — 拡張: OS とアプリの取得は既存手段を残し、対象設定を system-manager / nix-darwin の宣言へ集める。軸: 表現不可能 — 設定の配備元を単一のファイルに寄せる。

## 執行点

- `flake.nix`
- `system/linux.nix`
- `system/darwin.nix`
