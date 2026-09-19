#!/usr/bin/env bash
# setup-hooks.sh — Configure git to use the .githooks directory in the target repo.
set -euo pipefail

DEST=""
DRY_RUN=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dest)    DEST="$2";    shift 2 ;;
    --dry-run) DRY_RUN=true; shift ;;
    # Accepted but unused
    --owner|--repo|--default-branch|--typst-version|--min-typst|--ats-email)
               shift 2 ;;
    *)         echo "Unknown option: $1"; exit 1 ;;
  esac
done

[[ -z "$DEST" ]] && echo "ERROR: --dest is required" && exit 1

if [[ ! -d "$DEST/.git" ]]; then
  echo "  WARN: $DEST does not appear to be a git repository (no .git dir)."
  echo "        Skipping git hooks configuration."
  exit 0
fi

if [[ "$DRY_RUN" == "true" ]]; then
  echo "  DRY-RUN: would run: git -C $DEST config --local core.hooksPath .githooks"
  exit 0
fi

if [[ ! -d "$DEST/.githooks" ]]; then
  echo "  WARN: $DEST/.githooks does not exist — run copy-files.sh first."
  exit 1
fi

git -C "$DEST" config --local core.hooksPath .githooks
echo "  ✓  core.hooksPath set to .githooks in $DEST"
echo "     Active hooks:"
echo "       commit-msg  — Conventional Commits regex validation (no external deps)"
echo "       pre-commit  — typstyle --check (format gate)"
echo "       pre-push    — just ci (full local gate: fmt + yaml + build + verify)"
