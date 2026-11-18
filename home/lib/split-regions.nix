# A text file split in two by marker lines: `{ core, decisions }`.
#
# The shared instructions (config/agents/AGENTS.md) and Claude's own (config/claude/CLAUDE.md) stay
# one file each — the single source — and mark the sections that concern the *records of decisions*
# (precedent checks, "look up the existing ruling first", ADR enforcement points, technology
# selection, "ask whether an existing means suffices") with a pair of lines:
#
#   <!-- agent-mode:decisions:begin -->
#   ...
#   <!-- agent-mode:decisions:end -->
#
# This function cuts the text at those lines. `core` is everything outside the marked regions;
# `decisions` is everything inside them (regions separated by one blank line). The marker lines
# themselves are in neither. Claude Code imports both files, and an agent mode (`CLAUDE_MODE=...`,
# docs/agents/agent-mode.md) drops `decisions` through `claudeMdExcludes`, so the sections are not
# loaded rather than merely "not to be followed". Codex and Copilot read the unsplit file.
#
# A mistake cannot pass silently: an unbalanced marker, a nested one, or a file with no region at
# all fails the evaluation (an empty `decisions` would make the mode exclude nothing).
{ lib }:
let
  begin = "<!-- agent-mode:decisions:begin -->";
  end = "<!-- agent-mode:decisions:end -->";
in
text:
let
  step =
    acc: line:
    if line == begin then
      assert lib.assertMsg (!acc.inside) "split-regions: `${begin}` が入れ子になっている";
      acc
      // {
        inside = true;
        # 領域の間は 1 行の空行で区切る。
        decisions = acc.decisions ++ lib.optional (acc.decisions != [ ]) "";
      }
    else if line == end then
      assert lib.assertMsg acc.inside "split-regions: 対応する begin が無い `${end}` がある";
      acc // { inside = false; }
    else if acc.inside then
      acc // { decisions = acc.decisions ++ [ line ]; }
    else
      acc // { core = acc.core ++ [ line ]; };
  result = lib.foldl' step {
    inside = false;
    core = [ ];
    decisions = [ ];
  } (lib.splitString "\n" text);
in
assert lib.assertMsg (!result.inside) "split-regions: `${end}` で閉じられていない領域がある";
assert lib.assertMsg (
  result.decisions != [ ]
) "split-regions: 領域が 1 つも無い(decisions が空だと、モードが何も除外しない)";
{
  core = lib.concatStringsSep "\n" result.core;
  decisions = lib.concatStringsSep "\n" result.decisions + "\n";
}
