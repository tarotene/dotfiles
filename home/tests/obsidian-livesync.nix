# LiveSync CLI の daemon(home/modules/obsidian-livesync.nix、#1328)の宣言を評価で確かめる。
#
# `enabled` は `dotfiles.obsidian.livesync.enable = true` を足した vega の config、
# `disabled` は既定のままの vega の config。評価するだけで、generation は build しない。
{
  pkgs,
  lib,
  enabled,
  disabled,
}:
let
  vault = enabled.dotfiles.obsidian.vaultPath;
  service = enabled.systemd.user.services.obsidian-livesync;
  # home-manager は ExecStart を文字列のリストにまとめる。
  execStart = lib.concatStringsSep " " (lib.toList service.Service.ExecStart);
  packageNames = cfg: map (p: p.name or "") cfg.home.packages;
  wrapper = lib.findFirst (p: (p.name or "") == "obsidian-livesync") null enabled.home.packages;
in
# 既定は無効: unit も、ラッパーも、生の CLI も無く、Obsidian の自動起動が残る。
assert !(disabled.systemd.user.services ? obsidian-livesync);
assert !(builtins.elem "obsidian-livesync" (packageNames disabled));
assert disabled.xdg.configFile ? "autostart/obsidian.desktop";
# 有効: ExecStart はラッパーの `daemon` だけ(生の CLI を直接呼ばない)。
assert lib.hasSuffix "/bin/obsidian-livesync daemon" execStart;
assert !(lib.hasInfix "obsidian-livesync-cli" execStart);
# PATH に出るのはラッパーだけ。
assert builtins.elem "obsidian-livesync" (packageNames enabled);
assert !(lib.any (name: lib.hasPrefix "obsidian-livesync-cli" name) (packageNames enabled));
# Condition でなく Assert(印が無い・vault が無いと、起動できない失敗が degraded に出る)。
assert lib.hasSuffix "/obsidian-livesync/initialized.json" service.Unit.AssertPathExists;
assert service.Unit.AssertPathIsDirectory == vault;
assert !(service.Unit ? ConditionPathExists);
assert !(service.Unit ? ConditionPathIsDirectory);
# 上流 `deploy/livesync-cli.service`(1.0.44-cli)の値と、秘密を tmpfs に閉じる値。
assert service.Service.Type == "exec";
assert service.Service.Restart == "on-failure";
assert service.Service.RestartSec == 10;
assert service.Service.LimitNOFILE == 65536;
assert service.Service.RuntimeDirectory == "obsidian-livesync";
assert service.Service.RuntimeDirectoryMode == "0700";
assert service.Service.UMask == "0077";
assert service.Install.WantedBy == [ "default.target" ];
# daemon が窓の無い同期を担うので、Obsidian の自動起動は消える。
assert !(enabled.xdg.configFile ? "autostart/obsidian.desktop");
pkgs.runCommand "obsidian-livesync" { inherit wrapper; } ''
  # ラッパーを build する(writeShellApplication が shellcheck を走らせる)だけでなく、
  # サブコマンドが閉じていること、印が無いと daemon が非 0 で止まることを実行で確かめる。
  rc=0; ${wrapper}/bin/obsidian-livesync >/dev/null 2>&1 || rc=$?
  test "$rc" -eq 2
  rc=0; ${wrapper}/bin/obsidian-livesync fetch >/dev/null 2>&1 || rc=$?
  test "$rc" -eq 2
  rc=0; ${wrapper}/bin/obsidian-livesync init extra >/dev/null 2>&1 || rc=$?
  test "$rc" -eq 2
  rc=0; ${wrapper}/bin/obsidian-livesync daemon 2>err || rc=$?
  test "$rc" -eq 1
  grep -q 'not initialised' err
  # 同期先が S3 互換だと、--interval が無い daemon は何も同期しない(2026-11-11 に実測)。
  grep -q -- 'daemon --interval 60$' ${wrapper}/bin/obsidian-livesync
  touch $out
''
