# Osmo Pocket 3 を vega の常用 Web カメラ + マイクにする

vega の机に Osmo Pocket 3(Creator Combo)を据え置き、会議で内蔵カメラと内蔵
マイクの代わりに使うための運用メモ。機器側とアプリ側の手順、既定マイクを
固定する仕組み、実測値、まだ確かめていないことをまとめる。

- 対象機体: vega だけ(arcturus・altair は対象外)。
- 使う場面: ブラウザ会議(Google Meet)と Slack の huddle。
- マイク: Combo 付属の DJI Mic 2。Osmo 内蔵のレシーバで受けた音が USB 音声に乗る。
- 設置・給電: Combo 付属のミニ三脚でモニタ脇に置き、PC の USB-C から給電する。

## 1. 機器側の手順(使うたびに)

根拠: DJI「Osmo Pocket 3 User Manual v1.0」"Webcam Mode"(p.27、
`https://dl.djicdn.com/downloads/DJI_Osmo_Pocket_3/UM/20250826/DJI_Osmo_Pocket_3_User_Manual_v1.0_en.pdf`、
取得 2026-10-23)と、同「Release Notes」v01.02.06.04(取得 2026-10-23)。

1. Osmo の電源を入れ、付属の USB-C ケーブルで vega の USB-C ポートに繋ぐ。
2. Osmo の画面に USB モードの選択が出る。「Webcam」をタップする。
   画面下部に「Webcam」と出れば完了。
3. 解像度とフレームレートを変えるときは、Webcam モードの画面を上にスワイプする
   (Release Notes v01.02.06.04: 1080P 50/60fps に対応、25/30fps も選べる)。
   常用は 1080p30 にする(「4. 実測値」を参照)。
4. Mic 2 の送信機の電源を入れる。Combo で買った送信機は Osmo とリンク済みで、
   電源を入れると自動でリンクする(取説 "Microphone Connection" の "DJI Mic 2
   Transmitter Connection" 節)。

毎回「Webcam」のタップが要る。USB モードを記憶する設定は、取説に見つかっていない。
タップしていない状態では、USB ID が `2ca3:0020` で見え、カメラも音声も vega に現れない
(2026-10-23 に実際に起きた。そこから Webcam に戻す手順は、この日は確かめていない)。

## 2. アプリ側の手順(1 回だけ、以降はアプリが記憶する)

Firefox は Meet で、カメラを v4l2 で直接開くことを実測した(「4. 実測値」)。
このためカメラの既定は WirePlumber では決められず、アプリごとに選ぶ。

- **Google Meet**(`https://meet.google.com/`): ホーム画面の「設定」アイコン →
  「動画」→「カメラ」で、「DJIPocket3」を含む項目を選ぶ。プレビューが
  「動画」の右端に出る(Google Meet ヘルプ「画質と音質の改善 - パソコン」
  `https://support.google.com/meet/answer/9302964?hl=ja`、取得 2026-10-23)。
  ブラウザの許可は、サイトごとに記憶される。
- **マイクは何も選ばない。** 「音声」→「マイク」は既定のままにする。既定が Osmo になる
  (「3. 既定マイクの仕組み」)。
- **Slack**(vega の正は nix 版。`home/modules/desktop.nix` の宣言、#957): 環境設定の音声と動画で、
  カメラに DJIPocket3 を選ぶ。**画面名・項目名は未確認**(Slack のこのバージョンで確かめていない)。
  手で入れた Flatpak 版 `com.slack.Slack`(2026-10-23 の読み取りでは `devices=all` で `/dev/video*` が
  見えた)は宣言に無く、撤去する対象である(#957)。nix 版でカメラが見えることは、vega の実機では未確認。

## 3. 既定マイクの仕組み

Osmo を Webcam モードで刺している間、PipeWire の既定の入力は Osmo のマイクになる
(2026-10-23 に実機で確認: 内蔵マイクが手動選択のままでも、`default.audio.source` が
Osmo になり、Osmo のノードの `priority.session` は 40000)。抜いたときに内蔵マイクへ
戻ることは、ソースから導いた予想で、実機では見ていない(「5. まだ確かめていないこと」)。

実現は `config/wireplumber/wireplumber.conf.d/52-osmo-pocket-3-mic.conf`(vega の
host モジュールだけが配備する)。Osmo の入力ノードに `priority.session = 40000` を
宣言する。WirePlumber 0.5.8 の既定ノードの選択は、手で選んだ機器に 30000、過去に選んだ
機器に 20001−i を足す。40000 はその最大(Osmo の素の 2009 + 30000)を超えるので、
順序にも手動の選択にも依らず勝つ。理由と読んだ行は、ドロップインのヘッダにある。

OpenFit Pro との関係(#892、**実機では未確認**): ヘッドセットが繋がると既定の入力は
ヘッドセットへ移るが、移した先の優先度は上の式で Osmo に負けるはず。そうなら、ヘッドセットは
出力専用になり、通話用のプロファイル(HFP)に落ちずに A2DP のまま鳴る。Osmo を抜くと、
#892 の挙動に戻る。

効かないときの確認:

```sh
wpctl status | sed -n '/Sources:/,/^$/p'          # DJIPocket3 Analog Stereo に * が付くか
pw-metadata 0 | grep default.audio.source          # alsa_input.usb-DJI… か
systemctl --user restart wireplumber               # ドロップインを足した直後は要る
```

別のマイクを使いたいときは、Osmo を抜くか、Osmo の USB モードを Webcam 以外にする。
刺さっている間は、GUI で別のマイクを選んでも効かない。

カメラ側には規則を置いていない。使うアプリが PipeWire を経由しないので、効く相手が
いない。Chrome(nix)が PipeWire 経由でカメラを開くと分かったら、同じドロップインに
`monitor.v4l2.rules` を足す。

## 4. 実測値(2026-10-23、vega、Pop!_OS、カーネル 6.17.4)

Osmo のファームウェアのバージョンは、本体の画面で確かめていない(未確認)。
数値はファームウェアで変わりうる。

| 項目 | 結果 |
|---|---|
| USB | `2ca3:0023`、High-Speed(480 Mbps、USB 2.01)、要求電流 500 mA |
| UVC のデバイス | `/dev/video2`(`DJIPocket3: OsmoPocket3`)。同時に `/dev/video3` も生えるが、フォーマットを列挙しない(用途は未確認) |
| USB ID とモード | Webcam モードのとき `2ca3:0023`。同じ日に、Webcam をタップしていない状態では `2ca3:0020` で見え、`/dev/video*` も音声デバイスも生えなかった(その状態のモード名は未確認) |
| ピクセルフォーマット | MJPG と H264 だけ(非圧縮なし) |
| 列挙された解像度 | 1280x720、1920x1080、3840x2160、縦向きの 720x1280・1080x1920 |
| 実測 1080p30(MJPG) | 32〜35 fps |
| 実測 4K30(MJPG) | 22〜25 fps(USB 2.0 の帯域で頭打ち) |
| UVC の制御 | `pan_absolute`、`tilt_absolute`、`zoom_absolute`(100〜400) |
| UAC の入力 | 48 kHz、S16、ステレオ。`alsa_input.usb-DJI_DJIPocket3_<serial>-02.analog-stereo` |
| UAC の出力 | `alsa_output.usb-DJI_DJIPocket3_<serial>-02.analog-stereo`(使わない) |
| Mic 2 の音 | USB 音声に乗る。送信機を 1 m 離してカプセルをこすった録音のピークが −2.5 dB |
| Firefox 147 の経路 | v4l2 を直接(`fuser /dev/video2` に firefox-bin、PipeWire の映像ストリームは 0) |

4K を常用しない理由: 4K30 の実測が 25 fps に届かず、帯域を使い切る。Meet のプレビューも
720p で開く。DJI も 4K の UVC には「USB ケーブルの品質と PC の性能が要件を満たすこと」を
求めている(Release Notes v01.04.08.02、2024-05-15)。

Mic 2 の音が USB に乗ることは、DJI の取説・FAQ・リリースノートには書かれていない。
上の録音の A/B で確かめた事実。

## 5. まだ確かめていないこと

人の操作が要る確認は #954(Osmo の常用で、抜いたときの復帰・OpenFit Pro との併用・
Chrome の経路・給電を実機で確かめる)に手順がある。結果が出たらこの節を更新する。

- **Osmo を抜いたとき、既定の入力が内蔵マイクに戻るか。**
- **OpenFit Pro を繋いだとき、入力が Osmo に残り、ヘッドセットが A2DP のままか。**
  2026-10-23 は OpenFit Pro が手元になく、確かめていない。確かめるときは、Osmo を Webcam
  で刺した状態でヘッドセットを繋ぎ、`pw-metadata 0 | grep default.audio` と
  `wpctl status` の Sinks / Sources を読む。
- **Webcam モードで電池が減るか。** DJI の FAQ は「録画中は外部電源から直接給電し、録画が
  止まったときだけ充電する」と書く。取説の LED の表には、連続点灯の緑という項目が無い
  (緑の点滅が「充電中」)。2026-10-23 に本人が見たステータス LED は、緑の連続点灯だった。
  意味は未確認。
- **Auto Power Off が Webcam モードの待機中に効くか。** 設定はある(取説 Settings)が、
  Webcam モードでの挙動は確かめていない。会議の途中で電源が落ちるかを、長時間使って確かめる。
- **Chrome(nix)がカメラを PipeWire 経由で開くか。** Chromium の PipeWire カメラは
  `about:flags` の背後にあった(2024 時点の情報)。現行の 155 は未計測。
- **Slack(Flatpak)の設定画面の名称。**

## 6. 参考: ジンバルを v4l2 から動かす

UVC の制御に pan と tilt があるので、会議中に画角を変えられる。
値の単位は 1/3600 度(`step=3600`)。

```sh
v4l2-ctl -d /dev/video2 --set-ctrl pan_absolute=0,tilt_absolute=0
v4l2-ctl -d /dev/video2 --set-ctrl zoom_absolute=200
```

効き方(実際にジンバルが動くか)は、2026-10-23 に確かめていない。
