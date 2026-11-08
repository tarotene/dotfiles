# ja-polish — 日本語の文書を `llm` 経由で Gemini に書き直させる(docs/agents/ja-polish.md)。
#
# 有効にするのは個人の identity だけ(home/identities/personal.nix)。無料の API キーは、送った
# 文面がモデルの改善に使われうる契約なので、会社の機体には道具も設定も配らない(クラウドの
# AI 道具は personal の identity にだけ置く方針、#9 と同じ)。会社の Gemini 契約への接続は
# 別の作業(docs/agents/ja-polish.md「後続」)。
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
      ja-polish: send Japanese prose to Gemini (through llm, free-tier API key) for rewriting.
      Personal identity only; the free tier may use what is sent to improve Google's products
    '';
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
    home.packages = [ pkgs.ja-polish-llm ];
    home.file.".local/bin/ja-polish".source = "${pkgs.dotfiles-tools}/bin/ja-polish";
    xdg.configFile."dotfiles/ja-polish.toml".text = ''
      model = "${cfg.model}"
    '';
  };
}
