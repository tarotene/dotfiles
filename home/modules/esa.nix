# esa.io MCP サーバのトークン供給(ADR-0022)。
#
# `~/.claude.json` の `.mcpServers.esa` は従来
# `env.ESA_ACCESS_TOKEN = "${ESA_ACCESS_TOKEN}"` という記法を使っていた。この
# `${VAR}` 記法自体は Claude Code がセッション起動時の環境変数で正しく展開する
# (公式ドキュメント確認済み — 展開されないという誤解が ADR-0022 初稿にあった)。
# 実際の不具合は、供給元だった別の private リポジトリ(SOPS + direnv、実質
# シークレット1個のための器)の外では `ESA_ACCESS_TOKEN` という環境変数自体が
# 一度も存在しなかったこと。この変数をログインシェル起動時に毎回復号・export
# する形は ADR-0010 が明示的に退役させたパターン(シェル起動時の GPG PIN
# プロンプト)そのものなので採らない。代わりに、MCP サーバー起動というただ一点
# でだけ復号する専用 launcher(`scripts/esa-mcp-launcher`)を command に据え、
# env 展開自体を使わない。
#
# `~/.claude.json` への登録は `home/modules/claude-mcp-servers.nix` が公開する
# 共通の「口」(`dotfiles.claude.mcpServers`)に populate するだけでよい —
# `~/.claude.json` が Claude-Code-owned で store symlink にできない制約への
# 対処(冪等 jq merge)はそちらが一元的に持つため、このモジュール自身は
# activation スクリプトを持たない。
#
# このモジュールは `home/identities/personal.nix` からだけ import される —
# トークンの宛先は personal identity の master fingerprint(GnuPG が YubiKey 上の
# [E] サブ鍵に解決する、ADR-0003 Amendment)で、company ホストのカードでは
# 復号できないため、company に配ると毎セッション必ず失敗する MCP 登録だけが
# 残る。詳細は docs/claude/esa-mcp.md、手動プロビジョニングは docs/setup.md。
{ config, ... }:
let
  launcherPath = "${config.home.homeDirectory}/.local/libexec/esa-mcp-launcher";
in
{
  home.file.".local/libexec/esa-mcp-launcher" = {
    source = ../../scripts/esa-mcp-launcher;
    executable = true;
  };

  # LANG=ja は旧 private リポジトリの .mcp.json が持っていた実績値をそのまま
  # 引き継ぐ。
  dotfiles.claude.mcpServers.esa = {
    type = "stdio";
    command = launcherPath;
    args = [ ];
    env.LANG = "ja";
  };
}
