# Propose a shared permission-policy change

`/promote-permissions` inventories existing permissions and prepares a change to
`home/modules/agent-permissions.nix` (private values belong in the private wrapper).
It never adds permanent local allow rules or deletes project settings.

## Promotion queue

Allow rules saved by "don't ask again" are tolerated for the session and moved to a
promotion queue at the next session boundary (docs/agent-permissions.md). Start here:

```sh
agent-permissions pending --json   # candidates: id, rule, project
```

Each candidate gets one decision from the owner, never decided for them:

- `WebFetch(domain:…)` can become a policy declaration: `domains` (every project) or
  `project_rules.<project>.domains` (one project), in the shared policy
  (`home/modules/agent-permissions.nix`; a private host value goes in the private wrapper).
- `mcp__…`, `WebSearch` and `Bash(…)` have no declaration in the shared policy: tools stay
  `prompt`. Discard them, or open an Issue to extend the policy schema. Do not save them as
  permanent allow rules.
- After the owner decides, and after a promotion PR exists, withdraw the decided candidates:
  `agent-permissions pending --drop ID,ID`. An undecided candidate stays in the queue.

## Procedure

1. Run `agent-permissions inspect --json` with the current project supplied by
   `--project`. Keep the report private: rule text can include private paths.
2. Describe each proposed change as retirement, shared-source retention, or a
   policy declaration. Unknown syntax stays in review; do not infer its meaning
   from a command prefix. A shared file is never a migration write target.
3. Compare the proposed boundary with the current one. Present any expansion to
   the owner before applying it. Keep routine individual approvals limited to
   that execution; do not recommend saving a Bash/prefix allow rule.
4. Prepare a reviewable change in a dotfiles worktree, following its normal PR
   process. Use the shared policy for both agents; do not translate Claude Bash
   allow strings into Codex allow rules. Native behavior that cannot represent a
   scope goes through individual review.
5. For an actual migration, show the complete `inspect` report and proposed
   dispositions first. Run `agent-permissions migrate --dry-run --approved REVIEW`
   to check the current hashes without writing. Only after the owner confirms
   that exact retirement list, run the same command without `--dry-run` and read
   back with `agent-permissions check`.

`--dry-run` explicitly requests a proposal only, which is also the default.
The old `--delete-after` and `--keep-project-specific` migration options are
retired; report that change instead of deleting or rewriting shared settings.

For interrupted migration or invalid ownership records, follow the dedicated
CLI recovery procedure in `docs/agent-permissions.md`. Do not bypass the runtime
permission check with an arbitrary shell command.
