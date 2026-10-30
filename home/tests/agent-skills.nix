# Check the deployed artifacts, including overlays and executable helpers.
{
  pkgs,
  lib,
  files,
}:
let
  sourceRoot = ../../config/agents/skills;
  names = builtins.filter (name: builtins.pathExists (sourceRoot + "/${name}/SKILL.md")) (
    builtins.attrNames (builtins.readDir sourceRoot)
  );
  shared = builtins.filter (name: name != "browser-login-handoff") names;
  entries = lib.filterAttrs (
    path: _: lib.hasPrefix ".claude/skills/" path || lib.hasPrefix ".agents/skills/" path
  ) files;
  expected =
    map (name: ".claude/skills/${name}") (names ++ [ "repo-governance-common" ])
    ++ map (name: ".agents/skills/${name}") (shared ++ [ "repo-governance-common" ]);
in
assert builtins.length names == 43;
assert builtins.length shared == 42;
assert
  lib.sort builtins.lessThan (builtins.attrNames entries) == lib.sort builtins.lessThan expected;
assert lib.all (entry: !entry.recursive && !entry.force) (builtins.attrValues entries);
pkgs.runCommand "agent-skills" { } (
  lib.concatMapStringsSep "\n" (path: ''
    mkdir -p "$(dirname "$out/${path}")"
    ln -s ${entries.${path}.source} "$out/${path}"
  '') expected
  + ''
    for root in "$out/.claude/skills" "$out/.agents/skills"; do
      for dir in "$root"/*; do
        test -L "$dir"
        test -z "$(find -L "$dir" -type l -print -quit)"
        name=$(basename "$dir")
        if [ "$name" != repo-governance-common ]; then
          test -f "$dir/SKILL.md"
          test ! -L "$dir/SKILL.md"
        fi
        # Every source resource survives byte-for-byte, with its execute bit.
        while IFS= read -r -d $'\0' file; do
          relative="''${file#${sourceRoot}/$name/}"
          cmp "$file" "$dir/$relative"
          if [ -x "$file" ]; then test -x "$dir/$relative"; fi
        done < <(find -L "${sourceRoot}/$name" -type f -print0)
      done
      for profile in rust typst astro-site; do
        for script in setup-hooks.sh apply-repo-settings.sh; do
          test -x "$root/repo-governance/$profile/scripts/$script"
          cmp "$root/repo-governance/$profile/scripts/$script" "${sourceRoot}/repo-governance-common/scripts/$script"
        done
        for rule in ${sourceRoot}/repo-governance-common/templates/.github/rulesets/*.json; do
          relative="$profile/templates/.github/rulesets/$(basename "$rule")"
          if [ -e "${sourceRoot}/repo-governance/$relative" ]; then
            cmp "$root/repo-governance/$relative" "${sourceRoot}/repo-governance/$relative"
          else
            cmp "$root/repo-governance/$relative" "$rule"
          fi
        done
      done
      # The former per-language skills stay as entries that carry no resources.
      for name in rust-repo-governance typst-repo-governance astro-site-governance; do
        test "$(find -L "$root/$name" -type f | wc -l)" -eq 1
      done
      for name in pr-description venue-search inbox-adjudication; do
        cmp "$root/$name/reference/browser-operations.md" ${../../config/agents/reference/browser-operations.md}
      done
      for name in pr-description inbox-adjudication stacked-pr github-audit-triage repo-governance; do
        cmp "$root/$name/reference/ci-failures.md" "${sourceRoot}/repo-governance-common/reference/ci-failures.md"
      done
    done
    cmp "$out/.claude/skills/browser-login-handoff/reference/browser-operations.md" ${../../config/agents/reference/browser-operations.md}
  ''
)
