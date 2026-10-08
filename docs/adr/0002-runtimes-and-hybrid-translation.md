# ADR-0002 — runtime consolidation + hybrid config translation

- Status: Accepted
- Date: 2025
- Epic: #207

## Context

Today runtimes are managed by a spread of bespoke tools: rustup (Rust), uv
(Python), mise (Node), SDKMAN! (Java), goup (Go), plus ROS on one host. That is
a lot of overlapping version managers to provision and keep working.

Separately, we must decide *how* to translate the existing config files into the
flake: rewrite everything as Nix DSL, or keep files literal.

## Decision

### Runtimes

- **Consolidate Java and Go into `mise`.** Drop SDKMAN! and goup.
- **Keep `rustup` as the global Rust default.** Rationale: cross-compilation and
  C↔Rust FFI on the work PC, and embedded targets on the personal PC, need the
  full rustup toolchain/target management that a pinned nixpkgs Rust does not
  cover ergonomically.
- **Keep `uv` for Python** and `mise` for Node and other runtimes.
- **ROS is scoped to the personal host only** — it is not provisioned on the
  work machine.
- home-manager provides the **launchers** (`mise`, `direnv`, `rustup`); the
  actual toolchains stay per-project escape hatches (ADR-0001).

### Translation strategy — hybrid

- **Keep working config files literal** and reference them via `xdg.configFile`
  / `home.file`. Examples: the 15 `config/zsh/modules/*.zsh`, `starship.toml`,
  `alacritty.toml`, `sheldon/plugins.toml`.
- **Use Nix DSL only where interpolation pays** — e.g. per-host git identity,
  values that differ between hosts/identities, or where a `programs.*` module
  removes real boilerplate.
- Track literal configs that are good candidates for later nixification in
  `docs/nixification-roadmap.md` (e.g. sheldon → native plugin management).

## Consequences

- Fewer version managers to install and debug; mise becomes the common runtime
  launcher with rustup/uv as deliberate exceptions.
- Migration is low-risk: today's battle-tested config files keep running
  verbatim, so behaviour is preserved while ownership moves into the flake.
- A clear, incremental path (the roadmap) to deepen nixification later without
  blocking the cutover.
- Shell-extension modules deployed literally under this hybrid scheme gate on
  binary existence only, never on auth credentials (e.g. `GITHUB_TOKEN`) — see
  ADR-0005. A token-gated loader breaks in the token-less home-manager session.

## Amendment (2026-10-08 — #750)

Decision の「動いている設定ファイルはリテラルのまま置き、補間が効く所だけ Nix DSL を
使う」には理由が書かれておらず、Consequences の「battle-tested な設定がそのまま動く」
が実質の根拠になっていた。これは過去の動作実績を理由にする書き方で、ADR-751 の
前提(歴史的経緯と書き直しの作業量を除く)と食い違う。コード 0 行から選ぶとしたら
何を採るか、の問いで 3 軸の評価をやり直した。**決定は維持する。理由を書き直す。**

1. **表現不可能性。** リテラルのファイルは、補間が無い限りそれ自体が単一正本で、
   それを検査する道具(`zsh -n`、shellcheck)がそのまま効く。Nix DSL に翻訳すると、正本が DSL に移り、道具が見るのは生成物に
   なるので、構文の誤りを書いた場所で検出できなくなる。補間が効く所(ホスト・
   identity ごとの値)は DSL が単一正本になるので、この軸でも DSL が勝つ — 決定の
   後半と一致する。
2. **還元性。** 補間の無い所を DSL にしても、層が 1 つ増えるだけで、リテラルでは
   できない仕事をしない。`programs.*` モジュールが定型を除ける所は、その仕事がある
   ので DSL を使う。
3. **先進性。** 同点。

プロジェクトの `AGENTS.md` の「Hybrid translation」節は、同じ理由に書き換えてある。

### 執行点

- AGENTS.md
