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
      # 有料枠のキーを前提にした既定(docs/adr/1387-ja-polish.md の D19)。無料枠では、3.8 は混雑で応答が
      # 返らず(2026-11-12)、モデルごとの 1 日の上限(20 回)のあと、予備の軽量モデルに流れて、直すべき日本語が
      # 直らないまま通った(D17)。固定の見本の測定(2026-11-28、`ja-polish bench`)で、3.8 + 思考量 low が、
      # 取りこぼしが少なく、時間切れも少なかった。無料枠のキーに戻すときは、3.6 に替える。
      default = "gemini-3.8-flash";
      description = ''
        The model name passed to `llm -m`. The default assumes a paid-tier Gemini API key: the free tier
        has a per-day request limit per model, and after it the requests flow to the fallback model.
        It must be one that the pinned llm-gemini (nix/ja-polish-llm.nix) registers: check with
        `llm models`.
      '';
    };
    fallbackModel = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      # `crates/ja-polish/src/config.rs` の DEFAULT_FALLBACK_MODEL と同じにする。軽量モデルは、何も直さずに
      # 返す劣化が起きたので外した(D17)。測定(2026-11-28)で、直した段落が多く、時間切れが 0 だった
      # gemini-4-argon にした。単価は高いが、予備は主が混雑・時間切れのときだけ使う(D19)。
      default = "gemini-4-argon";
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
      # `crates/ja-polish/src/config.rs` の DEFAULT_THINKING_LEVEL と同じにする。gemini-3.8-flash は、既定の
      # 思考量だと 45 秒の時間切れが多発し(測定 2026-11-28。素通し 37 のうち 32)、low は 102 秒で終わった(D19)。
      default = "low";
      description = ''
        The thinking level passed to the main model (`llm -o thinking_level`; llm-gemini 0.35
        accepts these four). Lower is faster. `null` leaves it to the model.
      '';
    };
    textlint = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Run textlint's autofix before and after the model (ADR-1387 D13). It rewrites boilerplate
          such as "することができる。" deterministically, so the fix stays even when the model is busy.
          Without a `textlint` command on PATH, ja-polish skips it silently.
        '';
      };
      package = lib.mkOption {
        type = lib.types.package;
        # `textlint.withPackages` は、規則のパッケージを NODE_PATH に束ねる(nixpkgs の passthru)。
        # preset-ja-technical-writing のうち有効にするのは `ja-no-redundant-expression` だけで、
        # ほかの規則は ja-polish が実行のたびに作る設定(crates/ja-polish/src/textlint.rs)で無効にする。
        # prh は、private の文体規約の隣の `docs/style/prh.yml` があるときだけ使われる。
        default = pkgs.textlint.withPackages [
          pkgs.textlint-rule-preset-ja-technical-writing
          pkgs.textlint-rule-prh
        ];
        defaultText = lib.literalExpression "pkgs.textlint.withPackages [ pkgs.textlint-rule-preset-ja-technical-writing pkgs.textlint-rule-prh ]";
        description = "The textlint environment (textlint and its rule packages) that ja-polish calls.";
      };
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
    home.packages = [ cfg.package ] ++ lib.optional cfg.textlint.enable cfg.textlint.package;
    dotfiles.binaries.".local/bin" = [
      "ja-polish"
    ];
    # 空の `fallback_model` は「予備を使わない」(キーを省くと、ja-polish の既定の予備を使う)。
    xdg.configFile."dotfiles/ja-polish.toml".text = ''
      model = "${cfg.model}"
      fallback_model = "${lib.optionalString (cfg.fallbackModel != null) cfg.fallbackModel}"
      timeout_secs = ${toString cfg.timeoutSecs}
      textlint = ${lib.boolToString cfg.textlint.enable}
    ''
    # 空の `thinking_level` は「モデルの既定に任せる」(キーを省くと、ja-polish の既定の思考量を使う)。
    + ''
      thinking_level = "${lib.optionalString (cfg.thinkingLevel != null) cfg.thinkingLevel}"
    '';
  };
}
