# esa-index(SessionStart hook、docs/claude/esa-index.md)の設定の口。
#
# 社内 wiki(esa)のローカルミラーのパスと、hook を発火させる GitHub org は
# 私的な値なので、このソースツリーは一切持たない(ADR-0034「規則は public・
# 実値は private」)。値は private wrapper flake の extraModules が
# `dotfiles.esaIndex.mirrorPath` / `dotfiles.esaIndex.orgs` として注入する。
# `dotfiles.detectDrift.issueRepo`(drift.nix)と同じ「口だけを公開する
# extensible option」の型。どちらかが未設定なら何も配備せず、hook は
# 設定ファイルが無いので無音で終わる。
#
# なお home/modules/esa.nix は別物(個人の esa MCP のトークン供給、ADR-0022)。
{ config, lib, ... }:
let
  cfg = config.dotfiles.esaIndex;
in
{
  options.dotfiles.esaIndex = {
    mirrorPath = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        esa の全記事を日次で mirror した git clone の絶対パス。null のとき
        esa-index は発火しない。実値は private wrapper flake が注入する。
      '';
    };
    orgs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = ''
        esa-index を発火させる GitHub owner(cwd の `remote.origin.url` の
        owner と大文字小文字を無視して一致するもの)。空のとき発火しない。
      '';
    };
  };

  config = lib.mkIf (cfg.mirrorPath != null && cfg.orgs != [ ]) {
    xdg.configFile."esa-index/config.json".text =
      builtins.toJSON {
        mirror_path = cfg.mirrorPath;
        inherit (cfg) orgs;
      }
      + "\n";
  };
}
