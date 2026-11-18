# ADR-0000 — セッション単位のモードは、起動時に 1 つの宣言から展開し、展開できないときは通常のセッションに倒さない

- Status: Accepted
- Date: 2026-11-18
- Issue: No-Issue(巨大なリポジトリで、ADR のような覆しにくい決定に引きずられずに整理を進めたい、という本人の依頼。
  方針は本人が `AskUserQuestion` で選んだ: 環境変数で起動時に展開する・どのリポジトリでも・読ませないものは 4 種・2 段階)
- Amends: なし

## Context

決定の記録(ADR・設計文書)は、accepted のあとは変えない。覆すときは、新しい ADR を足す(ADR-0008)。普段は健全だが、
巨大なリポジトリで大きな整理をするときは、エージェントが記録を読む量と、記録に合わせようとする拘束が、整理の邪魔になる。
dotfiles では `docs/adr` が 96 本、`docs/agents` が 97 本、毎セッション読まれる共通規約が 1,084 行ある(2026-11-18)。

拘束を外す手段は、これまで機体単位(`hms` の適用時に固定)か、gate ごとの `SKIP_<STEM>=1`(人が起動前に 1 つずつ立てる)だった。
セッションごとに、読ませない範囲・追記する文・止める検査を、まとめて切り替える単位が無い。

## Decision

### D1: モードは、宣言(TOML)1 つから、3 つを展開して `claude` を exec する

`CLAUDE_MODE=<名前> claude` と起動すると、zsh の `claude` 関数が `agent-mode exec` に渡し、
`config/agents/modes/<名前>.toml` から、次の 3 つを作る。

- 読み取り禁止の設定: `--settings` にインラインで渡す `permissions.deny` の `Read(..)` と `Edit(..)`。
- 追記する文: `--append-system-prompt-file`。
- 止める検査の束: 既存の `SKIP_<STEM>=1`(セッション単位の逃げ道)。

### D2: 止める検査の名前は、逃げ道の表が唯一の正本で、宣言には stem だけを書く

stem が `escape::ALL` に無い、またはセッション単位を持たないときは、起動を失敗させる。新しい逃げ道は作らず、表にあるものを束ねる。

### D3: 展開できないときは、通常のセッションに倒さない

`CLAUDE_MODE` が指定されたのに、`agent-mode` が無い・宣言が読めない・検証に落ちるときは、起動しない(終了コード 127 または 2)。
拘束を外したつもりで、外れていない(または、外れていない前提で、外れた)セッションを黙って始めないため。

### D4: 第 1 段(モード)は、記録を読めも書けもしない。覆す ADR は第 2 段(通常のセッション)で足す

Claude Code の `Read` の deny は、同じパスへの Edit・Write も止める。したがってモードでは記録を足せない。モードの PR は
`--draft` で出し、本文の `## 触れた決定` に、消した・変えた検査・設定・文書を残す。通常のセッションで、覆す決定ごとに新しい ADR を
**同じブランチ**に足す。

## 先行例との対比

- D1: 起動時に、宣言から設定・追記する文・環境変数を束ねて渡す
  先行例: Anthropic, "CLI reference", https://code.claude.com/docs/en/cli-reference (取得 2026-11-18)
  差分: 異なる — 公式は個別のフラグだけで、束ねる仕組みは無い。束ねる側をこのリポジトリが持つ
  軸: 表現不可能 — 宣言 > 手続き
- D2: 逃げ道の名前を表から導く
  先行例: crates/agent-session/src/escape.rs (取得 2026-11-18)
  差分: 一致 — 新しい逃げ道を作らず、表にある名前を束ねる
  軸: 表現不可能 — 閉語彙(表)と突き合わせる
- D3: 展開できないときは起動しない
  先行例: crates/repo-scope-guard の「読めなければ deny」(docs/adr/1608-repo-scope-by-machine-contract.md の D1)
  差分: 一致 — 拘束を変える仕組みは、失敗したら緩める側ではなく、止める側に倒す
  軸: 表現不可能 — 失敗の状態から、通常のセッションに入る経路を無くす

## Consequences

- 同じ worktree で、通常のセッションとモードのセッションを並べられる(モードは起動したプロセスだけに効く)。
- 共通規約の本文(`@~/.agents/AGENTS.md` の import)は、段1 では読み込まれたままで、追記する文で適用外を宣言する。
  読み込まれない状態にするのは、同じスタックの後の段。
- CI の検査(`decision-colocation-check` など)と `pr-gate` は止まらない。
- モードを足すには、`config/agents/modes/<名前>.toml` と追記する文を足し、`home/modules/agents.nix` の配備に 2 行を足す。

## 執行点

- crates/agent-mode/src/lib.rs
- config/agents/modes/refactor.toml
- config/zsh/modules/53-tools-claude.zsh
