# Codex keeps its writable config; reconcile only the declared keys (ADR-457).
{
  config,
  lib,
  pkgs,
  ...
}:
let
  settingsReconcile = "${pkgs.dotfiles-tools}/bin/settings-reconcile";

  # Shared permission declarations own the sandbox and review policy.
  # This writer keeps only the unrelated terminal display preference.
  # OpenAI, Codex rust-v0.164.0 (retrieved 2026-10-26):
  # https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/protocol/src/config_types.rs#L175-L189
  # https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/tui/src/chatwidget/settings.rs#L751-L768
  # Codex 自身が端末タイトル(OSC)に描く項目。herdr はこれを pane の
  # `terminal_title_stripped` として受け取り、サイドバーの Codex 行 4 行目にそのまま
  # 出す(config/herdr/config.toml)。項目 ID は上流の `TerminalTitleItem`
  # (openai/codex、codex-rs/tui/src/bottom_pane/title_setup.rs)で、改名・削除は
  # 上流で破壊的変更として扱われる。取得 2026-10-20、rust-v0.160.0。
  #   activity         — 動作中のスピナー / 承認待ちの表示
  #   context-used     — context window の使用率
  #   five-hour-limit  — 5h rate limit の残り
  # サイドバーに見えるのは約 36 桁(`Context 0% used | medium | 5h ...` で切れた
  # 実機の表示からの推定、2026-10-20。幅そのものは herdr の API から取れていない)。
  # Codex が組む文字列は、context と 5h だけなら `Context 0% used | 5h 100% left`
  # (31 文字)で収まり、reasoning と weekly-limit を足すと 58 文字で 5h・週の値が
  # 切れる。上流の言い回し
  # (`Context … used` / `… left`)は短くできないので、項目の数で収める。
  # project-name は workspace 列と重なるので外している。
  codexConfigSpec = builtins.toJSON {
    "tui.terminal_title" = [
      "activity"
      "context-used"
      "five-hour-limit"
    ];
  };

in
{
  imports = [ ./agent-permissions.nix ];
  dotfiles.agentPermissions.enable = lib.mkDefault true;

  # Permission migration requires an explicitly reviewed snapshot. A first
  # activation deploys the policy and CLI, but never invents migration approval;
  # the router refuses tools until the owner-reviewed migration is complete.
  home.activation.reconcileAgentPermissions = lib.mkIf config.dotfiles.agentPermissions.enable (
    lib.hm.dag.entryAfter
      [
        "writeBoundary"
        "registerClaudeHooks"
        "registerClaudeStatusLine"
        "registerClaudeModelConfig"
        "reconcileCodexConfig"
      ]
      ''
        # pipeline-node: settings-reconcile
        if [ -f "$HOME/.local/state/dotfiles/agent-permissions.json" ]; then
          run ${pkgs.dotfiles-tools}/bin/agent-permissions apply \
            --policy ${config.dotfiles.agentPermissions.policyFile} --home "$HOME"
        else
          echo 'agent-permissions: migration review required; tool execution is blocked until migration.' >&2
          echo 'Run agent-permissions inspect --json, review each disposition, then migrate --approved REVIEW.' >&2
        fi
      ''
  );

  # A single writer preserves Codex-owned trust, model selection, and formatting.
  # TUI overrides last until the next switch; new sessions load the declaration.
  home.activation.reconcileCodexConfig = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    # pipeline-node: settings-reconcile
    run ${settingsReconcile} codex-config "$HOME/.codex/config.toml" ${lib.escapeShellArg codexConfigSpec}
  '';
}
