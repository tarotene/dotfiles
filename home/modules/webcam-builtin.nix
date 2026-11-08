# webcam-builtin — 内蔵カメラ(Intel IPU7 + ov08x40。arcturus)を、アプリが普通の Web カメラとして
# 開けるようにする、ユーザー環境の側の中継。
#
# なぜ中継が要るか: IPU7 の /dev/video0〜31 は生の Bayer を返す取り込み口で、アプリはそのまま使えない。
# 色と露出は libcamera の Software ISP が整える(libcamera 0.6 以降の Simple pipeline。この機体の apt は
# 0.2.0 で、IPU7 を扱えない)。そこで、nixpkgs の libcamera(0.7.0)で映像を取り、システム層の
# v4l2loopback が作る仮想カメラ(/dev/video40、card_label "Built-in Camera")へ流す。
# システム層の側(v4l2loopback モジュールと、/dev/udmabuf を開ける udev 規則)は
# config/ にあり、scripts/install-packages.sh が配る。実測の表と未確認の項目は
# docs/webcam-builtin-ipu7.md。
#
# 既存手段をそのまま使う(ADR-543): 中継は v4l2-relayd(nixpkgs 0.1.5)。読む側(アプリ)が仮想カメラを開いたときだけ
# 入力のパイプライン(= カメラ)を動かし、閉じると止める(実測 2026-11-05: 読む側が居ないあいだ IPU7 の
# /dev/video0 を誰も握らず、読む側が居るあいだだけ握る)。常駐するのは待機の v4l2-relayd だけ。
# 先行例は NixOS の services.v4l2-relayd(nixos/modules/services/video/v4l2-relayd.nix)。
#
# v4l2-relayd 0.1.5 を、この機体で動かすのに要った 3 点(ソース src/v4l2-relayd.c を読み、実測した):
#  1. 入力・待機映像のパイプラインの末尾に `appsink` を置かない。v4l2-relayd が、末尾の「リンクされていない
#     src パッド」へ自分で appsink を付ける(backend_pipeline_create)。
#  2. 末尾が caps だけで終わる文字列は `no element "video"` で落ちる。末尾に `identity` を置く。
#  3. 出力の v4l2sink に `sync=false` が要る。無いと 24〜26 枚で止まり、実効 約 1.7 fps になった
#     (appsink → appsrc の時刻が出力側の時計と合わず、フレームが遅れとして捨てられていると読んだが、原因の細部は未確認)。
#
# 全画素モード(3840x2160 出力。センサーは 3856x2176)に固定する: 低解像度の 1928x1088 モードは、
# IPU7 のファームウェアが毎フレーム `error group 3 code 18`(INSYS_MSG_ERR_CAPTURE_HW_ERR_BAD_FRAME_DIM)を返し、
# 99.9% が未書き込みになる。アプリ向けには 1920x1080 へ縮める(負荷は 8 コア中 約 0.66 コア)。
#
# arcturus だけが有効にする(home/hosts/arcturus.nix)。
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.webcam.builtin;

  # 仮想カメラの番号は、システム層の設定(config/modprobe.d)の video_nr を単一の正本にして、ここで複写しない。
  modprobeConf = builtins.readFile ../../config/modprobe.d/v4l2loopback-builtin-camera.conf;
  videoNr = builtins.elemAt (builtins.match "(.*\n)*options v4l2loopback[^\n]* video_nr=([0-9]+)[^\n]*\n*" modprobeConf) 1;
  device = "/dev/video${videoNr}";

  gstPluginPath = lib.makeSearchPathOutput "lib" "lib/gstreamer-1.0" (
    with pkgs.gst_all_1;
    [
      gstreamer
      gst-plugins-base
      gst-plugins-good
      pkgs.libcamera
    ]
  );

  outCaps = "video/x-raw,format=YUY2,width=1920,height=1080,framerate=30/1";

  relay = pkgs.writeShellScript "webcam-builtin-relay" ''
    export GST_PLUGIN_SYSTEM_PATH_1_0=${gstPluginPath}
    exec ${pkgs.v4l2-relayd}/bin/v4l2-relayd \
      -i ${lib.escapeShellArg "libcamerasrc ! video/x-raw,format=RGBx,width=3840,height=2160 ! videoscale ! videoconvert ! ${outCaps} ! identity"} \
      -s ${lib.escapeShellArg "videotestsrc pattern=black is-live=true ! ${outCaps} ! identity"} \
      -o ${lib.escapeShellArg "appsrc name=appsrc caps=${outCaps} ! videoconvert ! v4l2sink name=v4l2sink device=${device} sync=false"}
  '';
in
{
  options.dotfiles.webcam.builtin.enable = lib.mkEnableOption ''
    the relay that feeds the built-in camera (Intel IPU7) into the virtual /dev/video camera
    (v4l2loopback, system layer) apps open. Needs the system-layer files from
    scripts/install-packages.sh (docs/webcam-builtin-ipu7.md)
  '';

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = pkgs.stdenv.isLinux;
        message = "dotfiles.webcam.builtin is a systemd user service; enable it on Linux only";
      }
    ];

    # ログインで起動する常駐の service の pipeline-map のノード(ADR-872)。
    dotfiles.pipeline.timerNodes.webcam-builtin-relay = "webcam-builtin-relay";

    systemd.user.services.webcam-builtin-relay = lib.mkIf pkgs.stdenv.isLinux {
      Unit = {
        Description = "Relay the built-in camera (IPU7) into the v4l2loopback virtual camera, on demand";
        # 仮想カメラの不在(v4l2loopback が読み込まれていない)を、黙って起動しないままにせず、
        # `hms` の degraded と `systemctl --user --failed` に出す。
        AssertPathExists = device;
      };
      Service = {
        Type = "exec";
        ExecStart = "${relay}";
        Restart = "on-failure";
        RestartSec = 5;
        NoNewPrivileges = true;
      };
      Install.WantedBy = [ "default.target" ];
    };
  };
}
