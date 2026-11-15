# reply-coverage — 相手のいる会話への返信が、相手の未決の依頼すべてに答えているかを検査する
# (docs/agents/reply-coverage.md、決定は docs/adr/1585-reply-coverage.md)。
#
# このモジュールが置くもの:
#   - `reply-coverage` 本体(PATH 上の ~/.local/bin。`claude -p` を呼ぶのでネットワークと認証に届く必要があり、
#     Claude の sandbox の外で動かす → home/modules/claude.nix の excludedCommands、Codex は rules)
#   - 目印 ~/.config/dotfiles/reply-coverage。**このファイルの有無が、その機体で下書きの作成口の関所
#     (hook-router の `reply-coverage-gate`・`reply-thread-snapshot`)が働くかどうかの唯一の印**。
#     `dotfiles.host.name` の目印(home/modules/host.nix)と同じ型で、無ければ関所は何もしない。
#
# 送り先は、今のセッションと同じ Anthropic だけ(検出は `claude -p`)で、データの流れが新しく増えない。
# だから ja-polish(送り先が Google)と違い、機体・identity を限らず、既定で有効にする。
{ config, lib, ... }:
let
  cfg = config.dotfiles.replyCoverage;
in
{
  options.dotfiles.replyCoverage.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = ''
      reply-coverage: check that a reply draft answers every open ask of the counterpart
      (the whole conversation, not only the latest message). Installs the `reply-coverage` command and
      the marker that turns on the draft gate in hook-router. Detection calls `claude -p` (the same
      vendor as the agent session), so it is on by default everywhere.
    '';
  };

  config = lib.mkIf cfg.enable {
    dotfiles.binaries.".local/bin" = [
      "reply-coverage"
    ];
    xdg.configFile."dotfiles/reply-coverage".text = "enabled\n";
  };
}
