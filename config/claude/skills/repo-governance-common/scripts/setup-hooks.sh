#!/usr/bin/env bash
# setup-hooks.sh — Configure git to use the .githooks directory in the
# target repo. Single source shared by rust/typst/astro-site-repo-
# governance (#388): the three per-skill copies had zero real logic
# difference — only the names of ecosystem-specific flags they silently
# discard (--msrv vs --node-version vs --typst-version, ...) and the echo
# text of the "Active hooks" summary. This file drops both differences:
# ecosystem flags are discarded via a name-agnostic pattern instead of an
# enumerated per-ecosystem list, and the hooks summary points at
# .githooks/ (the actual source of truth for what each hook does) instead
# of repeating tool names here where they can drift out of sync.
#
# Deployed into every *-repo-governance skill directory by
# home/modules/claude.nix (ADR-0032-style single-source, multi-mount).
set -euo pipefail

DEST=""
DRY_RUN=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dest) DEST="$2"; shift 2 ;;
    --dry-run) DRY_RUN=true; shift ;;
    # The flags that take no value (rust's manual-invocation firmware
    # addin, and ADR-0000-rulesets-declaration-in-repo's shared
    # --with-review) — kept as explicit exceptions so they aren't
    # mis-parsed as value-taking flags by the `--*` catch-all below.
    --with-firmware) shift ;;
    --with-review) shift ;;
    # Every other ecosystem-specific flag seed.sh passes through from its
    # own COMMON_ARGS is a `--flag value` pair (rust's --msrv/--msrv-full/
    # --canonical-crate/--cli-crate, astro's --node-version/--package-name/
    # --package-version/--site-base/--pages-url, typst's --typst-version/
    # --min-typst/--ats-email, plus --owner/--repo/--default-branch shared
    # by all three). None of them matter to git-hooks configuration, so
    # this discards any such pair by pattern instead of enumerating three
    # per-ecosystem lists that would have to stay in sync with the
    # apply-rulesets.sh scripts (the authoritative per-ecosystem list).
    --*) shift 2 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

[[ -z "$DEST" ]] && echo "ERROR: --dest is required" && exit 1

if [[ ! -d "$DEST/.git" ]]; then
  echo "  WARN: $DEST does not appear to be a git repository (no .git dir)."
  echo "        Skipping git hooks configuration."
  exit 0
fi

if [[ ! -d "$DEST/.githooks" ]]; then
  # dry-run reports this as the expected pre-copy state rather than an
  # error: copy-files.sh --dry-run never actually creates .githooks (#388
  # D6), and seed.sh runs copy-files.sh -> setup-hooks.sh -> ... in one
  # set -e pipeline, so exiting non-zero here would abort the preview
  # before it reaches the settings/rulesets steps on a brand-new repo.
  if [[ "$DRY_RUN" == "true" ]]; then
    echo "  DRY-RUN: would configure core.hooksPath .githooks (dir not yet created)"
    exit 0
  fi
  echo "  WARN: $DEST/.githooks does not exist — run copy-files.sh first."
  exit 1
fi

if [[ "$DRY_RUN" == "true" ]]; then
  echo "  DRY-RUN: would run: git -C $DEST config --local core.hooksPath .githooks"
else
  git -C "$DEST" config --local core.hooksPath .githooks
  echo "  ✓  core.hooksPath set to .githooks in $DEST"
  echo "     Active hooks: see .githooks/ in this repository for the configured"
  echo "     commit-msg / pre-commit / pre-push hooks."
fi
