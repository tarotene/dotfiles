# ADR-0005 — shell-extension init gates on binary existence, never on auth

- Status: Accepted
- Date: 2026
- Epic: #207

## Context

#255 added a `GITHUB_TOKEN` gate to the ghr shell-extension loader
(`config/zsh/modules/30-tools-ghr.zsh`) on the assumption that `ghr shell bash`
requires a token to initialize. It does not:

```
GITHUB_TOKEN= ghr shell bash >/dev/null; echo $?
0
```

`ghr shell bash` exits 0 and emits a valid script (defining the `ghr()` wrapper
function) even when `GITHUB_TOKEN` is empty. The token warning that #255 wanted
to silence was already suppressed by the `2>/dev/null` redirect on the same
line — the added token gate was both redundant and harmful.

This surfaced under home-manager specifically: ADR-0002's hybrid scheme deploys
the same zsh modules verbatim, and the home-manager session has no
`GITHUB_TOKEN` set. With an empty token the entire `if` block was skipped, the
`ghr()` wrapper was never defined, and `ghr cd` fell through to the real binary,
which prints `ERROR Shell extension is not configured correctly.`

## Decision

Shell-extension initialization (`eval "$(tool init …)"` / `source <(tool …)`)
MUST gate only on binary existence (`command -v`), never on authentication
credentials such as `GITHUB_TOKEN`. Tool warnings are suppressed via
`2>/dev/null` on the init command, not by skipping the loader.

Where practical, a post-load assertion verifies that the expected wrapper, hook,
or completion actually registered and warns to stderr otherwise, so a silently
failed extension is caught at shell startup instead of as a cryptic runtime
error later.

## Consequences

- `ghr cd` works in token-less environments (the home-manager trial, fresh
  machines) again.
- The pattern generalizes to every shell-extension tool. #260 adds post-load
  assertions to `direnv`, `mise`, `uv`, `sheldon`, and `starship`.
- Load failures become visible immediately (a one-line stderr warning) rather
  than manifesting as a confusing error the first time the missing function is
  invoked.
