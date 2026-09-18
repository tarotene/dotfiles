# ADR-0015 — 診断を統合 CLI に再編し、決定論ノード+LLM ノードの判断ループにする

- Status: Accepted
- Date: 2026-09-18
- Issue: No-Issue(grill-me セッション中に発見・裁定)

## Context

`github-audit-rulesets`(#130)と `github-audit-charters`(ADR-0013)は、
列挙・ledger・overrides・`--selftest` の基盤を丸ごと複製した兄弟スクリプト
として生まれた。ここへ命名(ADR-0014)・Repo Settings・Renovate 導入状況・
文書正典(ADR-0016)という新しい診断領域を求められ、同じパターンで 3 本
追加すると、基盤の複製が 5 本になり、LLM が横断裁定するときに見る入口
(ledger)も 5 つに分かれてしまう。

さらに、charter-sweep スキル(#180、別セッションが並行して開発)は
「監査の findings を LLM が読み、一括レビュー表を経て一括適用し、merge
まで行う」というループを実装していたが、(a) README の品質が確立された
規範(standard-readme・GitHub 公式 等、ADR-0016 参照)から外れる方向に
劣化し、(b) 人間の裁定(GO)を経ずに PR の merge まで自動で進んでいた。
これは「決定論的な監査」と「LLM による裁定支援」の境界が設計として
明示されていなかったために起きた。

## Decision

1. **「診断」を、決定論ノードと LLM ノードを分離した閉ループとして
   定義する。**
   - 決定論ノード: `github-audit`(統合 CLI)が drift を検出し ledger 化
     する。LLM を一切呼ばない(既存 2 スクリプトの設計原則を継承)。
   - LLM ノード: `github-audit-triage` スキルが ledger の findings を
     読み、人間が裁定しやすい一括レビュー表に整形する。
   - 人間裁定: GO / 修正 / 除外(exempt) / 個別インタビュー送り、を
     リポジトリ単位で 1 回返す。
   - 書き戻し: 裁定結果を正本(GitHub topics・settings・PR)へ反映する。
   - 再監査: 決定論ノードを再実行して収束を確認する。
   - この閉ループの外で LLM が正本を直接書き換えることはない —
     `github-audit-triage` の完了定義は PR 作成までで、merge と merge 後の
     メタデータ反映は人間裁定後に別途行う(#180 の「merge まで自動」を
     ここで明示的に廃止する)。
2. **監査を単一コマンド `github-audit <domain…>` に統合する。** 列挙
   1 回を全ドメインで共有し、ledger・overrides を 1 つに集約する。ドメ
   インは `rulesets` `charters` `naming` `settings` `renovate` の 5 つ
   (charters は ADR-0016 の文書正典を含む)。旧 2 コマンドは廃止し、
   wrapper は置かない(単一ユーザーのツールであり、移行の摩擦より一本化
   の単純さを優先する)。
3. **exempt の意味論はドメイン単位にする。** `overrides.tsv` の書式を
   `<repo>\t<domain>\texempt` とし、`domain` に `*` を置けば全ドメイン
   免除になる。既存の 2 つの overrides ファイルは実装時に手動で新書式へ
   移行する。
4. **ドメイン追加の基準。** 新しい診断領域を追加する条件は、(a) `gh api`
   / `gh repo list` で決定的に取得できる事実であること、(b) 意味判断が
   必要な部分は LLM ノードへ切り出せること、の両方を満たす場合に限る。
   満たさない領域(例: 「この Issue litmus の質は十分か」)は監査に
   加えない。

## Alternatives considered

- **既存パターンのまま兄弟スクリプトを増殖させる**(naming/settings/
  renovate をそれぞれ独立コマンドにする) — 基盤複製が 5 本になり、LLM
  triage が 5 つの ledger をマージする手間を毎回背負う。棄却。
- **LLM ノードに正本への直接書き込みを許す**(#180 の運用) — README
  品質の劣化と、人間裁定なしの merge という 2 つの実害が既に観測されて
  いる。gate(遮断)ではなく triage(裁定支援)として設計し、書き戻しの
  実行点を人間裁定の後に固定する。
- **統合 CLI をサブコマンド形式(`git-<name>` のような分散解決)にする**
  — 本リポの `git-shelve` 等が先行例だが、ledger を 1 つに集約する目的
  には単一実体+引数選択の方が適う。棄却。

## Consequences

- `scripts/github-audit-rulesets` / `scripts/github-audit-charters` は
  廃止され、`scripts/github-audit` に統合される。`docs/github-audit-
  rulesets.md` / `docs/github-audit-charters.md` は `docs/github-audit.md`
  に統合再編される(既存の設計判断の記述は保存する)。
- `~/.claude/skills/{rust,typst,astro}-repo-governance` の 3 スキルは
  `config/claude/skills/` へ移設され、監査基準(settings・renovate)との
  乖離があれば skill 側を修正する(基準の正本は ADR、Closes #151)。
- `github-audit-triage` スキルが新設され、charter-sweep(#180)の設計
  (低確信フラグ・exempt 処分・self-verify)を継承しつつ、完了定義を
  PR 作成までに変更して巻き取る。#180 は close する。
- ドメインを追加・変更する場合は、この ADR を supersede する新しい ADR
  を起こす(ADR-0008 の規約)。

## Verification

- `github-audit --selftest` — 5 ドメイン全てで ok / drifted / exempt の
  分岐を fixture で確認。
- `github-audit --json` を実アカウントに対して実行し、ledger が
  `$XDG_STATE_HOME/github-audit/ledger.json` に単一ファイルとして書かれる
  ことを確認。
- `shellcheck -S error scripts/github-audit` が通る。
- `nix flake check` — 3 ホストとも green。
