# Contributing

See [README](README.md) for what this repository is and how to set up a
development environment.

## Issues

Judging question: Does resolving this change what `home-manager switch` (or the documented apt/bootstrap escape hatch) provisions or configures on a host?

Accepted:
- adopt a new CLI tool into home-manager's package list
- fix a home-manager module that fails to activate on a host

Rejected:
- add a new feature to `herdr` itself (belongs in the upstream `herdr` repo)
- pin a specific project's Rust toolchain version (belongs to that project's `mise`/`rustup` config, not this repo)

An Issue that proposes adopting a tool carries a "feasibility check on the host" section: the tool was run without a permanent change (for example `nix shell nixpkgs#<pkg> -c <cmd>`) on the real input and output, with the date, platform, command, result and any snag recorded. If it could not be tried, the Issue says so and why.

## Pull requests

Every PR body follows the 5-section skeleton described in `docs/claude/pr-description.md` and either closes an issue or states `No-Issue: <reason>`. CI (`.github/workflows/`) must pass before merge; this repository merges by squash only.

## Expectations

This is a single-maintainer repository. AI agents open issues and pull requests here, but merge decisions are made by a human.
