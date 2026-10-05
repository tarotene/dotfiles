# rulesets-write-guard — GitHub ruleset の直接書換を deny する

`crates/rulesets-write-guard`(Rust、ADR-0024)の設計根拠(ADR-503 D7)。

## 動機

ADR-503 は、required status check の正本を
対象リポジトリ自身の `.github/rulesets/*.json` に一本化し、
`apply-rulesets.sh`(`crates/apply-rulesets`、#414)が PUT/POST 直前に「宣言した context が実測
job 名として報告されるか」を検証するようにした(D5)。しかし Claude
セッションが `gh api -X PUT repos/O/R/rulesets/<id>` を直接叩けば、この
検証を素通りしてリポジトリ外の値をそのまま live ruleset に書き込める —
telepath#243 を含む複数リポジトリで required check を永久 Expected に
した事故を生んだのと同じ「テンプレートの required context が対象リポジトリの
実際のジョブ名と同じ PR で編集される保証が無い」構造を、hook を迂回する
形で再現してしまう。

## 仕組み

PreToolUse(`Bash`、`if: Bash(gh *)`)専用。`gh-edit-allow` と違い記録役は
持たない。

- `hook_io::shell::split` で語分割し、`gh api ...` の呼び出しだけを対象に
  する。判定に使えない入力(複合コマンド・展開・リダイレクト・glob・未知の
  フラグ)は `gh-edit-allow` と同じ理由で素通しする(位置引数を取り違える
  おそれがあるため)。
- method(`-X`/`--method`/`-XPOST` 圧縮形/`--method=POST` 等値形、省略時は
  GET)と path(`https://api.github.com/` 前置・先頭 `/` を許容)を取り出し、
  method が POST/PUT/PATCH/DELETE のいずれかで、path が
  `repos/{owner}/{repo}/rulesets` または `repos/{owner}/{repo}/rulesets/{id}`
  に一致すれば deny する。
- **deny のみ返す**(allow/ask は一切出さない)。不一致は通常の確認フローに
  そのまま落ちる。
- **bypass**: コマンド文字列の先頭に `RULESETS_WRITE_GUARD_BYPASS=<非空値>`
  という env var 代入があれば判定しない。使うのは、本人が `!` で
  `RULESETS_WRITE_GUARD_BYPASS=1 gh api ...` を手動で叩くときだけである。
  `apply-rulesets` の呼び出し(1 回の Bash tool 呼び出し)はそもそも
  この hook の deny に一致しないので、前置は不要で、`apply-rulesets` 自身も
  子プロセスに立てない(立てても hook は子の環境変数を見ない)。auto モードでは
  前置した形が分類器に safety bypass と判定されて拒否される(#707)。

## 先行例との差分

- **`gh-edit-allow`**: 同じ `hook_io::shell` 語分割・同じ「未知の入力は
  判定しない」設計を踏襲する。gh-edit-allow は allow-only、この hook は
  deny-only という向きの違いがある。語分割は `crates/gh-edit-allow/src/
  shell.rs` から `crates/hook-io/src/shell.rs` へ引き上げて共有した
  (ADR-0035 D1「単一正本 > 複写+同期」)。
- **実装言語**: ADR-0024 は新規 hook を既定で Rust とし、bash 例外は
  「既存の巨大 bash 資産(`scripts/github-audit` 等)を source して判定
  ロジックを再利用する」ときに限る(`rust-migration.toml` の
  コメント参照)。この hook は `gh api` の引数解析だけで完結し、
  `github-audit` を source する必要が無いため、既定どおり Rust で書いた。
