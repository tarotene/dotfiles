# 内蔵カメラ(Intel IPU7 + ov08x40)を arcturus の Web カメラにする

arcturus(Dell Pro 14 Premium、Lunar Lake)の内蔵カメラを、ブラウザなどが普通の
Web カメラとして開けるようにするための運用メモ。仕組み、使い方、実測値、
まだ確かめていないことをまとめる。

- 対象機体: arcturus だけ(vega は別の机用カメラ。altair は対象外)。
- 設定の置き場: システム層は `config/`(`udev`・`modprobe.d`・`modules-load.d`)を
  `scripts/install-packages.sh` が `/etc` へ配る。ユーザー環境は
  `home/modules/webcam-builtin.nix`(`dotfiles.webcam.builtin.enable`)。

## 1. 仕組み

```
ov08x40 → Intel CVS → IPU7 CSI2 → /dev/video0〜31(生の Bayer)
                                     │ libcamera 0.7.0(nixpkgs)の Simple pipeline + Software ISP
                                     ▼  3840x2160 RGB → 1920x1080 YUYV
                              v4l2-relayd(読む側が居るあいだだけカメラを動かす)
                                     ▼
                         /dev/video40「Built-in Camera」(v4l2loopback)
                                     ▼
                              Chrome などのアプリ
```

- IPU7 の `/dev/video0〜31` は生の Bayer を返す取り込み口で、アプリはそのまま使えない。
  色と露出は libcamera の Software ISP が整える。
- この機体の apt の libcamera は 0.2.0 で、IPU7 の Simple pipeline + Software ISP
  (0.6 以降の報告)を持たない。そのため libcamera は nixpkgs の 0.7.0 を使う。
  PipeWire の libcamera プラグイン(apt の `libspa-0.2-libcamera`)は、同じ
  `libcamera0.2` に依存するので使えない見込み(0.2.0 での実測はしていない)。
- 仮想カメラ(v4l2loopback)はカーネルモジュールなので、システム層(apt の DKMS)。
  `v4l2loopback-dkms` を `home/hosts/arcturus.nix` が宣言する。
- `/dev/udmabuf` を一般ユーザーに開ける udev 規則は、systemd 本体の
  `rules.d/70-uaccess.rules.in` と同じ 1 行(取得 2026-11-05)。この機体の systemd 255 には
  無いので足している。`/dev/dma_heap/*` は開けない。

## 2. 使い方

適用の順序(新しい機体、または初回):

1. `hms`(ユーザー環境。中継の user service が入る)。
2. `scripts/install-packages.sh`(システム層。udev 規則・モジュールのオプションを `/etc` へ配り、
   `v4l2loopback` を読み込む)。`--dry-run` で、何を配るかを先に読める。

その後は、アプリのカメラ選択で「Built-in Camera」を選ぶだけでよい。アプリが閉じると、
カメラは自動で止まる(待機の `v4l2-relayd` だけが常駐する)。

状態の確かめ方:

- `systemctl --user status webcam-builtin-relay.service`: 待機中の中継。
- `v4l2-ctl --list-devices`: 「Built-in Camera」が `/dev/video40` として出る。
- `fuser -v /dev/video0`: 読む側が居ないあいだは何も出ない(カメラは止まっている)。
- カメラそのものの診断: `nix shell nixpkgs#libcamera -c cam --list`。

## 3. 実測値(2026-11-05、arcturus、Pop!_OS、カーネル 7.2.6-76070206-generic)

| 項目 | 結果 |
|---|---|
| カーネル側 | IPU7・ov08x40・intel_cvs が読み込まれ、`ov08x40 → Intel CVS → CSI2 0 → Capture 0` が繋がる(`media-ctl -p`) |
| libcamera 0.7.0 | `Internal front camera` を検出する(`cam --list`) |
| 低解像度 1928x1088 モード | **使えない。** 99.9% が未書き込みで、カーネルに `error group 3 code 18` が毎フレーム出る |
| 全画素 3856x2176 モード | 28.57 fps、エラー 0 件 |
| `/dev/udmabuf` だけを開けた Software ISP | 3840x2160 の RGB を 28.57 fps で出す(`dma_heap` を開けない状態) |
| 仮想カメラへの中継 | 1920x1080 YUYV を 300 枚、28.57 fps で届けた(`v4l2-ctl --stream-mmap`) |
| オンデマンド | 読む側が居ないあいだ `/dev/video0` を誰も握らず、読む側が居るあいだだけ `v4l2-relayd` が握る |
| 負荷 | 4K 受け → 1080p YUY2 の GStreamer 経路で、プロセス全体 約 8.3%(8 コア中 約 0.66 コア。GStreamer の `rusage`) |
| 画質 | 色はやや緑に寄る。ov08x40 用のチューニングファイルが libcamera に無く、`uncalibrated.yaml` へ退避する |

低解像度モードを使わない理由: コード 18 は、カーネルの
`drivers/staging/media/ipu7/abi/ipu7_fw_isys_abi.h` で `INSYS_MSG_ERR_CAPTURE_HW_ERR_BAD_FRAME_DIM`
(グループ 3 = CAPTURE)。**なぜ寸法が不正と判定されるかの原因は未確認**(センサードライバーの
誤りか、ファームウェアの制約かを断定していない)。

## 4. `v4l2-relayd` 0.1.5 の落とし穴(中継の宣言に反映済み)

ソース(`src/v4l2-relayd.c`)を読み、この機体で実測した。

1. 入力・待機映像のパイプラインの末尾に `appsink` を置かない。`v4l2-relayd` が、末尾の
   「リンクされていない src パッド」へ自分で `appsink` を付ける(`backend_pipeline_create`)。
   置くと `no src pad available` で `NULL` が返り、終了処理の `gst_element_set_state` が
   `GST_IS_ELEMENT` の assertion で落ちる(エラー本文は `GST_DEBUG=V4L2_RELAYD:4` を付けないと出ない)。
2. 末尾が caps だけで終わる文字列は、`no element "video"` で落ちる。末尾に `identity` を置くと通る
   (原因の細部は未確認)。
3. 出力の `v4l2sink` に `sync=false` が要る。無いと 24〜26 枚で止まり、実効 約 1.7 fps になる。

## 5. まだ確かめていないこと

- **撮影中のプライバシー LED が点くか。** 同機種の報告では、今のカーネルの既定(`06cb:0701` の
  `ICVS_HOST_PRIV_CTRL`)では点かない。本人が目視していない。
- **Chrome のページで、実際に映像が出たか。** Chrome が `/dev/video40` を開いて読んでいることは
  `fuser` で確認した。ページの映像は確認していない。
- **Firefox・Zoom・Slack から見えるか。** 確認を省略した(Chrome だけで十分という判断)。
- **サスペンドから復帰したあとに映るか。** 同機種の報告に、カメラ使用後のサスペンドが `-EBUSY` で
  失敗する例がある(ただし Intel の psys DKMS を使う構成。この構成は使っていない)。確認していない。
- **画質の調整。** ov08x40 用のチューニングファイルを書けば、色と露出が整う可能性がある。

## 6. 参考

- systemd, `rules.d/70-uaccess.rules.in`(`KERNEL=="udmabuf", TAG+="uaccess"`、取得 2026-11-05):
  https://github.com/systemd/systemd/blob/main/rules.d/70-uaccess.rules.in
- NixOS, `services.v4l2-relayd`(取得 2026-11-05):
  https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/services/video/v4l2-relayd.nix
- 同機種(Dell Pro 14 Premium PA14250)の報告(取得 2026-11-05):
  https://github.com/jibsta210/svp7500-camera-fix-pack/issues/11
