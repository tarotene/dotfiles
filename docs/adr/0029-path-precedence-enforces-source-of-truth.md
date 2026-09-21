# ADR-0029 — PATH の優先順位を ADR-0001 の執行機構として宣言下に置く

- Status: Accepted
- Date: 2026-03-02
- Issue: #NNN
- Amends: ADR-0001 の「home-manager が user environment の source of truth
  である」という決定を*変更しない*。その決定を実際に執行する機構が
  存在しなかった、という欠落を埋める。ADR-0002 の rustup 存続、ADR-0025 の
  `~/.cargo/bin` 位置づけ、ADR-0006 の nixGL 前提は、いずれも本 ADR の
  制約として尊重される。

## Context

### 発端

COSMIC ランチャーに "Alacritty" が 2 個表示され、片方を選ぶと window が
2 個出る、という UI 上の違和感から調査が始まった。`.desktop` が 2 個ある
こと自体はすぐ分かった — 一方は Phase 4(#218)で退役した手続き型
インストーラが `~/.local/share/applications/` に手置きした残骸で、
home-manager 管理外なので `hms` を何度回しても消えない。

問題はその先だった。

### 実測 — 宣言と実体の乖離

`cosmic-launcher`(pid 2680)の PATH を `/proc/<pid>/environ` から採取した:

```
~/.go/bin : ~/.go/current/bin : ~/.cargo/bin : ~/.local/bin : ~/.nix-profile/bin : /nix/var/nix/profiles/default/bin : /usr/local/sbin : ...
```

`~/.cargo/bin` が `~/.nix-profile/bin` **より前**にある。つまり nixpkgs が
配る側の `.desktop` が書いている `Exec=alacritty` も、`~/.cargo/bin/alacritty`
に解決される。**2 つのエントリは同じ cargo 版 0.15.1 を起動していた。**

`home/modules/desktop.nix` が宣言している nixGL ラップ済み 0.17.0 は、
事実上一度も動いていなかった。稼働中の端末(pid 294588)の `/proc/<pid>/exe`
も `~/.cargo/bin/alacritty` を指していた。

これが数ヶ月にわたり気づかれなかったのは、cargo 版がシステムの mesa を
リンクして**普通に動いてしまう**からである。nix 版が nixGL なしで起動すると
window を開く前に落ちる(ADR-0006)ため失敗は可視だが、逆向きの置き換えは
無症状だった。唯一の症状がランチャーのエントリ重複だった。

### 実測 — alacritty は特殊例ではない

`~/.cargo/bin` ∩ `~/.nix-profile/bin` は 23 件。内訳は:

- **rustup シム 13 件**(`cargo` `rustc` `rustfmt` `rust-analyzer` … すべて
  `→ rustup` の symlink)。nix 側の `rustup` パッケージも同一機構の
  シムを配っており、どちらが勝っても `~/.rustup/toolchains` を読む。
  ADR-0002 の project-scoped toolchain 解決は影響を受けない。
- **`cargo install` 済みの実バイナリ 10 件** — nix 版を隠していた:
  `alacritty` `bat` `cargo-embed` `cargo-flash` `ghr`
  `interactive-rebase-tool` `probe-rs` `sheldon` `starship` `zellij`

`home/modules/packages.nix` は starship と sheldon について「`programs.*` /
shell module 由来」と書いているが、PATH 上では cargo 版が勝っていた。
加えて `~/.deno/bin` の `deno` も nix 版を隠していた。

**ADR-0001 の source-of-truth 宣言は、PATH という執行機構のレベルで
守られていなかった。**

### 原因 — 構造であって個別ミスではない

```
/etc/profile.d/nix.sh   (root)      → nix を PATH に入れる(最初)
~/.profile                          → .local/bin を prepend
~/.profile: . "$HOME/.cargo/env"    → .cargo/bin を prepend  ← 原因
~/.zprofile: . "$HOME/.go/env"      → go を prepend
```

nix はシステムレベル(`/etc/profile.d/`)で PATH に入る。したがって
**ユーザレベルの prepend はすべて構造的に nix を追い越す**。
どの ad-hoc インストーラも、自分のディレクトリを prepend する限り、
「最後に入れた者が勝つ」のではなく「必ず nix に勝つ」。

`~/.profile` / `~/.zprofile` / `~/.bash_profile` はいずれも home-manager
管理外の素のファイルで、`home.sessionPath` も未使用だった。
対話 zsh では `config/zsh/modules/40-dev-rust.zsh` と
`config/shell/common_env` が同じ prepend を重ねていた(prepend 地点は
計 3 箇所)。

### 実測 — 修正が安全であることの根拠

- `~/.cargo/bin` ∩ システム(`/usr/bin` `/usr/local/bin` `/bin`)= **0 件**
- `~/.deno/bin` / `~/.bun/bin` ∩ システム = **0 件**

  → これらを PATH 末尾へ回しても、システム側に奪われる名前は存在しない。
  失うものがない。
- `~/.local/bin` ∩ システム = `open` / `xdg-open` のみ。
  `scripts/detach-open.sh` による**意図的**な shadow(COSMIC の
  `xdg-open` が前景でブロックする問題への対処)。
  → `.local/bin` は先頭維持が必須。
- `~/.go/bin` ∩ nix = **0 件** → `.zprofile` は触る必要がない。
- apt 由来の rust は不在。

### 検討して棄却した機構

`~/.config/environment.d/` は本件に**届かない**。リポジトリには前例
(`10-fcitx5.conf`)があるが、`systemctl --user show-environment` の PATH は
`/usr/local/sbin:…:/snap/bin` のみで nix も cargo も含まないのに対し、
`cosmic-session` の PATH には両方ある。COSMIC セッションは systemd user
manager の環境を経由せず、login shell から起動されている。

## Decision

### 1. PATH の優先順位は ADR-0001 の執行機構である

「home-manager が source of truth である」は、PATH 上で
`~/.nix-profile/bin` が ad-hoc インストーラのディレクトリに勝つことで
初めて執行される。両者に同名バイナリが存在しうる以上、PATH 順序は
設計判断であって偶然に委ねてよい実装詳細ではない。

### 2. 規定する順序

```
$HOME/.local/bin  →  <nix profile>  →  <system>  →  ad-hoc installer dirs
```

- `.local/bin` が先頭 — home-manager が配るスクリプト群の置き場であり、
  かつシステム `xdg-open` を意図的に shadow している。
- nix がシステムに勝つ — 既に `/etc/profile.d/nix.sh` がそう置いている。
  本 ADR はその位置を**明示的に指定しない**。追い越さないだけでよい。
- ad-hoc インストーラのディレクトリ(`.cargo/bin` `.deno/bin` `.bun/bin`)は
  **末尾**。nix に同名があれば負け、なければそのまま使える。

### 3. ad-hoc インストーラのパスを prepend してはならない

シェル起動系のどこであれ、`$HOME` 配下のインストーラ固有ディレクトリを
PATH の**先頭**に足すことを禁じる。append のみ許す。
これは `. "$HOME/.cargo/env"` のような、prepend する外部スクリプトを
そのまま source することの禁止を含む。ファイル自体は残してよい
(第三者が source しても、既に PATH にあれば no-op になる)。

### 4. `~/.profile` を home-manager の管理下に置く

`~/.profile` は GUI セッションの PATH を決める唯一のファイルであり、
`Exec=` が裸の名前であるランチャーエントリはすべてここで組み上がった
PATH に対して解決される。管理外のまま放置すれば、任意のインストーラが
いつでも先頭に割り込める。

`home.file` で配り、`dotfiles.quarantine.managedFiles` で既存の実体を
`.pre-nix` に退避してから採用する。store symlink なので read-only であり、
外部からの追記は**設計として失敗する**(`config/herdr/config.toml` と
同じ性質)。

Linux 限定とする(ADR-0018 に従いモジュール内で分岐)。実測は COSMIC の
ものであり、darwin の login shell は `/etc/zshrc` 経由で nix に到達する
ため事情が異なる。未実測のホストのログインファイルを採用するのは避ける。

### 5. 裸の `Exec=` に依存しない

`.desktop` の `Exec=` は store path の絶対パスにする
(`config/autostart/fcitx5.desktop` と同じ `pkgs.replaceVars` の形)。
PATH を直したとしても、`.desktop` が裸の名前で解決する限り
「宣言と実体の乖離」は再発しうる経路として残るため、二枚重ねにする。

## Consequences

### 良い方向

- 10 件 + `deno` の shadow が一括で解消する。個別に `cargo uninstall` して
  回る必要がない(それは症状潰しであり、次の `cargo install` で再発する)。
- 以後 `cargo install` / `deno install` しても、nix に同名があれば
  宣言側が勝つ。**新しい shadow が構造的に発生しなくなる。**
- ADR-0006 の保証(nix GUI アプリは nixGL 経由で自前 GL を使う)が
  alacritty について回復する。
- `~/.profile` への割り込みが失敗として可視化される。

### 受け入れるコスト

- **rustup の profile 追記が失敗しうる。** 症状は
  `error: could not amend shell profile: '/home/tarotene/.profile'`
  (文字列をバイナリ内に実在確認済み)。発生経路は `rustup-init` と
  `rustup self uninstall` の 2 つのみで、`rustup update` /
  `rustup toolchain install` / `rustup self update` は `~/.profile` に
  触らない(`rustup self` のサブコマンドは `update` / `uninstall` /
  `upgrade-data` の 3 つ)。
  `home/modules/runtimes.nix` が `rustup` を nix パッケージとして宣言して
  おり、`bootstrap.sh` にも `docs/setup.md` にも `rustup-init` を走らせる
  箇所がないため、実際の発生確率はほぼゼロ。
  回避は `rustup-init --no-modify-path`(`NO_MODIFY_PATH` の実在確認済み)。
  失敗時点で PATH は既に正しいので実害もない。
- **`~/.profile` の採用はログイン不能事故のリスクを持つ。** 適用時は
  別 TTY を開いたまま再ログイン検証する。`.pre-nix` が残るため手動復旧も
  generations による rollback も可能。
- cargo 側に残る 10 個の実バイナリは PATH 上では死蔵になる。削除は
  別 Issue(本 ADR の射程は順序であって在庫ではない)。

### 射程外

- `~/.local/bin` ∩ `~/.nix-profile/bin` の shadow(`mise` `uv` `uvx`
  `claude`)は別問題として残す。`.local/bin` を nix の後ろへ回すと
  `claude` の実体が入れ替わり、`claude-plan-model` が読む「installed
  claude の baked model catalog」が変わってしまう — 本 ADR が正そうと
  している「黙った実体すり替え」と同じ事故を起こす。別 Issue で棚卸しする。
- `~/.go/bin`(nix と衝突 0 件)。
- 「window が 2 個出る」機序そのもの。`setsid ~/.cargo/bin/alacritty -e
  sleep 20` は 1 プロセスしか生まないことを実測しており、バイナリ側の
  問題ではない。`StartupWMClass=Alacritty` を共有する 2 エントリを
  COSMIC ランチャーが併合しているものと見られる。エントリが 1 個に
  なっても残るなら COSMIC 側の別 Issue として切る。

## 姉妹事案

#300「ROS PYTHONPATH がシェル起動時に `uv run pytest` 等を壊す」は
同クラスの問題である — 外部インストーラがシェル起動時の環境変数を汚染し、
宣言側の意図を上書きする。本 ADR は将来この種の判断の基準になる。
