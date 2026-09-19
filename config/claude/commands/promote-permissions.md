# Promote Permissions Command Specification

## Command Configuration
```yaml
custom_commands:
  promote-permissions:
    description: "Migrate generic permissions from project .claude/settings.local.json to user-side ~/.claude/settings.json"
```

## RFC 2119 Compliance

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this command specification are to be interpreted as described in RFC 2119.

## Command Behavior Requirements

This command MUST actively interact with Claude Code settings to:
1. **Read Project Permissions** - Parse `.claude/settings.local.json` from current project
2. **Categorize Permissions** - Separate generic/reusable from project-specific patterns using subsumption-aware logic
3. **Merge to User Config** - Add generic permissions to `~/.claude/settings.json` (with subsumption check + wildcard collapsing)
4. **Delete Project Config** - Delete the project settings file by default (no flag required)
5. **Stale Global Cleanup** - Remove entries from user global that reference stale/renamed project paths
6. **Migration Report** - Display categorized summary after applying changes

## Required Usage Patterns

Claude MUST support the following usage patterns:
- `/promote-permissions` - Apply directly after backup (default: no confirmation needed)
- `/promote-permissions --dry-run` - Show what would be migrated without changes
- `/promote-permissions --keep-project-specific` - Preserve project-specific permissions in project config instead of deleting

## Permission Categorization Logic

When analyzing permissions, Claude MUST apply classification using this framework:

```typescript
interface PermissionClassification {
  generic: string[];      // Reusable across projects
  projectSpecific: string[];  // Tied to this project only
}

function categorizePermissions(permissions: string[]): PermissionClassification {
  const generic: string[] = [];
  const projectSpecific: string[] = [];

  for (const perm of permissions) {
    if (isGenericPermission(perm)) {
      generic.push(perm);
    } else {
      projectSpecific.push(perm);
    }
  }

  return { generic, projectSpecific };
}

function isGenericPermission(perm: string): boolean {
  // Universal patterns
  if (perm === "WebSearch") return true;
  if (/^WebFetch\(domain:(github\.com|pypi\.org|crates\.io)\)$/.test(perm)) return true;

  // MCP patterns
  if (/^mcp__github__/.test(perm)) return true;

  // Git operations
  if (/^Bash\(git (checkout|add|commit|push|fetch|rebase|rm|check-ignore|log|status|diff):/.test(perm)) return true;

  // GitHub CLI
  if (/^Bash\(gh (pr|issue|label) /.test(perm)) return true;

  // Shell utilities
  if (/^Bash\((find|cat|grep|xargs|echo|env)(:|\))/.test(perm)) return true;
  if (/^Bash\(grep /.test(perm)) return true;

  // System process/port utilities
  if (/^Bash\(lsof /.test(perm)) return true;
  if (/^Bash\(ps -p /.test(perm)) return true;
  if (/^Bash\(kill /.test(perm)) return true;
  if (/^Bash\(tee \/tmp\//.test(perm)) return true;
  if (/^Bash\(\[ -f /.test(perm)) return true;

  // Rust toolchain (generic across all Rust projects)
  if (/^Bash\(rustup/.test(perm)) return true;
  if (/^Bash\(rustc /.test(perm)) return true;
  if (/^Bash\(cargo /.test(perm)) return true;

  // Package registries
  if (/^Bash\(curl -s https:\/\/crates\.io\//.test(perm)) return true;

  // NPM/Playwright generic (without hardcoded project paths)
  if (/^Bash\(npx --prefix \* playwright /.test(perm)) return true;

  // GPG signing utilities
  if (/^Bash\(gpg(conf)? /.test(perm)) return true;
  if (/^Bash\(gpg-connect-agent /.test(perm)) return true;

  // MCP CLI
  if (/^Bash\(mcp (call|list|get) /.test(perm)) return true;

  // Python/uv generic patterns (without project-specific CLI names)
  if (/^Bash\(uv (sync|run python|run pytest|pip install):/.test(perm)) return true;
  if (/^Bash\((PYTHONNOUSERSITE=1|UV_NO_SYNC=1|TORCH_FORCE_WEIGHTS_ONLY_LOAD=\d+) /.test(perm)) return true;
  if (/^Bash\(python3:/.test(perm)) return true;
  if (/^Bash\(\.venv\/bin\/(python|pytest)/.test(perm)) return true;
  if (/^Bash\(timeout \d+ \.venv\/bin\/python/.test(perm)) return true;

  // Project-specific patterns (return false)
  // - Hardcoded absolute paths: /home/user/...
  if (/\/home\/[^/]+\//.test(perm)) return false;

  // - Project-specific CLI commands: uv run <project-name>
  if (/^Bash\(uv run (?!python|pytest)[^:)]+/.test(perm)) return false;

  // - env vars with PROJECT-NAME prefix (e.g. TELEPATH_*, MYAPP_*)
  if (/^Bash\([A-Z][A-Z0-9]*_[A-Z][A-Z0-9_]*=[^ ]+ /.test(perm) &&
      !/^Bash\((PYTHONNOUSERSITE|UV_NO_SYNC|TORCH_FORCE|GIT_EDITOR|RUSTUP_TOOLCHAIN)/.test(perm)) return false;

  // - Domain-specific docs (not universal registries)
  if (/^WebFetch\(domain:(?!github\.com|pypi\.org|crates\.io)[^)]+\.readthedocs\.io\)/.test(perm)) return false;
  if (/^WebFetch\(domain:docs\.[^)]+\.org\)/.test(perm)) return false;

  return false;
}
```

## Classification Categories

### Generic/Reusable Patterns
**MUST** be promoted to user-side when detected:

#### Universal
- `WebSearch` - Universal web search capability
- `WebFetch(domain:github.com)` - GitHub access
- `WebFetch(domain:pypi.org)` - Python package registry
- `WebFetch(domain:crates.io)` - Rust package registry

#### GitHub MCP Integration
- `mcp__github__issue_write` - Issue creation/updates
- `mcp__github__issue_read` - Issue reading
- `mcp__github__pull_request_read` - PR reading
- `mcp__github__update_pull_request` - PR updates
- `mcp__github__list_issues` - Issue listing
- `mcp__github__create_pull_request` - PR creation
- `mcp__github__search_issues` - Issue search

#### Git Operations
- `Bash(git checkout:*)` - Branch switching
- `Bash(git add:*)` - Staging changes
- `Bash(git commit:*)` - Creating commits
- `Bash(git push:*)` - Pushing changes
- `Bash(git fetch:*)` - Fetching updates
- `Bash(git rebase:*)` - Rebasing branches
- `Bash(git rm:*)` - Removing files
- `Bash(git check-ignore:*)` - Checking gitignore

#### GitHub CLI
- `Bash(gh pr create:*)` - PR creation
- `Bash(gh pr view:*)` - PR viewing
- `Bash(gh pr edit:*)` - PR editing
- `Bash(gh issue create:*)` - Issue creation
- `Bash(gh issue view:*)` - Issue viewing
- `Bash(gh label list:*)` - Label listing

#### Shell Utilities
- `Bash(find:*)` - File search
- `Bash(cat:*)` - File reading
- `Bash(grep:*)` - Pattern search
- `Bash(xargs:*)` - Argument processing
- `Bash(echo:*)` - Output text
- `Bash(env)` - Environment variables

#### Python/uv Generic
- `Bash(uv sync:*)` - Dependency sync
- `Bash(uv run python:*)` - Python execution
- `Bash(uv run pytest:*)` - Test execution
- `Bash(uv pip install:*)` - Package installation
- `Bash(PYTHONNOUSERSITE=1 uv run pytest:*)` - Isolated pytest
- `Bash(UV_NO_SYNC=1 uv run:*)` - Skip sync execution
- `Bash(python3:*)` - Python 3 execution
- `Bash(.venv/bin/python:*)` - Virtual env Python
- `Bash(.venv/bin/pytest:*)` - Virtual env pytest
- `Bash(PYTHONNOUSERSITE=1 PYTHONPATH= .venv/bin/python:*)` - Clean Python
- `Bash(PYTHONNOUSERSITE=1 PYTHONPATH= .venv/bin/python -m pytest:*)` - Clean pytest
- `Bash(timeout 60 .venv/bin/python -m pytest:*)` - Timed pytest (60s)
- `Bash(timeout 120 .venv/bin/python -m pytest:*)` - Timed pytest (120s)
- `Bash(TORCH_FORCE_WEIGHTS_ONLY_LOAD=0 uv run pytest:*)` - PyTorch pytest

### Project-Specific Patterns
**MUST NOT** be promoted (deleted or kept in project config):

#### Hardcoded Paths
- `Bash(git -C /home/user/project log:*)` - Absolute path references
- Any permission containing `/home/<user>/<project>/`

#### Project CLI Commands
- `Bash(uv run myproject:*)` - Project-specific CLI name
- `Bash(uv run <project-name>:*)` - Any non-standard uv run command

#### Domain-Specific Documentation
- `WebFetch(domain:docs.pytorch.org)` - PyTorch docs
- `WebFetch(domain:montreal-forced-aligner.readthedocs.io)` - MFA docs
- `WebFetch(domain:mfa-models.readthedocs.io)` - MFA models docs
- Any domain not in universal registries (github.com, pypi.org, crates.io)

## Autonomous Heuristics

These rules MUST be applied automatically on every invocation without asking the user:

1. **Subsumption check** — before promoting an entry, verify it is not already covered by an existing user-global wildcard (e.g., `Bash(grep -E "...")` ⊆ `Bash(grep *)`). Subsumed entries are silently dropped from the project file.
2. **Wildcard collapsing** — when ≥2 project entries share a common command prefix, promote them as a single wildcard (e.g., `rustup toolchain *` + `rustup run *` + `rustup target *` → `Bash(rustup:*)`).
3. **Project-marker auto-detection** — derive project-specific markers automatically: run `git rev-parse --show-toplevel` to get the repo root, then read `Cargo.toml` or `package.json` for binary/script names. Any permission containing the repo root path or a discovered binary name is classified as project-specific.
4. **Stale global cleanup** — scan existing user global permissions for entries that reference renamed/deleted paths (old crate paths, old binary names from prior refactors). Remove them in the same pass. Heuristic: any `sed -n 'NNp' <path>` or `nm <path>` referencing a file that no longer exists on disk.
5. **Default apply** — always backup first (`settings.backup.<timestamp>.json`), then apply without confirmation. `--dry-run` remains available but is no longer the default.

## Implementation Requirements

1. **Repository Detection**: Claude MUST detect current git repository root via `git rev-parse --show-toplevel`
2. **Project Marker Extraction**: Claude MUST read project `Cargo.toml` or `package.json` to identify project-specific binary/command names
3. **File Reading**: Claude MUST read `.claude/settings.local.json` from project root
4. **User Config Reading**: Claude MUST read existing `~/.claude/settings.json`
5. **Subsumption Check**: Claude MUST skip promotion for any entry already covered by an existing user wildcard
6. **Stale Entry Cleanup**: Claude MUST remove user global entries that reference project paths no longer present on disk
7. **Categorization**: Claude MUST apply permission classification logic including new Rust/system patterns
8. **Deduplication**: Claude MUST avoid adding duplicate permissions to user config
9. **Merging**: Claude MUST preserve existing user permissions
10. **Validation**: Claude MUST validate JSON structure via `jq -e .` before writing
11. **Backup**: Claude MUST backup original user config to `~/.claude/settings.backup.<timestamp>.json` before modifying
12. **Progress Reporting**: Claude MUST show a post-apply categorized report (removed / promoted / unchanged)

## Migration Workflow

The preferred implementation is a Python script executed via Bash (avoids shell quoting issues with JSON):

```python
import json, re, shutil
from datetime import datetime
from pathlib import Path

# 1. Auto-detect project root
import subprocess
repo_root = Path(subprocess.check_output(
    ["git", "rev-parse", "--show-toplevel"], text=True).strip())

# 2. Read project settings
project_settings_path = repo_root / ".claude/settings.local.json"
if not project_settings_path.exists():
    print("No .claude/settings.local.json found — nothing to migrate")
    exit(0)
with open(project_settings_path) as f:
    project_settings = json.load(f)

# 3. Read user settings and create backup
user_settings_path = Path.home() / ".claude/settings.json"
with open(user_settings_path) as f:
    user_settings = json.load(f)
backup_path = user_settings_path.parent / f"settings.backup.{datetime.now().strftime('%Y%m%d-%H%M%S')}.json"
shutil.copy2(user_settings_path, backup_path)

# 4. Classify and apply (subsumption + stale cleanup + promote)
allow = user_settings["permissions"]["allow"]
project_allow = project_settings.get("permissions", {}).get("allow", [])

def should_remove_from_global(entry, repo_root_str):
    # Stale sed entries with project-specific paths that no longer exist
    m = re.search(r"sed.*?'[^']*' ([^ )]+)", entry)
    if m and not Path(m.group(1)).exists():
        return True
    # nm entries with old absolute paths that no longer exist
    m = re.search(r"nm ([^ )]+)", entry)
    if m and not Path(m.group(1)).exists():
        return True
    return False

removed_global = [e for e in allow if should_remove_from_global(e, str(repo_root))]
kept = [e for e in allow if e not in removed_global]
existing_set = set(kept)

new_entries = [e for e in project_allow if isGenericPermission(e) and e not in existing_set]
user_settings["permissions"]["allow"] = kept + new_entries

# 5. Write updated user settings
if not options.dryRun:
    with open(user_settings_path, "w") as f:
        json.dump(user_settings, f, indent=2, ensure_ascii=False)
        f.write("\n")
    # Delete project settings (default) or keep project-specific
    if not options.keepProjectSpecific:
        project_settings_path.unlink()
    else:
        project_specific = [e for e in project_allow if not isGenericPermission(e)]
        with open(project_settings_path, "w") as f:
            json.dump({"permissions": {"allow": project_specific, "deny": [], "ask": []}},
                      f, indent=2, ensure_ascii=False)
            f.write("\n")

return {
    "removed_from_global": len(removed_global),
    "promoted": len(new_entries),
    "project_settings_deleted": not options.keepProjectSpecific,
    "backup": str(backup_path),
}
```

## Required Response Format

Claude MUST use the following response format:

```
🔄 Analyzing permissions in current project...

## Project Settings
📂 Found: .claude/settings.local.json
📊 Total permissions: 51

## Categorization Results

### Generic/Reusable (34 permissions)
✅ WebSearch
✅ WebFetch(domain:github.com)
✅ WebFetch(domain:pypi.org)
✅ WebFetch(domain:crates.io)
✅ mcp__github__issue_write
✅ mcp__github__issue_read
... (show all generic)

### Project-Specific (5 permissions)
❌ WebFetch(domain:docs.pytorch.org)
❌ WebFetch(domain:montreal-forced-aligner.readthedocs.io)
❌ Bash(git -C /home/tarotene/myproject log --oneline -10)
❌ Bash(uv run myproject:*)
... (show all project-specific)

## Migration Summary
- 🎯 Generic permissions: 34 found
- ➕ New to user config: 28
- ✓ Already in user config: 6
- 🗑️ Project-specific (deleted): 5

## Actions Taken
✅ Updated ~/.claude/settings.json (28 new permissions)
✅ Deleted .claude/settings.local.json

✅ Migration completed! Restart Claude Code to apply changes.
```

## Error Handling Requirements

Claude MUST handle the following scenarios:
- **No Project Settings**: Report that no `.claude/settings.local.json` exists
- **Not in Git Repo**: Report that command must be run from git repository
- **JSON Parse Error**: Report invalid JSON structure
- **Write Permission**: Report file permission errors
- **Backup Failure**: Warn but continue if backup fails

## Dry Run Mode

When `--dry-run` is specified, Claude MUST:
1. Read all configuration files
2. Perform categorization
3. Show what WOULD be migrated
4. Display diff of user config changes
5. NOT modify any files

## Keep Project-Specific Mode

When `--keep-project-specific` is specified, Claude MUST:
1. Promote generic permissions to user config
2. Write only project-specific permissions back to project config
3. NOT delete project settings file

Example project config after migration with this flag:
```json
{
  "permissions": {
    "allow": [
      "WebFetch(domain:docs.pytorch.org)",
      "Bash(uv run myproject:*)"
    ],
    "deny": [],
    "ask": []
  }
}
```

## Best Practices

1. **Run from Project Root**: Always execute from repository root directory (the command auto-detects via `git rev-parse`)
2. **Preview if unsure**: Use `--dry-run` when introducing the command in a new project to inspect classification before committing
3. **Auto-backup is mandatory**: The command never touches `~/.claude/settings.json` without creating a timestamped backup first
4. **Restart Claude**: Settings require Claude Code restart to take effect
5. **Version Control**: The `.claude/settings.local.json` deletion is local-only; commit its removal to git to signal intent to teammates

## Security Considerations

- User config may be shared across sensitive projects
- Only promote permissions that are truly generic
- Avoid promoting permissions with hardcoded credentials or paths
- Review project-specific patterns before deletion

## Integration with Other Commands

This command complements:
- `/backup-claude-settings` - Backup user config after migration
- `/restore-claude-settings` - Restore user config on new machines

## Examples

### Standard Migration
```
/promote-permissions
```

### Preview Changes
```
/promote-permissions --dry-run
```

### Migrate and Clean
```
/promote-permissions --delete-after
```

### Preserve Project Permissions
```
/promote-permissions --keep-project-specific
```
