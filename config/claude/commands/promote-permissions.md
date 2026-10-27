# Propose a native permission change

`/promote-permissions` reviews permissions against each agent's native settings.
Claude and Codex have separate declarations; the shared ownership policy and
session-boundary promotion queue are retired (ADR-457, 2026-10-27 Amendment).

1. Read the relevant native config and declarations without exposing private
   paths or server values. Claude's rules live in `home/modules/claude.nix`;
   Codex's declared settings live in `home/modules/codex.nix`. Use
   `dotfiles.claudePermissions.extraAllow` and
   `dotfiles.codexPermissions.{secretPaths,domains,mcpAllow,apps}` for scoped inputs.
   Private values belong in the private wrapper, following ADR-0034.
2. Explain the exact proposed rule, its scope, and whether it expands access.
   Present expansions to the owner before applying them. Do not translate a
   Claude Bash rule into a Codex prefix rule or infer safety from a tool name.
3. Prepare an approved declaration change in a dotfiles worktree and follow its
   normal PR process. Keep the two native policies independently meaningful.
   MCP Apps and custom MCP servers have different native approval paths; check
   the pinned source before proposing a permanent allowance.
4. After applying with `hms .`, read back each affected native setting. A saved
   config is evidence of configuration only; report reviewer execution as
   unverified unless a fresh session actually exercised it.

Default behavior is a proposal only. Do not delete project settings, replay an
old promotion queue, or run the retired shared `inspect` / `migrate` / `check`
workflow as a prerequisite for ordinary tool execution. For removal of old
shared changes, follow `agent-permissions retire --home PATH [--dry-run]` in
`docs/agents/agent-permissions.md`; it preserves changes without proven ownership.
