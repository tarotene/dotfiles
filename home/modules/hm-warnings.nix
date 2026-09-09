# home-manager 自体が apply 時(hms/home-manager switch)に出す活性化ノイズの
# 抑止だけを束ねる。三点とも upstream で未解決/意図的に revert 済みの警告で
# あり、根絶にはローカルでの上書きしかない(docs/README.md 参照)。
{ config, lib, ... }:
{
  # W1: `warning: Using 'builtins.derivation' to create a derivation named
  # 'options.json' ... without a proper context.`
  # home-manager manual(manpages 生成)が nixpkgs make-options-doc 経由で
  # context の落ちた store path を参照し、Determinate Nix だけがこれに
  # 警告する。nixpkgs 側は「Determinate Nix が警告すべきでない」として
  # 修正を拒否済み(NixOS/nixpkgs#482766, closed)。home-manager 側は
  # 未解決(nix-community/home-manager#7935, open、取得 2026-09-09
  # https://github.com/nix-community/home-manager/issues/7935)。
  # manpages を切れば docs derivation ごと eval から消え、警告も消える。
  # 失うのは `man home-configuration.nix`
  # (代替: https://nix-community.github.io/home-manager/options.xhtml)。
  # upstream か Determinate Nix 側が直したらこの行を削除する。
  manual.manpages.enable = false;

  # W3: `There are N unread and relevant news items.`
  # 既読化の仕組みがなく、switch のたびに件数が増え続ける。読みたいときは
  # 手動で `home-manager news` を叩けばよい。
  news.display = "silent";

  # W2: `warning: 'install' is a deprecated alias for 'add'`
  # home-manager の installPackages activation
  # (modules/home-environment.nix, 固定 rev d4fd246)が
  # `nix profile install` を叩く。新しめの Nix では `install` は
  # `add` の deprecated alias。upstream は一度 `add` に修正したが
  # (nix-community/home-manager#8756)、Lix に `nix profile add` が無く
  # activation が壊れるため revert 済み(#8835)。#9598(open、取得
  # 2026-09-09 https://github.com/nix-community/home-manager/issues/9598)
  # は未解決で、flake update では直らない。
  # 全ホストは bootstrap.sh で Determinate Nix を入れる前提(Lix は使わない)
  # なので、ここでは `add` に固定して安全に警告を消す。中身は
  # home-environment.nix の installPackages(standalone 用の else 分岐)を
  # install → add だけ変えた写し。upstream #9598 が解決したらこの
  # mkForce を削除する。
  home.activation.installPackages = lib.mkForce (
    lib.hm.dag.entryAfter [ "writeBoundary" ] ''
      function nixReplaceProfile() {
        local oldNix="$(command -v nix)"

        nixProfileRemove 'home-manager-path'

        run $oldNix profile add $1
      }

      if [[ -e ${config.home.profileDirectory}/manifest.json ]] ; then
        INSTALL_CMD="nix profile add"
        INSTALL_CMD_ACTUAL="nixReplaceProfile"
        LIST_CMD="nix profile list"
        REMOVE_CMD_SYNTAX='nix profile remove {number | store path}'
      else
        INSTALL_CMD="nix-env -i"
        INSTALL_CMD_ACTUAL="run nix-env -i"
        LIST_CMD="nix-env -q"
        REMOVE_CMD_SYNTAX='nix-env -e {package name}'
      fi

      if ! $INSTALL_CMD_ACTUAL ${config.home.path} ; then
        echo
        _iError $'Oops, Nix failed to install your new Home Manager profile!\n\nPerhaps there is a conflict with a package that was installed using\n"%s"? Try running\n\n    %s\n\nand if there is a conflicting package you can remove it with\n\n    %s\n\nThen try activating your Home Manager configuration again.' "$INSTALL_CMD" "$LIST_CMD" "$REMOVE_CMD_SYNTAX"
        exit 1
      fi
      unset -f nixReplaceProfile
      unset INSTALL_CMD INSTALL_CMD_ACTUAL LIST_CMD REMOVE_CMD_SYNTAX
    ''
  );
}
