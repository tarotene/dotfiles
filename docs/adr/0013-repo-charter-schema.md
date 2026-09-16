# ADR-0013 — 全自作リポジトリに README charter スキーマを強制する

- Status: Accepted
- Date: 2026-09-16
- Issue: No-Issue(セッション内で生まれた仕組み化依頼への対応)

## Context

herdr で開いていた複数の個人リポジトリ(公開・非公開を含む)を実地調査
した結果、README の目的文と GitHub description は全リポジトリに存在して
いた。欠けていたのは「この Issue はこのリポジトリに属するか」を判定できる
形式(litmus test)と、それを機械検査できる共通スキーマだった。GitHub
topics も調査した範囲の全リポジトリで未設定だった。

このうち非公開の 1 リポジトリでは、この問題が既に実害化していた。README
の「自動提案は入力を 1 回だけ見て提案し、以降の工程には関与しない」という
言明と、ある open Issue の内容(それと正反対の反復ループ提案)が正面から
矛盾していた(固有名は private/company リポジトリ名を書かない方針
[`docs/claude/public-publish-guard.md`]により伏せる)。判定軸が言語化
されていないと、AI Agent は目指すべき方向性がブレ、人間は何を期待すべきか
ブレて見当外れの Issue を切りかねない — これは単発のミスではなく、
スキーマが存在しない限り再発する構造的な問題である。設計判断の詳細
(スキーマの各要素をこの形にした理由、監査を LLM なしにした理由)は
`docs/claude/repo-charter.md` に記録する。

## Decision

1. **全ての自作リポジトリの README に charter スキーマを持たせる。**
   - タイトル直後の第 1 段落の第 1 文 = 目的 1 文。GitHub の description は
     この文字列のミラーとし、独立した二つ目の要約にしない(正本は README)。
   - `## Scope`(In / Out の箇条書き)。
   - `## Issue litmus`(判定問 + 採用例・棄却例)。
   - GitHub topics を最低 1 つ設定する。
   - 見出しリテラルは英語で固定し、機械検査のアンカーにする。
2. **強制点は 2 つ、Issue 起票時の照合は含めない。**
   - 作成時: `repo-charter` スキル(`config/claude/skills/repo-charter/`)が
     charter インタビュー → README 雛形 → description/topics 反映 →
     自己検証までを一式で行う。
   - 事後: `github-audit-charters`(`scripts/github-audit-charters`、
     `docs/github-audit-charters.md`)が全リポジトリを横断し、決定的な
     6 項目(readme-missing / no-purpose-paragraph / purpose-mismatch /
     no-scope-section / no-issue-litmus-section / no-topics)の drift を
     read-only で報告する。
   - Issue 起票時にリトマス照合を自動で行う hook、および各リポジトリ自身の
     CI 自己検査は、意味判断が絡み LLM なしでは書式検査までしかできない
     ため今回は見送る(スコープ外、第 2 弾候補)。
3. **AGENTS.md は監査対象に含めない。** 方向性の正本は README、AGENTS.md
   は作業規約という別問題であり、同じ検査対象にすると中身を検査できない
   AGENTS.md が形骸化したまま合格してしまう。`repo-charter` スキルは
   AGENTS.md 雛形に charter への参照 1 行を播くのみ。
4. **既存リポジトリへの適合化は一括では行わず、監査の drift 報告に任せて
   順次行う。** 上記の非公開リポジトリは、charter 適合の作業(そのリポジトリ
   自身の PR)を別途行う計画で、この適合作業の過程で名前と実際の責務の
   乖離が判明したため改名も予定している(具体的な旧名・新名はそのリポジトリ
   側の private な変更であり、本 ADR および dotfiles のツリーには残さない)。
   残りのリポジトリは各々を次に触るタイミングで順次適合化する。

## Alternatives considered

- **charter を独立ファイル(`CHARTER.md`)に分離する** — README と正本の
  場所が割れ、Web UI で最初に見る場所と機械検査の場所が一致しなくなる。
  棄却(詳細は `docs/claude/repo-charter.md`)。
- **`## Charter` という単一節に Purpose/Scope/Issue litmus を集約する** —
  検査は単純になるが、既存リポジトリ全部で README の再構成が必要になり
  段階的適合化(Decision 4)と相性が悪い。棄却。
- **Issue 起票時にリトマス照合まで自動化する** — 意味判断が要り、LLM
  呼び出しなしには実装できない。今回は決定的な監査(存在チェック・文字列
  一致)だけに限定し、意味判断が要る照合は第 2 弾に送る。
- **AGENTS.md の有無も監査項目に含める** — 存在チェックしかできず、中身の
  空虚化を検出できないため、監査に「AGENTS.md がある」という誤った安心感を
  持たせるリスクがあると判断し除外。

## Consequences

- `scripts/github-audit-charters`(新規、`home/modules/packages.nix` 経由で
  `~/.local/bin` に配布)、`config/claude/skills/repo-charter/`(新規)、
  `docs/github-audit-charters.md`、`docs/claude/repo-charter.md`(いずれも
  新規)が本 ADR の実装物。
- 本 ADR の時点では、charter スキーマを満たすリポジトリはまだ 0 件であり、
  `github-audit-charters` は所有する全リポジトリを `drifted` と報告する。
  これは想定内の初期状態であり、監査自体の不具合ではない。各リポジトリは
  次に触るタイミングで `repo-charter` スキルによって順次適合化する。
- charter スキーマを変更する場合(見出しリテラルや判定項目の追加・変更)は、
  この ADR を supersede する新しい ADR を起こす(ADR-0008 の規約)。

## Verification

- `github-audit-charters --selftest` — fixture で ok / drifted(複数の
  missing 理由)/ exempt の分岐と、override 解除後に元の drift 判定へ戻る
  回帰を確認。
- `github-audit-charters` を実アカウントに対して実行し、所有する全リポジ
  トリが charter 未適合のため `drifted` と報告されることを確認
  (2026-09-16 時点: total 34 repo(s) — ok=0 drifted=34 exempt=0。この
  ADR が導入するのはスキーマと監査ツールそのものであり、個別リポジトリの
  適合化はここに含まれない)。
- `shellcheck -S error` が `scripts/github-audit-charters` を通る。
- `nix flake check` — 3 ホストとも green。
