# ~/Downloads を「溜め込めない場所」にする。
#
# 動機は溜め込み防止の強制力(/grill-me セッションで確定)。tmpfs は不採用: 守り
# たい不変条件は「N 日より古いものが無い」だが、tmpfs が表現するのは「再起動を
# 越えない」であり、再起動頻度が高いマシンでは朝の大きな受信物が昼に消える一方、
# 再起動しないまま数ヶ月放置すれば何も消えない — 動機に対して代理条件が悪い。
# 加えて standalone home-manager のユーザー systemd は非特権で tmpfs を mount
# できず、実現にはシステム層(fstab)か $XDG_RUNTIME_DIR(3 GB 台の上限)への
# 迂回が要り、三層モデルの層の規律を破る(ADR-0001)。
#
# 採る手段は systemd-tmpfiles の age cleanup(14 日、home-manager 既存の
# `systemd.user.tmpfiles.rules` 経由)。判定は tmpfiles 既定(atime/mtime/ctime
# のいずれか新しければ延命)で、/home が `noatime`(git.nix 側ではなくシステム
# 層のマウント)のため実質「最後の変更または到着から 14 日」になり、開いて読む
# だけでは延命しない。14 日は領収書・出張報告・面談メモ等「提出まで 1 週間超」
# の書類が実在するため(7 日では消えすぎる)。
#
# 対象は Linux(systemd)と darwin(launchd)の両方 — worktree.nix / drift.nix
# と同じ「配備を同じモジュールに同居させる」型(ADR-0018)。
{
  config,
  lib,
  pkgs,
  ...
}:
{
  # systemd.user.tmpfiles.rules は home-manager が Linux のみに実体化する
  # (assertPlatform)ため isLinux ガードは必須ではないが、drift.nix の書式に
  # 揃えて明示する。
  systemd.user.tmpfiles.rules = lib.mkIf pkgs.stdenv.isLinux [
    "d %h/Downloads - - - 14d"
  ];

  # launchd 版。TCC(macOS のプライバシー制御)は LaunchAgent が起動する実行体
  # の実パスに権限を記録する。nix store 配下のバイナリは rebuild ごとにパスが
  # 変わり付与が無効化されるため、Apple 署名済みで path が安定する
  # /usr/bin/find を直接呼ぶ(wrapper や配備物を新設しない)。削除条件は
  # tmpfiles の既定(3 種のタイムスタンプのいずれか新しければ延命)を find の
  # 論理積(AND)で写す。初回のみ TCC 権限付与が要る(docs/cutover-runbook.md
  # の Post-cutover 手順)。
  launchd.agents.downloads-clean = lib.mkIf pkgs.stdenv.isDarwin {
    enable = true;
    config = {
      ProgramArguments = [
        "/usr/bin/find"
        "${config.home.homeDirectory}/Downloads"
        "-mindepth"
        "1"
        "-atime"
        "+14"
        "-mtime"
        "+14"
        "-ctime"
        "+14"
        "-delete"
      ];
      StartInterval = 86400; # 1d — systemd 側の OnUnitActiveSec=1d に相当
      RunAtLoad = true; # systemd 側の OnStartupSec=5min に相当(初回も掃除する)
    };
  };
}
