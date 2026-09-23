# adr-numbering — ADR 番号をローカル連番でなく導入 PR の番号にする

設計判断の記録: `docs/adr/0000-adr-number-by-pr.md`(Amends ADR-0008)
checker(単一ソース): `scripts/adr-number-check`
サーバ側 required check: `.github/workflows/ci.yml` の `dry-run` job
人間層(利便性): `config/claude/hooks/adr-number.sh`(PostToolUse、段 3)

ADR の採番衝突がこのリポジトリで 2 回起きた(2026-09-19 の ADR-0020→0021、
2026-09-23 の ADR-0033 二重)。原因は連番が分散システム(並列 worktree・
並列セッション)に置かれた中央アロケータであること。lock ファイルのような
「共有セルを守る」対策ではなく、番号を PR 番号にすることで「共有セルその
ものを無くす」——GitHub が原子的に採番するため衝突が構造的に不可能になる
(先行例: rust-lang/rfcs、kubernetes/enhancements の KEP)。

## 番号帯

| 番号帯 | 意味 | 検査 |
|---|---|---|
| `n < 0100` | grandfathered 連番(既存の 0001〜0099 帯) | 重複のみ |
| `n >= 0100` | PR-number era。`n` は導入 PR の番号と一致必須 | 全項目 |
| `0000` | 起草中(PR 未作成)。main に載ってはならない | 存在で fail |

閾値を固定値 `0100` にしたのは、grandfather 境界を「この PR の番号」の
ような可変値にすると、並行して採られる連番 ADR(緩衝帯 `0035..0099`)との
調整がマージ順に依存してしまうため。固定閾値なら調整が一切不要になる。

## なぜ Issue 番号(KEP 型)でなく PR 番号か

KEP は tracking issue の番号をそのまま使う一相方式(リネーム不要)だが、
このリポジトリの `AGENTS.md` は `No-Issue: <reason>` を正式な escape hatch
と規定し「番号が欲しいだけの捨て Issue を作るな」と明記している。実測でも
ADR の `Issue:` 欄は `No-Issue(...)` が最頻であり、Issue 番号方式は既存
規範と両立しない。そのため PR 番号 + 二相方式(起草 → PR 作成後に改番)を
採る。

## 起草フロー(二相)

1. `docs/adr/0000-<slug>.md` を書く(本文 H1 も `# ADR-0000 — ...`)
2. commit → push → `gh pr create`
3. `<PR番号>-<slug>.md` へ改番し、本文 H1 も書き換える
   - 段 3 が着地していれば PostToolUse hook が自動でやる
   - まだなら手動: `adr-number-check --fix <PR番号>`
4. commit + push(この時点で CI が green になる)

`--fix` は `0000-*.md` のリネームだけでなく、既に採番済みの ADR を
別番号へ改番する用途(2026-09-19 の `ec16267` 相当の手動改番)にも使える
——`<file>` を明示すれば任意の `docs/adr/*.md` を対象にできる。

## 判定エンジンの 2 系統

判定項目は適用範囲が 2 系統に分かれる。混ぜると既着地の ADR を現在の PR
番号で誤判定する(例: `ADR-0376` 着地後、ADR を追加しない PR `#380` で
`0376 != 380` となり required check が永久に通らなくなる)。

- **A. リポジトリ全体**(`docs/adr/` を丸ごと走査、PR 番号を使わない):
  番号重複なし / `0000-*.md` なし / ファイル名の番号 == 本文 H1 の番号 /
  全 ADR が `docs/README.md` の索引にリンクされている
- **B. base 差分で新規追加された ADR にのみ適用**(`--base` 必須):
  追加は最大 1 本 / `n >= 0100` なら PR 番号と一致(`n < 0100` の緩衝帯は
  無条件で通過)

CI では push イベント(base/PR 番号が無い)は A のみ、pull_request
イベントは base SHA を追加 fetch した上で A + B の両方を判定する。

## `github-audit` にドメインを作らない理由

`scripts/github-audit` の `judge_titles` は「個別 PR の適合性は client
guard と required check の仕事であり、audit がそれを見ると CI が green
なのに drift を報告する状態になる」と既に明記している(ADR-0031)。ADR
番号の重複はまさにその instance conformance に当たるため、同じ原則で
audit ドメインを作らない。

## スコープ外(意図的)

- 他リポジトリへの播き — 播種先リポジトリは `docs/adr/` を持たず検査対象が
  存在しない。将来必要になったら「検査が配線されているか」の presence
  detection のみを `github-audit` に足す(`judge_titles` と同型)。
- 既存の連番衝突(ADR-0020→0021)のバックフィル — ADR-0008 の immutable
  原則により、事後の一括改番はしない。
