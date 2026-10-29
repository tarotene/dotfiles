# コーディングエージェントの一時成果物(scratchpad)を、再起動を越えて 14 日残す
# (darwin 側)。Linux は /tmp の規則そのものを替える(config/tmpfiles/tmp.conf、
# scripts/install-packages.sh)ので、ここでは何もしない — 同じ保証を二重に持たない。
#
# 動機: エージェントは一次成果物(計画・下書き・出力)を /tmp に書く癖があり、再起動や
# 電源断で失われる。Claude Code の scratchpad は
# `/tmp/claude-<uid>/<project>/<session>/scratchpad/` で、環境変数
# `CLAUDE_CODE_TMPDIR` を設定すると木ごと移る(Anthropic, "Explore the .claude
# directory", https://code.claude.com/docs/en/claude-directory.md、取得 2026-10-29)。
# 実害の報告: anthropics/claude-code#92166(取得 2026-10-29)。
#
# macOS の /tmp は再起動で消える(claude-code#88773・#97759 の報告)が、消している
# 部品も止め方も一次資料で確かめていない。そこで /tmp には触らず、移した先の
# `~/.local/state/claude-tmp` を、~/Downloads と同じ launchd + /usr/bin/find の形
# (home/modules/downloads.nix)で 14 日掃除する。
#
# 未確認(後続 Issue で altair の実機に当てて読む): sandbox の中から移した先に
# 書けるか、再起動後も残るか。`/tmp` へ直接書かれた分は、scripts/install-packages-darwin.sh
# が periodic の日数を 14 日にそろえるだけで、起動時の消去は残る。
{
  config,
  lib,
  pkgs,
  ...
}:
let
  scratchDir = "${config.xdg.stateHome}/claude-tmp";
in
{
  config = lib.mkIf pkgs.stdenv.isDarwin {
    home.sessionVariables.CLAUDE_CODE_TMPDIR = scratchDir;

    # find は存在しない道具の対象を掃除できず毎日エラーになるので、先に作る。
    home.activation.claudeTmpDir = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
      run mkdir -p ${lib.escapeShellArg scratchDir}
    '';

    # downloads.nix と同じ。削除条件は tmpfiles の既定(3 種のタイムスタンプのいずれか
    # 新しければ延命)を find の論理積(AND)で写す。TCC を避けるため、Apple 署名済みで
    # パスが安定する /usr/bin/find を直接呼ぶ。
    dotfiles.pipeline.timerNodes.claude-tmp-clean = "claude-tmp-clean";
    launchd.agents.claude-tmp-clean = {
      enable = true;
      config = {
        ProgramArguments = [
          "/usr/bin/find"
          scratchDir
          "-mindepth"
          "1"
          "-atime"
          "+14"
          "-mtime"
          "+14"
          "-ctime"
          "+14"
          "-delete"
        ];
        StartInterval = 86400; # 1d
        RunAtLoad = true;
      };
    };
  };
}
