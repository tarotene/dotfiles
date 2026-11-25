# ADR-1712 — 画面の面に触れる計画に、`構造:` の行を要求する(設計の時点の検査)

- Status: Accepted
- Date: 2026-11-25
- Issue: No-Issue(ADR-1711 の構造の規則の続き。ページの構造を選ぶ規則を、設計の時点で守らせる)

## Context

ADR-1711 は、ページの構造を、課題の性質の軸から選ぶ規則にし、すべての HTML に宣言を要求した。この検査は、HTML を書いた後に働く。構造は、HTML を書く前、つまり計画の時点で決まる。計画の時点には、構造を選んだかを問う仕組みが無い。

- `plan-precedent-gate` は、`## 先行例との対比` 節の形式(各 `Dn` の先行例・差分・軸、技術選定なら本命・対抗馬・既存手段)を見る。内容で発火する検査は無い。重い欄は、書かれていれば整合を見るだけで、書くべきかを判定しない(`docs/agents/technology-selection.md`)。
- Write・Edit に掛かる hook は、HTML を見ない。`new-tool-guard` は、shebang 付きのファイルと、`bin`・`scripts`・`hooks`・`cmd` の下の新規ファイルと、パッケージの宣言を新しい道具とみなす。HTML の雛形の新規作成は、その外にある。
- したがって、計画を書く段で、構造を課題の性質から選ぶ手順を踏んだかは、機械で問えない。構造は、ADR-1711 の検査(宣言の有無と整合)に任される。

## Decision

### D1: 画面の面に触れる計画は、`## 先行例との対比` の中に、`構造:` の行を書く。`plan-precedent-gate` が、形だけを検査する

計画の本文に、画面の面を示すファイル(`.html`・`.css`・`.svelte`・`.astro`・`.tsx`・`.jsx`)の名前、`config/web/`、`page-types.toml` が出るのに、`構造:` の行が無ければ deny する。行の形は、`構造: <パターン id> — 軸: <軸=値, …> — 退けた: <パターン id>(<理由>)`、または画面を変えない計画の `構造: 該当なし — <理由>`(ダッシュは `—`・`–`・`-` のいずれでもよい)。`## 先行例との対比` 節があれば、その中を探し、節の無い計画(免除の 1 行だけの計画)は、計画全体から探す。`構造:` があるのに形が違うときは、形の違いとして指摘する。

**形だけを見る。** パターン id と軸の語彙は `config/web/structures.toml` にあり、この gate は、リポジトリを読まない(どのリポジトリの計画にも同じ gate が働くため、特定の checkout に依存させない)。語彙との整合は、計画の内容の判断で、`copilot-plan-review` の lens A が監査する(監査項目に足した)。

先行例: `crates/plan-precedent-gate/src/lib.rs` の `HEAVY_RE` と、`docs/agents/technology-selection.md`(重い欄は、書かれていれば整合を見るだけで、書くべきだったかは見ない。発火条件の判定は意味判断で、決定論では解けない) (取得 2026-11-25)
差分: 異なる — 既存は「書くべきか」を機械で判定しない設計だが、画面の面は、拡張子とパスという形で近似できる。誤検知(画面を変えない計画)は、免除の 1 行で抜けられる。ダッシュ種を 3 つ許す書式は、既存の免除行(`先行例: 該当なし — …`)と同じ。
既存手段: crates/plan-precedent-gate/src/lib.rs — 拡張: 既存の gate の `judge_precedent` に、条件つきの欄を足す(`既存手段:` の要求と同じ「条件の検出 → 欄の要求 → メッセージ」の型)
軸: 検出のみ — 計画の文章から、画面に触れるかを機械で確定できない。最終の担保は、すべての HTML に構造の宣言を要求する ADR-1711 の検査に置く。

### D2: 文法の正本は `precedent-grounding` スキル §3 に置く。CLAUDE.md と Codex の指示文は、1 句で指す

`構造:` の文法と、画面の面の判定、形だけを見ること、構造を選ぶ工程の所在(`docs/agents/personal-web-app.md` の「画面の作り方」)を、`precedent-grounding` スキルの §3 に書いた。`config/claude/CLAUDE.md` と `config/codex/AGENTS.codex.md` は、文法を複写せず、スキルを指す(既存の `Dn` のトークンと同じ扱い)。`copilot-plan-review` の lens A の監査項目と、`plan` の語彙(`ground` 事象の説明)に、構造の行を足した。

軸: 還元 — 文法の正本を 1 か所(スキル)に置き、指示文には複写しない。

## Alternatives considered

- **gate が `config/web/structures.toml` を読み、パターン id と軸の語彙も検査する**: gate が、どのリポジトリの計画でも、特定の checkout に依存する。語彙の整合は、内容の判断で、lens A のほうが適す。採らない。
- **`## 先行例との対比` の `Dn` に、構造の選択を、技術選定の重い欄(本命・対抗馬)の形で書かせる**: 構造の選択は、外部依存の新設でも、戻せないものでもなく、重い欄の発火条件に当たらない。専用の 1 行のほうが、検出しやすい。採らない。
- **画面の面の判定に、`画面`・`ページ` などの語も使う**: 画面を変えない計画の本文にも、頻出する。誤検知が多すぎる。拡張子とパスだけにした。

## Consequences

- 画面の面に触れる計画は、`構造:` の行がなければ、ExitPlanMode の前に deny される。画面を変えない計画は、`構造: 該当なし — <理由>` の 1 行で抜ける。
- この ADR の計画も、画面の面(`config/web/` ほか)に触れるので、この検査の対象である。
- 判定は、計画の文章の形の近似で、意味では確定しない。見逃し(画面を変えるのに、画面の面を示す名前が計画に出ない)は、ADR-1711 の検査(HTML の宣言の有無)が、書いた後に拾う。
- `example_block`(拒否文の例文)は変えない。`構造:` は条件つきで、通常の拒否には出ない。拒否文は、`構造:` の拒否のときだけ、行の書き方を 1 文で添える。

## 執行点

- `crates/plan-precedent-gate/src/lib.rs`
- `crates/plan-precedent-gate/tests/cmd/check-structure-ok.toml`
- `crates/plan-precedent-gate/tests/cmd/check-structure-ng.toml`
- `crates/copilot-plan-review/src/prompts.rs`
- `config/agents/plan/vocabulary.json`

## Verification

- `cargo test -p plan-precedent-gate`: 単体試験(画面の面に触れる計画は、`構造:` が無ければ deny、書けば通る、免除はダッシュ 3 種で通る、形の崩れは形の違いとして指摘、拡張子とパスの認識、節が無いときは計画全体を探す、節があるのに節の外に書いても通らない、画面に触れない計画には要求しない)と、`--check` の fixture(`check-structure-ok`・`check-structure-ng`)が成功する。既存の fixture(38 件)は、変えずに通る。
- `cargo test -p copilot-plan-review`、`cargo test -p hook-router`(ルータの同値性試験)が成功する。
- 実際の計画で: この ADR を決める計画ファイルを `plan-precedent-gate --check` に渡し、`構造:` の行を含む計画が通ること。
