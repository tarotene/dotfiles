# ADR-0018 — darwin ホストと macOS システム層の Homebrew 薄層写像

- Status: Accepted
- Date: 2026-09-19
- Issue: No-Issue(grill-me セッション中に発見・裁定)

## Context

2022 M2 MacBook Air(aarch64-darwin)を home-manager 管理下に組み込むにあたり、
Linux 3 ホストの三層モデル(ADR-0001: home-manager が正、apt は root 所有物の
ための薄い escape hatch)を macOS にどう写すかを決める必要があった。

macOS の宣言的設定管理には確立された選択肢が複数ある(nix-darwin、standalone
home-manager + Homebrew の組み合わせ、あるいは Homebrew Bundle 単体)。今回は
1 台のみの新規ホスト追加であり、既存 3 ホストとの対称性(同じ `home-manager
switch` ワークフロー、同じ `hms`/`bootstrap.sh`)を保つことを優先した。

## Decision

1. **nix-darwin は導入しない。** standalone home-manager をそのまま
   aarch64-darwin に拡張する(flake.nix の per-system 化のみ、ADR-0019 と
   同じコミット群)。macOS のシステム層(root 所有・OS 統合が要る設定)は
   nix-darwin ではなく **Homebrew** に委ねる — `packages/declarative/
   apt-packages.txt` と対称の `packages/declarative/Brewfile`、
   `scripts/install-packages.sh` と対称の `scripts/install-packages-darwin.sh`
   (`brew bundle --file=Brewfile`)。ADR-0001 の三層モデルの「システム層は
   薄い escape hatch」という原則をそのまま踏襲する。
2. **初日の Brewfile は Google Chrome cask のみ。** ターミナルは nix の
   `pkgs.alacritty`(nixGL ラッパーなし — macOS は自前の GL スタックを持つ、
   ADR-0006 の対偶)。IME は macOS 標準を使う(fcitx5/mozc は darwin では
   一切配備しない)。`open`/`xdg-open` は macOS ネイティブの `/usr/bin/open`
   をそのまま使う(shadow しない — detach-open.sh が回避する COSMIC 固有の
   フォアグラウンドブロック問題自体が macOS には存在しない)。
3. **フルスタックで積む。** shell/git/gpg/atuin/runtimes/packages/claude/
   herdr/worktree/quarantine の各モジュールは darwin でも有効化する。
   Linux 専用要素(fcitx5、nixGL、systemd --user、COSMIC XDG autostart)は
   各モジュール内で `lib.mkIf pkgs.stdenv.isLinux` / `isDarwin` により分岐
   する(in-module ガード方式 — 詳細は各 PR の説明を参照)。
4. **gpg-agent の寿命境界は darwin では linger の概念に依存しない。**
   ADR-0003 Amendment 2 の「[S] パスフレーズのキャッシュ境界は agent
   プロセスの寿命(≒ ログインセッション)」という設計そのものは変わらないが、
   Linux 側の `assertNoLinger`(systemd --user の linger 設定を検査する)は
   darwin には存在しない概念であり、launchd agent はそもそもログイン
   セッションと寿命を共にする(launchd の per-user agent はユーザーの
   ログアウトで終了する)。よって darwin では `assertNoLinger` 相当の
   アサーションは不要であり、追加しない。

## 先行例

- nix-darwin 不採用: 先行例: 本リポジトリ ADR-0001(三層モデル)—
  取得日 2026-09-19 — 差分: 一致(Homebrew↔apt の対称写像で列を足すのみ)。
  nix-darwin の Linux 対応物は NixOS そのものであり、Pop!_OS ベースの
  現行憲章とは非互換。層モデルの対称性を優先し、nix-darwin の採否は
  将来必要になった時点で改めて ADR を起こす撤退条件付きの判断とする。
- pinentry/gpg-agent darwin 対応: 先行例:
  nix-community/home-manager#2964(gpg-agent darwin 対応 PR)—
  取得日 2026-09-19 — 差分: 一致(macOS では pinentry-mac がデフォルトの
  確立済み経路)。
- launchd.agents による systemd timer 代替: 先行例: home-manager
  `modules/launchd/default.nix` — 取得日 2026-09-19 — 差分: 一致
  (`StartInterval` が `OnUnitActiveSec` 相当)。
- herdr の darwin ビルド可否: 先行例: nixpkgs
  `pkgs/by-name/he/herdr/package.nix`(`meta.platforms = lib.platforms.unix`、
  darwin 時に cctools/xcbuild を追加)— 取得日 2026-09-19 — 差分: 異なる
  (patches/herdr-worktree-names.patch がキャッシュ済み派生を無効化するため
  darwin でもローカルビルドが要る点は Linux と同じだが、実機/CI での
  ビルド可否そのものは未検証 — 本 PR の CI 変更で macos-latest 実行を
  初めて走らせる)。

## Alternatives considered

- **nix-darwin を導入する** — システム設定(defaults, launchd, Homebrew
  宣言管理)まで nix の管理下に置ける。しかし新しいレイヤー導入は
  1 台のみの追加という今回のスコープに対して過大で、Linux 側との
  非対称(NixOS 対応物が無い)を増やす。将来 darwin ホストが増え、
  システム設定の宣言化ニーズが高まった時点で再検討する。
- **Homebrew も避けて完全に nix だけで完結させる** — Google Chrome や
  今後増えうる GUI アプリの macOS ネイティブ配布(署名済み .app、
  自動更新)は nixpkgs 経由よりも Homebrew cask の方が実務上安定している
  ことが広く知られており(GUI アプリの nixpkgs 版はコード署名やサンドボックス
  周りで壊れやすい)、apt と同じ「システム統合が要るものは薄い escape
  hatch に逃がす」原則をそのまま適用した。
- **fcitx5/mozc を darwin にも展開する** — mozc の macOS ネイティブビルドは
  nixpkgs で一般的でなく、macOS 標準 IME で要件を満たせるため、わざわざ
  移植する理由がない。

## Consequences

- macOS ホストのシステム層は Brewfile が正本になる。ドリフト検出は
  `brew bundle check --file=packages/declarative/Brewfile` で手動実施
  (apt 層に自動ドリフト検出が無いのと同じ非対称)。
- 今後 darwin ホストが増える場合、Brewfile とモジュール内 darwin 分岐の
  パターンをそのまま再利用できる。
- nix-darwin を採用する場合は、この ADR を supersede する新しい ADR を
  起こす。

## Verification

- `nix flake check --all-systems --no-build` — darwin config の eval が
  ubuntu ランナーで通ることを確認(型エラー・未ガードの Linux 専用
  オプションを検出)。
- CI(`.github/workflows/nix.yml`)の `build-host` matrix に追加した
  `altair` 行が `macos-latest` でフルビルドできること(herdr パッチ
  ビルドの実機確認を含む)。
- 実機検証は `docs/setup-macos.md` の手順に従う。
