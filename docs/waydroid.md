# Waydroid(Android コンテナ)で Android 版 LINE を動かす — vega

#323。LINE は今、Chromium の `--app` 方式の独立アプリ(`home/modules/line.nix`)で動いている。Waydroid +
Android 版 LINE は、より本物に近い代替。この文書は、宣言(何を置いたか)と、人が実機で行う手順、まだ確かめていないことを分ける。

## 宣言したもの(この変更)

| 何 | どこ | 理由 |
|---|---|---|
| apt パッケージ `waydroid` | `home/hosts/vega.nix` の `dotfiles.apt.extraPackages` | root のコンテナ実行環境と kernel の binder が要る。system layer(ADR-0001) |
| Waydroid の apt リポジトリの追加 | `scripts/install-packages.sh` | Ubuntu/Pop!_OS の archive に無い。一覧に `waydroid` がある host でだけ追加する(arcturus と CI は何もしない) |

## 読んだ事実

- vega の kernel は `7.2.6-76070206-generic`。`/proc/filesystems` に `nodev binder` があり、`CONFIG_ANDROID_BINDER_IPC_RUST=y`・
  `CONFIG_ANDROID_BINDER_DEVICES="binder,hwbinder,vndbinder"`。`CONFIG_ANDROID_BINDER_IPC` は未設定(`# … is not set`)。
  `/dev/binderfs` は無い(読んだ日: 2026-10-28、`/boot/config-$(uname -r)` と `/proc/filesystems`)。
  旧 kernel 6.17.4 の「DKMS の binder モジュールが要る」という前提(#323 の起票時)は、この kernel では当てはまらない見込み。
- Waydroid の apt リポジトリ(https://repo.waydro.id、導入スクリプトを**実行せず**読んだ。2026-10-28): keyring は
  `https://repo.waydro.id/waydroid.gpg` を `/usr/share/keyrings/waydroid.gpg` に置き、`deb [signed-by=…] https://repo.waydro.id/ <codename> main`
  を `/etc/apt/sources.list.d/waydroid.list` に書く。受け付ける codename に `noble` がある。
  `dists/noble/main/binary-all/Packages` に `waydroid` 1.6.3(Architecture: all)があり、依存は `lxc`・`python3-gbinder`・`dbus`・
  `polkitd | policykit-1`・`pipewire-pulse | pulseaudio`・`iptables` など。
- 公式の導入文書(https://docs.waydro.id/usage/install-on-desktops、取得 2026-10-28)の Debian/Ubuntu の節: Wayland セッションを
  有効にする(Ubuntu 22.04+)。Ubuntu 26.10 未満は第三者リポジトリが要る。導入後は「アプリケーションメニューから Waydroid を
  起動する」。systemctl を使わない起動は `sudo waydroid container start` のあと、別の端末で `waydroid session start`(sudo なし)。

## 人の手順(vega の端末で、1 手順ずつ。終わったら結果を #323 に書く)

1. **リポジトリと一覧を揃えて入れる。**
   - 遷移: vega の端末で dotfiles の checkout(`git pull --ff-only` 済みの main)へ移り、先に `hms`(この変更が main に入ったあと。
     `docs/operations.md`)で、`~/.config/dotfiles/apt-packages.txt` に `waydroid` が入る。
   - コマンド: `./scripts/install-packages.sh --dry-run` で `Would install Waydroid apt repo` の行を確かめ、`./scripts/install-packages.sh`。
   - 完了確認: `apt-cache policy waydroid` が `repo.waydro.id` の候補を出し、`dpkg -l waydroid` が `ii`。
   - 根拠: `scripts/install-packages.sh`(Tailscale と同じ型)、公式の導入文書。
2. **コンテナを起動する。**
   - コマンド: `sudo waydroid container start`。続けて別の端末で `waydroid session start`(sudo なし)。
   - 完了確認: 端末に `Android with user 0 is ready` が出る(公式の導入文書の「Manually Starting Waydroid」)。
   - **未確認**: binderfs の mount を Waydroid が自分で行うか(`/dev/binderfs` は今は無い)。Rust 版 binder で Android が起動するか。
     失敗したら、出力と `journalctl -u waydroid-container` をそのまま #323 に貼る。
3. **初期化(Android のイメージの取得)。** 公式の文書は「アプリケーションメニューから Waydroid を起動する」とだけ書く
   (取得 2026-10-28)。メニュー上の項目名は COSMIC では未確認。`waydroid init` の引数(Google アプリの有無など)も、この文書では
   読んでいないので未確認。LINE を入れるのに Play ストアが要るか(`-s GAPPS` 相当)は、手順 4 の前に決める。
4. **LINE を入れて、サブ端末として承認する。** LINE は 2025-03 以降、Android のサブ端末 1 台を公式に許可している(#323 の起票時の記載。
   一次ソースは未再確認)。スマートフォン側の承認の画面名は未確認。
   - 完了確認: Waydroid 内の LINE でメッセージを送受信でき、スマートフォンの LINE も引き続き使える。

## 戻し方

宣言を外す(`home/hosts/vega.nix` の `waydroid`)だけでは apt のパッケージは消えない。消すときは `sudo apt remove waydroid` と、
`/etc/apt/sources.list.d/waydroid.list`・`/usr/share/keyrings/waydroid.gpg` の削除。公式の文書は、やり直しのときに
セッションとコンテナを止めてから削除し、reboot してから後始末をする手順を持つ(「Reinstalling Waydroid」)。

## まだ確かめていないこと(#323 に残す)

- binderfs の mount、Rust 版 binder での実動作、Android の初期化(手順 2)。
- 初期化の引数と、Play ストアの要否(手順 3)。
- COSMIC(Wayland)での表示と入力(fcitx5 での日本語入力を含む)。
- 動いたときに、Chromium の LINE(`home/modules/line.nix`)を外すか残すか。
