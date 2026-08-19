# ADR-0006 — nix GUI アプリは自前の GL スタックを抱える

- Status: Accepted
- Date: 2026
- Issue: #13

## Context

nixpkgs でビルドされた GL アプリケーションは、ドライバを `/run/opengl-driver`
に探しに行く。これは NixOS 専用のパスであり、Pop!_OS には存在しない。結果として
EGL が初期化できず、home-manager が入れた GUI アプリは二通りに壊れる。

- **落ちる**: `alacritty` はウィンドウを開く前に終了する。

  ```
  $ ~/.nix-profile/bin/alacritty -e true
  Error: ... NotSupported("provided display handle is not supported")
  ```

- **黙ってソフトウェア描画に落ちる**: Chrome / Slack / Zoom は EGL 喪失を握り潰し、
  GPU プロセスを `--use-gl=disabled` で走らせる。

  ```
  MESA-LOADER: failed to open dri: /run/opengl-driver/lib/gbm/dri_gbm.so
  ERROR:ui/gl/gl_display.cc:638] eglInitialize OpenGL failed with error EGL_NOT_INITIALIZED
  ```

後者が長く見過ごされたのは、症状が「動くが遅い」だったからで、ノートPCでは
バッテリーと性能の実損になる。

グラフィックスタックそのものは ADR-0001 でシステム層（apt / root 所有）に置くと
決めている。壊れているのはそのドライバではなく、**user 層のアプリがシステム層の
ドライバに到達できない**ことなので、この橋渡しをどの層に置くかを決める必要がある。

## Decision

**nix の GUI アプリは、システムの mesa ではなく nix の mesa を自分で抱えて動かす。**
`nixGL` を flake input として採用し、GL を使うパッケージだけを明示的にラップする。
システムのグラフィックスタックには一切触れない。

具体的には:

1. `flake.nix` に `nixgl` input を追加し、`inputs.nixpkgs.follows = "nixpkgs"` を
   付ける。これは整理のためではなく必須で、nixGL が配る mesa / libglvnd は
   アプリケーションのプロセスに dlopen されるため、nixpkgs が二重になると glibc が
   二重になり `GLIBC_2.x not found` になる。
2. オーバーレイで `pkgs.nixgl` を生やす。`nixgl.packages.<system>.nixGLIntel` を
   そのまま使わず `default.nix` を直接 import しているのは、nixGL の flake output が
   x86_64-linux で `enable32bits = true` を決め打ちしており、外から上書きできないため。
3. `home/modules/desktop.nix` に `nixGLWrap` ヘルパーを1つ定義し、`alacritty` /
   `google-chrome` / `slack` / `zoom-us` に**明示的に**適用する。
4. `nixGLIntel` 実行ファイル自体も `home.packages` に置き、アドホックな GL アプリと
   切り分け用のエスケープハッチとする。

### ラッパーが `bin/` と `.desktop` の両方を書き換える理由

パッケージによって起動経路が違う。Alacritty と Zoom は `Exec=alacritty` /
`Exec=zoom` という素の名前を書いており PATH で解決されるが、**Chrome と Slack は
絶対ストアパスを書き込んでいる**。`bin/` のラップだけで済ませると、シェルからは
直るのにランチャーからは壊れたまま、という最悪の非対称が残る。

### 一括 map ではなくパッケージ毎に明示適用する理由

GUI パッケージのリスト全体に map すると、次に GL と無関係な GUI パッケージを足した
瞬間に 1.1 GiB の GL クロージャが無言で付いてくる。fcitx5 の autostart を
プロファイルシンボリックリンクではなくストアパスで固定したのと同じ理由で、
「宣言されているもの」と「実際に効いているもの」を乖離させない。

### 32bit GL を落とす

`enable32bits = false` を渡す。実測でラッパーのクロージャが 2.1 GiB → 1.1 GiB になり、
alacritty の起動は変わらなかった。現在の GL 消費者はすべて 64bit である。
**Steam や wine を nix で入れるなら true に戻すこと。**
`enableIntelX86Extensions` は true のまま残す。これが `LIBVA_DRIVERS_PATH` を
intel-media-driver に向けるもので、「GPU がある Chrome」と「動画をデコードできる
GPU がある Chrome」の差になる。

## Alternatives considered

### 1. システムの mesa を指すだけにする（棄却: 実測で動かない）

nix mesa をクロージャに抱えたくないので、ローダ変数をシステムの mesa に向けてみた。
**動かない。**

```
$ GBM_BACKENDS_PATH=/usr/lib/x86_64-linux-gnu/gbm \
  LIBGL_DRIVERS_PATH=/usr/lib/x86_64-linux-gnu/dri \
  __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  ~/.nix-profile/bin/alacritty -e true
Error: ... NotSupported("provided display handle is not supported")   # 変化なし
```

`/usr/share/glvnd/egl_vendor.d/50_mesa.json` が指す `library_path` は
`"libEGL_mesa.so.0"` という素の名前で、nix のプロセスからは解決できない。
解決できるようにすれば今度は glibc-2.39 リンクのドライバを glibc-2.42 の
プロセスに読み込ませることになる。**nix の mesa を持つのは好みではなく唯一の選択肢。**

同じ3変数を **nix の mesa** に向けると `LD_LIBRARY_PATH` なしでも起動した
（クロージャ約 1.0 GiB）。つまり自前で最小ラッパーを書く道もあったが、VA-API と
Vulkan の面倒を自分で見続ける義務を負うので、上流の nixGL に乗った。

### 2. nixglhost（棄却: Mesa 非対応）

numtide の `nix-gl-host` はホストのドライバを再利用するので nix mesa が不要になる。
しかし README のサポート表で Mesa / Nouveau / AMD proprietary はいずれも非対応で、
実質 Nvidia プロプライエタリ専用。3台とも Intel/Mesa の当環境では使えない。

### 3. `/run/opengl-driver` を nix の mesa に張る（棄却: 壊れ方が悪い）

nixpkgs の libglvnd/mesa はこのパスを焼き込んでいるので、symlink を1本作れば
ラッパーも `.desktop` 書き換えも不要になり、`nix run` で拾ったアドホックな GL アプリ
まで直る。最も手数が少ない。それでも採らない:

- root 所有の system layer に踏み込む（ADR-0001 のエスケープハッチ側の話になる）
- symlink の先が **GC ルートにならない**。`nix-collect-garbage` が実体を消した瞬間に
  全 GUI アプリが再び死ぬ。しかも原因が症状から遠い
- flake を更新して mesa のストアパスが変わるたびに system unit 側を追随させる必要があり、
  「宣言と実体の乖離」を自分から作り込むことになる

user 層で完結し、home-manager の世代管理とロールバックがそのまま効く方を採る。

## Consequences

- home-manager のクロージャが 5.4 GiB → 6.7 GiB に増える。CI（`nix.yml`）は
  cache.nixos.org からこの差分を追加で引く。
- ラップした結果は素の derivation なので `passthru` と `.override` が落ちる。
  `.override` を使いたい場合は **ラップする前の**パッケージに適用する。
- GL を使う GUI パッケージを追加したら `nixGLWrap` で包むことを明示的に判断する。
  包み忘れの症状は「起動しない」か「黙って遅い」のどちらかになる。
- Nvidia のホストが増えたらこの ADR は書き直しになる。`auto.nixGL*` は
  `/proc/driver/nvidia/version` を読むため `--impure` を要求し、`nix flake check` と
  ホスト別ビルドマトリクスが壊れる。現在は3台とも Intel/Mesa なので `nixGLIntel` で
  純粋評価を保てている。

## Verification

`company-pop-new`（Intel Lunar Lake / Pop!_OS 24.04 / COSMIC Wayland）で実測:

- `alacritty -e true` が無言で成功する（従来は即死）
- Chrome の GPU プロセスが `/dev/dri/renderD128` に 8 本の fd を持ち、
  `libEGL_mesa` と `libgallium` をマップしている。`--use-gl=disabled` は解消し、
  `MESA-LOADER` / `eglInitialize` エラーはログから消えた
- Slack も同じ署名（`/dev/dri` fd 6 本、mesa の GL ライブラリ 11 個）
- Chrome は従来どおり `zwp_text_input_v3` をバインドしており、#14 の日本語入力経路に
  回帰はない（`WAYLAND_DEBUG=1` で確認）
- Zoom は検証できていない。GL 以前にこのホストで起動しないため（#24）
