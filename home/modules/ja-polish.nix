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
      # 暫定: 3.7・3.8 は、無料枠のキーで応答が返らない(2026-11-12 に vega で `llm prompt -m …` を
      # 直接実行。3.8・3.7 は 60〜120 秒待っても無応答、ja-polish 経由では `high demand`。3.6 は即座に
      # 応答)。AI Studio の状態ページは All Systems Operational で、無料枠は「優先して切り捨てられる枠」と
      # 明記されている(https://aistudio.google.com/status、取得 2026-11-12)。
      # 戻す条件: `llm prompt -m gemini-3.8-flash -n --no-stream 'こんにちは'` が 30 秒以内に応答すること。
      default = "gemini-3.6-flash";
      description = ''
        The model name passed to `llm -m`. A general-purpose model that the free tier of the Gemini
        API allows (Google, "Gemini API Pricing", https://aistudio.google.com/docs/pricing,
        retrieved 2026-11-08) and that answers on the free tier right now (see the comment above
        for why it is not the newest one). It must be one that the pinned llm-gemini
        (nix/ja-polish-llm.nix) registers: check with `llm models`.
      '';
    };
    fallbackModel = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      # `crates/ja-polish/src/config.rs` の DEFAULT_FALLBACK_MODEL と同じにする。応答が速い軽いモデルで、
      # 2026-11-12 に vega の無料枠で `llm prompt -m gemini-flash-lite-latest` が即座に応答した
      # (docs/agents/ja-polish.md「モデルの選択」)。
      default = "gemini-flash-lite-latest";
      description = ''
        The model that ja-polish tries once when `model` is busy (retries exhausted) or times out.
        `null` turns the fallback off. The thinking level is not passed to it, because a fallback
        model does not necessarily offer the same choices.
      '';
    };
    thinkingLevel = lib.mkOption {
      type = lib.types.nullOr (
        lib.types.enum [
          "minimal"
          "low"
          "medium"
          "high"
        ]
      );
      # 既定は null(モデルの既定に任せる。gemini-3.6-flash は medium)。値は、固定の見本で応答時間と
      # 書き直しの質を測って決める(docs/adr/1387-ja-polish.md の D11)。
      default = null;
      description = ''
        The thinking level passed to the main model (`llm -o thinking_level`; llm-gemini 0.35
        accepts these four for gemini-3.6-flash). Lower is faster. `null` leaves it to the model.
      '';
    };
    timeoutSecs = lib.mkOption {
      type = lib.types.ints.positive;
      default = 45;
      description = ''
        Seconds to wait for one request. A model that does not answer within it counts as timed
        out and ja-polish moves on to the fallback model instead of waiting again.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];
    dotfiles.binaries.".local/bin" = [
      "ja-polish"
    ];
    # 空の `fallback_model` は「予備を使わない」(キーを省くと、ja-polish の既定の予備を使う)。
    xdg.configFile."dotfiles/ja-polish.toml".text = ''
      model = "${cfg.model}"
      fallback_model = "${lib.optionalString (cfg.fallbackModel != null) cfg.fallbackModel}"
      timeout_secs = ${toString cfg.timeoutSecs}
    ''
    + lib.optionalString (cfg.thinkingLevel != null) ''
      thinking_level = "${cfg.thinkingLevel}"
    '';
  };
}
