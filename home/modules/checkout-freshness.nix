# git-checkout-freshness タイマーの対象に足す親チェックアウトの口(#78、
# docs/claude/git-checkout-freshness.md)。許可リストは明示リストで、この
# public リポジトリが固定で持つのは dotfiles 自身だけ。他の対象(私的な
# リポジトリのパス)は、private wrapper flake が
# `dotfiles.checkoutFreshness.extraPaths = [ "<絶対パス>" ];` として注入する
# (ADR-0034「規則は public・実値は private」)。`home/modules/worktree.nix` が
# 読む。option の宣言を worktree.nix の外に置くのは、worktree.nix が
# `config` で包まれていない素の attrset で、option を足すと本体全体を包み直す
# 差分になるため。
{ lib, ... }:
{
  options.dotfiles.checkoutFreshness.extraPaths = lib.mkOption {
    type = lib.types.listOf lib.types.str;
    default = [ ];
    description = ''
      git-checkout-freshness が default branch へ fast-forward する親
      チェックアウトの絶対パス(dotfiles 自身に加えて)。安全条件(clean・
      ahead 0・default branch 上)を満たさない repo は黙って skip される。
    '';
  };
}
