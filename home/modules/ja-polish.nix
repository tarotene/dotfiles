# ja-polish — 日本語の文書を `llm` 経由で Gemini に書き直させる(docs/agents/ja-polish.md)。
#
# 機体にも identity にも縛られない機構で、有効にするかは `dotfiles.jaPolish.enable` の 1 行で
# 決まる。今は個人の identity(home/identities/personal.nix)だけで立てている。理由は、送り先の
# Gemini の扱い(送った文面が学習に使われるか)を確かめ済みの経路が、個人の無料 API キーだけだから
# (無料枠は改善に使われうる契約で、個人の側はそれでよいと本人が裁定した)。会社の側は、会社契約の
# Claude Code から会社契約の Gemini へつなげられる見込みだが、Gemini 側の要件はまだ調べていない。
# 確かめたら、会社の identity で enable を立て、`package`(llm と送り先のプラグイン)と `model`
# を会社の契約に合わせる(docs/agents/ja-polish.md「横展開」)。
#
# このモジュールが置くもの:
#   - `ja-polish` 本体(PATH 上の ~/.local/bin。Claude の sandbox の外で動かす
#     → home/modules/claude.nix の excludedCommands)
#   - `llm` + `llm-gemini`(nix/ja-polish-llm.nix。nixpkgs の版では gemini-3.8-flash を呼べない)
#   - 設定ファイル ~/.config/dotfiles/ja-polish.toml。**このファイルの有無が、その機体で
#     ja-polish が有効かどうかの唯一の印**で、関所はこれが無ければ何も検査しない。
#
# API キーは置かない。各機体で一度、`llm keys set gemini` を手で実行する(キーは llm の
# 保存先 ~/.config/io.datasette.llm/keys.json に入り、そのディレクトリは
# agentPermissions.secret_paths で Claude の sandbox と Read から隠す)。
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.jaPolish;
in
{
  options.dotfiles.jaPolish = {
    enable = lib.mkEnableOption ''
      ja-polish: send Japanese prose to Gemini (through llm) for rewriting. Enable it only where
      the data handling of the destination has been checked: the default free-tier API key may
      use what is sent to improve Google's products
    '';
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.ja-polish-llm;
      defaultText = lib.literalExpression "pkgs.ja-polish-llm";
      description = ''
        The llm environment (llm and the plugin for the destination) that `ja-polish` calls. The
        default is llm with llm-gemini for Gemini through a Google AI Studio key
        (nix/ja-polish-llm.nix). Replace it to use another destination, for example the
        Gemini contract of an organization, once its data handling has been checked.
      '';
    };
    model = lib.mkOption {
      type = lib.types.str;
      default = "gemini-3.8-flash";
      description = ''
        The model name passed to `llm -m`. The newest general-purpose model that the free tier of
        the Gemini API allows (Google, "Gemini API Pricing", https://aistudio.google.com/docs/pricing,
        retrieved 2026-11-08). It must be one that the pinned llm-gemini (nix/ja-polish-llm.nix)
        registers: check with `llm models`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];
    dotfiles.binaries.".local/bin" = [
      "ja-polish"
    ];
    xdg.configFile."dotfiles/ja-polish.toml".text = ''
      model = "${cfg.model}"
    '';
  };
}
