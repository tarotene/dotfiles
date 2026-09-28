# apt(system 層)の host 固有 escape hatch(#3)。宣言ファイル自体
# (`packages/declarative/apt-packages.txt`)は全 Linux ホスト共通のまま
# 変更しない — vega だけに要る system76/virt-manager/lightdm 等をそこへ
# 混ぜると、`scripts/install-packages.sh` が arcturus にもインストール
# しようとしてしまう。host module がこのモジュールの
# `dotfiles.apt.extraPackages` へ list で足すと、home-manager が共通
# ファイルと連結して `~/.config/dotfiles/apt-packages.txt` に配備する。
# `detect-drift`(`crates/detect-drift`)と `scripts/install-packages.sh`
# は共にこの配備先を読む単一正本にする(3段解決の1段目、
# `apply-rulesets.sh` #417 と同じ「配備先 > checkout 相対」の型)。
#
# `dotfiles.detectDrift.issueRepo`(drift.nix)と同じ「口だけを公開する
# extensible option」型 — この dotfiles(public)のソースツリー自体は
# host 固有の値を持たず、host module(vega.nix 等、既に署名鍵等の実値を
# 持つ)が埋める。
{
  config,
  lib,
  ...
}:
let
  cfg = config.dotfiles.apt;
  sharedText = builtins.readFile ../../packages/declarative/apt-packages.txt;
  extraText =
    if cfg.extraPackages == [ ] then
      ""
    else
      "\n# --- Host-specific escape hatch (dotfiles.apt.extraPackages, host module) ---\n"
      + lib.concatMapStrings (line: line + "\n") cfg.extraPackages;
in
{
  options.dotfiles.apt.extraPackages = lib.mkOption {
    type = lib.types.listOf lib.types.str;
    default = [ ];
    description = ''
      This host's additions to the apt system layer, appended after the
      shared `packages/declarative/apt-packages.txt`. A host module (e.g.
      `home/hosts/vega.nix`) sets this for packages that genuinely need
      root, a system service, or a driver on THAT host only — putting them
      in the shared file would make `scripts/install-packages.sh` try to
      install them on every other Linux host too.

      Each list entry is one line of the final apt-packages.txt: a `#`
      comment line explaining why the next package can't be home-manager
      (dotfiles#4's classification rule), or a bare package name. Blank
      strings are allowed as spacers between reasoned groups.
    '';
  };

  config = {
    xdg.configFile."dotfiles/apt-packages.txt".text = sharedText + extraText;
  };
}
