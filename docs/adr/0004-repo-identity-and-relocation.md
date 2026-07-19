# ADR-0004 — repo identity & relocation to public `tarotene/dotfiles`

- Status: Accepted
- Date: 2025
- Epic: #207

## Context

The migration happens in the existing repository. We need to decide the
long-term home and identity of the repo: name, visibility, history, and release
model.

## Decision

- **Keep the `dotfiles` name.** The migration does not rename the project.
- **Develop on a `feat/home-manager` integration branch** in the current repo.
  Sub-issues (#208–#220) land as PRs into that branch; it merges once the
  cutover is complete.
- **Relocate to a public `tarotene/dotfiles`** repository as the final step
  (Phase 5, #220), via a **clean orphan history** — not a force-push of the
  existing history. The publish is **secret-scan gated**: history is scanned and
  the new public history starts clean.
- **No semver releases.** This is a personal-environment repo; the source of
  truth is the latest commit on the default branch plus pinned `flake.lock`,
  not tagged releases.

## Consequences

- A single repo carries the migration; no premature split or rename churn.
- The public repository starts from a clean, secret-free history rather than
  inheriting years of procedural commits that may contain sensitive artefacts.
- Reproducibility comes from `flake.lock` pinning and generations, so dropping
  semver releases costs nothing.
