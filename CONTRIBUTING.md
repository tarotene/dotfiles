# Contributing

## Issue litmus

判定問: Does resolving this change what `home-manager switch` (or the documented apt/bootstrap escape hatch) provisions or configures on a host?

採用例:
- adopt a new CLI tool into home-manager's package list
- fix a home-manager module that fails to activate on a host

棄却例:
- add a new feature to `herdr` itself (belongs in the upstream `herdr` repo)
- pin a specific project's Rust toolchain version (belongs to that project's `mise`/`rustup` config, not this repo)
