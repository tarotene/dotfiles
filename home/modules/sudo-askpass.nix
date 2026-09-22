# sudo パスワードを pinentry 経由で渡す — SUDO_ASKPASS ラッパー。
#
# 背景: Claude Code(および Codex / Copilot CLI)の Bash ツール実行には制御端末
# がない。sudo(8) は「端末がなく SUDO_ASKPASS が設定されている」場合に限り
# `-A` なしで自動的に askpass helper を使う(sudo.ws のマニュアル、2026-09-22
# 参照)ため、この変数を宣言するだけで agent セッション(tty なし)だけが
# askpass 経路になり、対話端末(tty あり)は従来どおり端末プロンプトのまま
# 切り替わる — sudo 自身の tty 判定に委ねているので、この宣言側で分岐する
# 必要はない。
#
# helper の実体は scripts/sudo-askpass。gpg.nix が選んだ pinentry パッケージ
# (Linux: pinentry-gnome3 / darwin: pinentry_mac、ADR-0018)をそのまま再利用し、
# 生の Assuan プロトコルで GETPIN を発行する。GPG と同じ入力面・同じ grab 挙動
# に揃えるためで、apt に ssh-askpass 系を別途足すことはしない。
#
# 意図的にキャッシュしない(sign-prewarm.sh とは対照的): sudo の askpass 承認
# は「どのコマンドが root で実行されるか」を GUI 上で示せない(sudo が helper
# に渡すのは汎用のプロンプト文字列のみ)。コマンド内容を目視できる唯一の関所は
# Claude 自身の Bash permission ダイアログなので、claude.nix の
# permissionRules には sudo 系を一切足さず default ask のままにしてある —
# askpass 側の入力とあわせて二段の承認にする設計。
#
# 既知の注意点(docs/claude/sudo-askpass.md に詳細): このホストでは pinentry
# 自体が人間の操作なしに既存の秘密情報を返す経路が過去に確認されている
# (GNOME login keyring の自動アンロックが疑われる、調査中)。この機構は
# pinentry が「人間の入力を要求している」ことを技術的に保証しない前提で
# 設計している。
{
  config,
  lib,
  pkgs,
  ...
}:
let
  repoRoot = ../..;
  # home-manager's own gpg-agent module already resolves the right binary name
  # per pinentry package (pinentry.program defaults to
  # `pinentry.package.meta.mainProgram or "pinentry"` — see nix-community/
  # home-manager modules/services/gpg-agent.nix). Reusing it here means no
  # isDarwin branch is needed for the binary name itself: pinentry-gnome3's
  # mainProgram is "pinentry", pinentry_mac's is "pinentry-mac", both resolved
  # generically.
  pinentryBin = lib.getExe' config.services.gpg-agent.pinentry.package config.services.gpg-agent.pinentry.program;
  askpassPath = "${config.home.homeDirectory}/.local/libexec/sudo-askpass";
in
{
  home.file.".local/libexec/sudo-askpass" = {
    source = pkgs.replaceVars (repoRoot + "/scripts/sudo-askpass") {
      pinentry = pinentryBin;
    };
    executable = true;
  };

  # GUI 起動(herdr ペイン等)とシェル起動の両方に届かせるための二重宣言
  # (shell.nix の SHELL 宣言と同じ理由)。systemd --user は Linux のみ。
  home.sessionVariables.SUDO_ASKPASS = askpassPath;
  systemd.user.sessionVariables = lib.mkIf pkgs.stdenv.isLinux { SUDO_ASKPASS = askpassPath; };
}
