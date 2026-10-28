# atop-trace — vega の「数秒固まる」の原因を後から読めるように、CPU・メモリ・IO の圧力(PSI)と
# プロセスごとの使用量を 10 秒ごとに記録する(ADR-0000-cachyos-migration-hold の再訪条件 ①)。
#
# 既存手段をそのまま使う(ADR-543): #249 の 2026-10-26 の手順が atop の生ファイル
# (10 秒間隔)を指名している。自前の timer で /proc/pressure/* を記録する案は、atop の raw ファイルが
# PSI を持つので採らない(2026-10-28、nixos-26.05 の atop 2.12.1 を一般ユーザーで 5 秒間隔に回し、
# `atop -r <file> -P PSI` に CPU・メモリ・IO の圧力の行が出ることを読んだ)。
#
# 一般ユーザーで動く: 全プロセスの /proc は読めるが、短命なプロセスの記録(atopacctd の
# プロセスアカウンティング)は root が要るので持たない。固まりの前後の区間を見るには足りる。
# 量: 1 サンプルあたり約 8 KB(同じ実測、31 サンプル = 261 KB)、10 秒間隔で 1 日 約 65 MB。3 日分を残して消す。
#
# 読み方: `atop -r ~/.local/state/atop/atop_YYYYMMDD`(t で次、T で前、b で時刻へ)。固まったら
# `logger -t freeze-mark …` で印を付け、`journalctl -t freeze-mark` の時刻で該当区間を開く
# (docs/operations.md の「Freeze trace」)。vega だけが import する(home/hosts/vega.nix)。
{ config, pkgs, ... }:
let
  stateDir = "${config.xdg.stateHome}/atop";
  interval = 10; # 秒
  samplesPerFile = 8640; # 10 秒 × 8640 = 24 時間。終わると service が日付の新しいファイルで起動し直す
  keepDays = 3;
in
{
  # `atop -r` を手元で使うため PATH にも置く。
  home.packages = [ pkgs.atop ];

  # 1 つの service は 24 時間(10 秒 × 8640 サンプル)で終わり、timer が終了の 1 分後に
  # 日付の新しいファイルで起動し直す。常駐の service + Restart= にしないのは、(1) pipeline-map が
  # 自動発火の登録箇所として認めるのは timer だけだから(ADR-872)、(2) atop が壊れたとき、
  # Restart= は 5 秒ごとに再起動を繰り返すが、timer は 1 分に 1 回で済むから。
  dotfiles.pipeline.timerNodes.atop-trace = "atop-trace";

  systemd.user.services.atop-trace = {
    Unit = {
      Description = "atop raw recording for freeze diagnosis (${toString interval}s, ${toString keepDays} days)";
    };
    Service = {
      # 古い記録を消してから、今日の名前のファイルに記録する。`%%` は systemd の `%` のエスケープ、
      # `$$` は `$` のエスケープ。
      ExecStartPre = [
        "${pkgs.coreutils}/bin/mkdir -p ${stateDir}"
        "-${pkgs.findutils}/bin/find ${stateDir} -name 'atop_*' -mtime +${toString keepDays} -delete"
      ];
      ExecStart = "${pkgs.runtimeShell} -c 'exec ${pkgs.atop}/bin/atop -w ${stateDir}/atop_$$(${pkgs.coreutils}/bin/date +%%Y%%m%%d) ${toString interval} ${toString samplesPerFile}'";
      # 診断が診断の対象を悪化させない: CPU・IO とも最低の優先度。
      Nice = 19;
      IOSchedulingClass = "idle";
    };
  };

  systemd.user.timers.atop-trace = {
    Unit.Description = "Keep atop recording for freeze diagnosis";
    Timer = {
      # ユーザーの systemd が起動して 1 分後に最初の起動、以後は service が終わるたびに 1 分後。
      OnStartupSec = "1min";
      OnUnitInactiveSec = "1min";
      Unit = "atop-trace.service";
    };
    Install.WantedBy = [ "timers.target" ];
  };
}
