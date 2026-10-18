# ADR-833 — アーカイブ済みのリポジトリには、naming の宣言の欠落(`class-undeclared`)を要求しない

- Status: Accepted
- Date: 2026-10-18
- Issue: No-Issue(`/github-audit-triage` の実行中に発見・裁定。アーカイブ済み 29 リポジトリが `class-undeclared` で drift していた。ユーザーの裁定は「監査側を直す」)
- Amends: ADR-0014(`class-undeclared`)、ADR-0026(アーカイブ済みを naming だけ判定する)

## Context

`github-audit` は、アーカイブ済みのリポジトリを、naming 以外の 5 ドメインでは `not-applicable` にする。
読み取り専用のリポジトリの governance drift は、直されることが無いからである(`docs/github-audit.md`、Issue 278)。
naming だけがアーカイブ済みも判定するのは、ADR-0026 の `lifecycle-*` の候補検出が `isArchived` を入力にするためで、
`class-undeclared` まで判定する意図は記録に無い。

2026-10-17 の監査では、アーカイブ済み 44 件のうち 29 件が `class-undeclared` だった。
これらは ADR-0014 の `naming-*` トピックを要求する前にアーカイブされたか、`archived-2026` の整理(`docs/repo-lifecycle.md`)で
`naming-*` を付けずにアーカイブされたものである。

アーカイブすると description と topics も読み取り専用になる(`docs/repo-lifecycle.md` の「Deprecate-then-archive チェックリスト」。
API での書き込みの実測は未確認)。宣言を付けるには、アーカイブの解除と再アーカイブが要る。
直す手段が状態の往復しか無い要求は、監査が出し続けても収束しない。

## Decision

### D1: アーカイブ済みで `naming-*` が 0 個のときは、`class-undeclared` を出さず `not-applicable` にする

`naming-*` が 0 個のアーカイブ済みリポジトリは、`missing` を空にし、verdict を `not-applicable` にする。
`lifecycle_candidates`(`lifecycle-study-candidate` など)は従来どおり計算して残す。

### D2: 他の naming の検査は変えない

- 生きているリポジトリの `class-undeclared` は従来どおり。
- `naming-*` が 2 個以上なら、アーカイブ済みでも `class-ambiguous`。
- `naming-*` が 1 個あれば、アーカイブ済みでもパターンと閉語彙を検査する。

宣言が既にあるものは、宣言と実名の食い違いを検出する意味が残る。欠落だけが、直せない要求になる。

## 先行例

- このリポジトリの `docs/github-audit.md`(「Archived repositories are now included in the account listing」節)。アーカイブ済みを、直されない drift として 5 ドメインで `not-applicable` にしている。本決定は同じ理由を naming の宣言の欠落に延ばす。
- ADR-0026。アーカイブ済みを naming でだけ判定する根拠は `lifecycle-*` の候補検出で、`class-undeclared` ではない。
- `docs/repo-lifecycle.md`。アーカイブ前に description と topics を更新する順序が必須である根拠。

## 却下した案

- **各リポジトリを盲再導出でクラス決定し、解除・トピック追加・再アーカイブする案。** 29 回の状態往復と 29 件の裁定が要る。得られるのは、使われなくなったリポジトリの分類だけで、`lifecycle-*` の候補検出には効かない。
- **`overrides.tsv` に 29 行を足して exempt にする案。** 人が書き足し続ける台帳になる。新しくアーカイブしたリポジトリごとに同じ手作業が要り、監査の穴も残る。

## 見つからなかったもの

- アーカイブ済みリポジトリで、トピックの書き込みが API で実際に拒否されることの実測。書き込みを伴うので、この裁定の時点では行っていない。拒否されなければ、D1 は「直せない要求を外す」ではなく「要求しないと決める」に読み替わるが、結論は変わらない。

## 執行点

この ADR が定義する決定を執行する、今回変更・追加した実体:

- `scripts/github-audit`
- `crates/github-audit/src/domains/naming.rs`
- `crates/github-audit/tests/judges.rs`
- `docs/github-audit.md`
