# private wrapper flake の marker(#567、ADR-0034 Amendment)。
#
# `scripts/hms.sh` の `resolve_default_ref()` は
# `~/.config/dotfiles/private-hub` を読んで、この host に private wrapper
# flake が登録されているかを判定する。ADR-0034 の元の設計はこのマーカーを
# 「home-manager が書かない手置きファイル」としていた — dotfiles(この
# public リポジトリ)が値を宣言すると、wrapper の識別子(URL・パス・名前)
# が public リポジトリの管理対象ファイルに書き込まれることになり、ADR-0034
# 「規則は public・実値は private」に反するため。
#
# しかし実際には、この手置き設計に構造的な欠陥があった(#567、2026-09-29
# 実測): private wrapper flake のローカル checkout を明示パスで `hms`
# 適用しても、それだけではマーカーは設置されない。マーカー無しのまま
# 次に素の `hms`(引数無し)を実行すると、`resolve_default_ref()` は
# 無警告で `DEFAULT_REF`(public 単体)に縮退し、wrapper が配ったファイル
# (bleep の denylist config 等、ADR-0034 Amendment 2026-09-24)が
# home-manager の orphan cleanup で撤去される。
#
# 解決: マーカーの値そのものは今も public リポジトリに書かない(このモジュール
# は口だけを公開する — `dotfiles.detectDrift.issueRepo`(drift.nix)と同じ
# 「口だけを公開する extensible option」の型)。実値は private wrapper flake の
# extraModules が `dotfiles.privateHub.ref = "<実際の flake ref>";` として
# 注入する。これにより「wrapper を適用した」という事実そのものが、この
# マーカーの設置を home-manager 活性化の一部として自動的に伴うようになり、
# 手動でのマーカー設置忘れという中間状態が構造的に起きなくなる。
{
  config,
  lib,
  ...
}:
let
  cfg = config.dotfiles.privateHub;
in
{
  options.dotfiles.privateHub.ref = lib.mkOption {
    type = lib.types.nullOr lib.types.str;
    default = null;
    description = ''
      `hms` が既定で適用する private wrapper flake の参照(`resolve_default_ref()`
      が読む `~/.config/dotfiles/private-hub` マーカーの内容)。null のときは
      このモジュールは何も配備しない(wrapper 未登録ホスト向けの既定)。
      実値は private wrapper flake の extraModules が注入する — この
      public リポジトリのソースツリー自体は一切値を持たない(ADR-0034)。
    '';
  };

  config = lib.mkIf (cfg.ref != null) {
    xdg.configFile."dotfiles/private-hub".text = cfg.ref + "\n";
  };
}
