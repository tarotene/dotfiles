# ADR-0000 — ADR 番号をローカル連番でなく導入 PR の番号にする

- Status: Accepted
- Date: 2026-09-23
- Issue: No-Issue(`/grill-me` セッション中に発見・裁定)
- Amends: ADR-0008(`docs/adr/NNNN-<slug>.md` という形式だけを定め、
  誰がいつどう採番するか・衝突時にどうするかを一切規定していなかった。
  ADR-0008 の他の Decision(1 ADR = 1 決定、accepted 後 immutable)は
  そのまま有効)

## Context

ADR の採番衝突がこのリポジトリで現に 2 回起きている。

1. **2026-09-19、ADR-0020**(解決済み)— stack 側で
   `0020-ruleset-review-layer-addin.md` を書いている最中に `#227` が main
   に同番号を先に確定させた。`ec16267` + `7a15a16` で `0021` へ手動改番し、
   10 ファイル・約 54 行の参照を追従した。しかし squash された `c3d263c`
   (#224) の commit subject は今も `(ADR-0020)` のままで、main の commit
   履歴と実ファイル番号が恒久的に不一致になっている — 事後修復は完全には
   巻き戻らない。
2. **2026-09-23、ADR-0033**(`#374` で解決)— `#360`(nav-doc) と
   `#365`(machine-state) が 24 秒差でマージされ、両方が `# ADR-0033` を
   名乗った。`ADR-0033` という参照文字列 25 箇所がどちらを指すか判別不能に
   なり、`AGENTS.md` の ADR 一覧から nav-doc 側が一度も書かれないまま
   落ちた。

根本原因は、連番が分散システム(並列 worktree・並列セッション)に置かれた
中央アロケータであること。ADR 番号を検査・採番する自動化は hook・CI・
skill・github-audit のどこにも存在しなかった。

## Decision

ADR 番号を `docs/adr/` 内のローカル連番ではなく、その ADR を導入した PR の
番号にする。

| 番号帯 | 意味 | 検査 |
|---|---|---|
| `n < 0100` | grandfathered 連番(既存の 0001〜0099 帯。新規 ADR はこの帯を
  使わない) | 重複のみ |
| `n >= 0100` | PR-number era。`n` は導入 PR の番号と一致必須 | 全項目 |
| `0000` | 起草中(PR 未作成)。main に載ってはならない | 存在で fail |

起草フローは二相になる: `docs/adr/0000-<slug>.md` を書く → commit → push →
`gh pr create` → `<PR番号>-<slug>.md` へ改番し本文 H1 も書き換える →
commit + push。判定エンジンは `scripts/adr-number-check` に集約し、CI の
required check(`.github/workflows/ci.yml` の `dry-run` job)がこれを呼ぶ。
本 ADR 自身がこのフローの第一適用例であり、`0000-adr-number-by-pr.md` として
起草され、この PR の番号に改番されている。

閾値を `0100` の固定値にしたのは、grandfather 境界を「この PR の番号」の
ような可変値にすると、別セッションが並行して採っている連番 ADR(例:
`ADR-0035`)との調整が必要になり、境界宣言がマージ順に依存してしまうため。
固定閾値なら `0035..0099` が in-flight の連番 ADR 用の緩衝帯として機能し、
一切の調整が不要になる。

## Alternatives considered

- **Issue 番号で採番する(KEP 型)** — 棄却。このリポジトリの
  `AGENTS.md`「Pull request descriptions」節は `No-Issue: <reason>` を
  正式な escape hatch と規定し「番号が欲しいだけの捨て Issue を作るな」と
  明記している。ADR の `Issue:` 欄は実測で `No-Issue(...)` が最頻であり、
  Issue 番号方式は既存規範と両立しない。
- **lock ファイルで採番を直列化する** — 棄却。
  [adr/madr#28](https://github.com/adr/madr/issues/28) で 2020-10-12 に
  提案されたが、提案者自身が「Dev may forget to modify the lock file」と
  限界を明記し、MADR 本家で 6 年間 open のまま未解決。中央アロケータの
  再発明であり問題の構造を変えない。
- **日付ベース命名に移行し番号を捨てる** — 棄却。このリポジトリは
  `ADR-NNNN` 参照を実測 987 箇所(docs 473 / skills 123 / home 106 /
  scripts 87 / hooks 27 / .github 13 / AGENTS.md 100)持ち、参照語彙が
  全滅する。
- **衝突を受容し事後の一括改番コマンドを整備する** — 棄却。衝突が起きる
  前提であり「番号衝突が原理的に起こりえない状態にする」という目的を
  満たさない。

## Consequences

- 新規 ADR の番号が PR 番号と一致するため、番号から作成時期の相対順序が
  読み取れなくなる(PR 番号は ADR 以外の全 PR も消費するため連続しない)。
  既存の ADR 索引(`AGENTS.md`、`docs/README.md`)がこの相対順序を代替する。
- 1 PR に複数の ADR を同時追加できなくなる(実測で `ca1b7c1` (#184) が
  3 ADR を同時追加していた)。複数必要な場合は ADR-0027 の単一チェーンで
  段に分ける。
- `docs/adr/` に `github-audit` のような GitHub API 前提の監査ドメインを
  作らない — 個別 PR の適合性は client 側の自動修正と CI required check の
  仕事であり、audit がそれを見ると「CI が green なのに drift を報告する」
  状態になる(`scripts/github-audit` の `judge_titles` が既にこの原則を
  明記している)。

## Verification

- `scripts/adr-number-check --selftest`
- CI: `.github/workflows/ci.yml` の `dry-run` job が `adr-number-check` を
  呼ぶ(詳細: `docs/claude/adr-numbering.md`)
