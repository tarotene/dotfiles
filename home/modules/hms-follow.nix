# hms-follow: main が進んだら `hms --follow` が自動で追従する(ADR-1068)。
# 10 分ごとに timer(Linux は systemd、darwin は launchd)が `hms --follow` を呼ぶ。
# 追従するのは、いまの世代を引数なしの `hms` が作った場合だけで、`hms .` などで当てた
# 検証用の世代・hms を通さない switch・許可ポリシーの変更を含む main には当てず、
# herdr の通知で 1 回だけ知らせる。判断は crates/hms の `follow_decision`、設計は
# docs/operations.md「Following main automatically」。
#
# 配備するのは timer / launchd agent だけ。`hms` 自体は packages.nix が
# `~/.local/bin/hms` に置く。
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.hmsFollow;
  home = config.home.homeDirectory;
  hmsPath = "${home}/.local/bin/hms";

  # PATH は世代によらず同じ固定の値にする。store パス(`lib.makeBinPath`)を埋めると
  # 世代ごとに unit / plist の中身が変わる。launchd は plist が変わると agent を bootout
  # するので、追従が自分の走らせた switch で打ち切られうる(home-manager の
  # modules/launchd は plist が同一なら bootout しない)。hms が呼ぶ nix・home-manager・
  # herdr・systemctl・hostname・pgrep・tailscale-prefs は、どれも次のどこかにある。
  # Linux の systemd も同じ値にして、2 つの OS で食い違わせない。
  followPath = lib.concatStringsSep ":" [
    "${home}/.nix-profile/bin"
    "/nix/var/nix/profiles/default/bin"
    "${home}/.local/bin"
    "/usr/bin"
    "/bin"
  ];
in
{
  options.dotfiles.hmsFollow.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = ''
      main が進んだら `hms --follow` で自動追従する timer を配備するか。止めたいときは
      false にして `hms` を当てる(一時的に止めるだけなら
      `systemctl --user stop hms-follow.timer`)。
    '';
  };

  config = lib.mkIf cfg.enable {
    systemd.user.services.hms-follow = lib.mkIf pkgs.stdenv.isLinux {
      Unit = {
        Description = "Apply pushed main with hms when this generation was made by a bare hms";
        # keep-old: switch は、この unit が実行中でも再起動しない。追従の最中の switch が
        # 自分自身を再起動して、適用を途中で打ち切るのを防ぐ。
        X-SwitchMethod = "keep-old";
      };
      Service = {
        Type = "oneshot";
        ExecStart = "${hmsPath} --follow";
        Environment = "PATH=${followPath}";
        # oneshot の既定の TimeoutStartSec は無限。止まった適用が次の周期を塞ぎ続けない
        # ように、ビルドを含めても十分長い上限を置く。
        TimeoutStartSec = "1h";
      };
    };

    # `Persistent=` は OnCalendar= の timer にだけ効く(drift.nix の注記)ので書かない。
    # 起動の 5 分後に最初の追従を見て、以後 10 分おき。起動直後のネットワーク未復帰と
    # ぶつからないよう、少しずらす。
    dotfiles.pipeline.timerNodes.hms-follow = "hms-follow";
    systemd.user.timers.hms-follow = lib.mkIf pkgs.stdenv.isLinux {
      Unit.Description = "Follow pushed main with hms every 10 minutes";
      Timer = {
        OnBootSec = "5min";
        OnUnitActiveSec = "10min";
        RandomizedDelaySec = "1min";
        Unit = "hms-follow.service";
      };
      Install.WantedBy = [ "timers.target" ];
    };

    # launchd 版(ADR-0018)。StandardOutPath の親ディレクトリは launchd が作らないので、
    # 既存の `~/Library/Logs` に置く。
    # pipeline-map の対応付けは、上の systemd 側の dotfiles.pipeline.timerNodes.hms-follow が兼ねる。
    launchd.agents.hms-follow = lib.mkIf pkgs.stdenv.isDarwin {
      enable = true;
      config = {
        ProgramArguments = [
          hmsPath
          "--follow"
        ];
        StartInterval = 600;
        RunAtLoad = false;
        EnvironmentVariables.PATH = followPath;
        StandardOutPath = "${home}/Library/Logs/hms-follow.log";
        StandardErrorPath = "${home}/Library/Logs/hms-follow.log";
      };
    };
  };
}
