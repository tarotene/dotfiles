# ADR-0008 — 記録の器の選択規約

- Status: Accepted
- Date: 2026-09-09
- Issue: #122

## Context

このリポジトリには判断・調査を書き残す器が既に 4 種類ある。

- `docs/adr/` — ADR(ADR-0001〜0007)
- `docs/claude/<name>.md` — 個々の Claude Code hook/skill の設計と根拠。
  ADR-0007 §5 が「1 hook = 1 doc の厳密対応は強制しない」等の対応原則を
  定めている
- `docs/` 直下の Investigation records(例: `ime-chrome-diagnosis.md`)
- `research` スキル — 高信頼な一次情報に対する調査結果を Markdown で残す

どれをいつ使うかの優先順位は未裁定だった。stacked PR 運用(#124)を検討する
過程で、GitHub ネイティブの Stacked PR 機能が public preview であることや
`gh-stack` 拡張の open issue 一覧のような**時間で腐る事実**と、「素の `--base`
+ `gh stack link` を使う」という**腐らない裁定**を同じ文書に混在させると、
preview が GA した瞬間に文書全体の書き直しが要る、という具体的な困りごとが
出た。

外部の一次情報を調査したところ、「新しい判断が出たとき、A(ADR)/B(設計文書)
/C(調査記録)のどれに書くか」という**器の間の優先順位規約そのもの**は、
Michael Nygard の ADR 原典・adr.github.io・AWS Prescriptive Guidance・
Google Cloud Architecture Center・Malte Ubl の "Design Docs at Google" の
いずれにも存在しないことが判明した(2026-09-09 時点の一次確認)。文献が
与えるのは各器の**適格条件**だけで、器の間の decision procedure は
このリポジトリ自身で決めるしかない。

確定した事実(一次確認、取得日つき):

- Michael Nygard, "Documenting Architecture Decisions"(2011-11-15,
  https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions
  、取得 2026-09-09): 「Each record describes a set of forces and a
  single decision in response to those forces.」「One ADR describes one
  significant decision for a specific project.」1 ADR = 1 決定。
- AWS Prescriptive Guidance
  (https://docs.aws.amazon.com/prescriptive-guidance/latest/architectural-decision-records/adr-process.html
  、取得 2026-09-09): 「When the team accepts an ADR, it becomes
  immutable. If new insights require a different decision, the team
  proposes a new ADR.」ADR は accepted 後 immutable、覆すときは新 ADR で
  supersede。
- Microsoft Azure Well-Architected Framework
  (https://learn.microsoft.com/en-us/azure/well-architected/architect-role/architecture-decision-record
  、`ms.date: 2026-04-10`、取得 2026-09-09): 「Avoid making decision
  records design guides. If more justification or design ideation is
  available, provide a link to a document as supplemental material, but
  the decision must be clear and stand alone without that material.」
  決定は ADR に、設計・裏付けの詳細は別文書に置いてリンクする。
- GitLab Handbook, "Architecture Design Workflow"
  (https://handbook.gitlab.com/handbook/engineering/architecture/workflow/
  、取得 2026-09-09): 「Design documents get constantly updated with new
  insights and knowledge, after every iteration」「Team members often
  write immutable ADRs. If a decision has to be changed, we can note
  that it has been superseded and create a new ADR with the new
  decision. This helps to reduce the amount of work required to keep a
  design doc up-to-date.」設計文書は living document、ADR を immutable に
  保つことがその更新コストを下げる。
- joelparkerhenderson/architecture-decision-record(v3.2.0、README
  updated 2025-05-29、取得 2026-09-09): ADR にしない基準として「decisions
  that are not about architecture, or are tiny such as minimal-risk or
  self-contained or single-developer, or are already fully covered
  elsewhere such as by standards or policies or documentation, or are
  temporary such as workarounds or proofs of concepts or experiments」
  を挙げる。同時に「Timestamps: … especially important for aspects that
  may change over time, such as costs, schedules, scaling, and the
  like.」— 時間変化する事実には日付を打つべきとも述べる。
- Olaf Zimmermann, "ADR = Any Decision Record? Architecture, Design and
  Beyond"(2021-04-23、更新 2022-09-06、
  https://ozimmer.ch/practices/2021/04/23/AnyDecisionRecords.html
  、取得 2026-09-09): personal/managerial な決定まで記録対象を広げる立場を
  示す。
- RFC 7322, "RFC Style Guide"(H. Flanagan, S. Ginoza, IAB, 2014-09、
  https://www.rfc-editor.org/rfc/rfc7322.txt
  、取得 2026-09-09)§4.8.6.1: 「If a dated URI (one that includes a
  timestamp for the page) is available for a referenced web page, its
  use is required.」「Note that URIs may not be the sole information
  provided for a reference entry.」日付付き URI が使えるなら使うことが
  必須、URI 単独では参照として不十分。

joelparkerhenderson は "single-developer" な決定を ADR 不適格の例に挙げて
いる。これを字義どおり適用すると、このリポジトリの既存 ADR-0001〜0007 は
(単一開発者の dotfiles における決定である以上)ほぼ全部不適格になる。
一方 Zimmermann は personal/managerial な決定まで記録対象を広げる立場を
示している。既存の 7 本の ADR が既に存在するという事実は、このリポジトリが
暗黙に Zimmermann 側を選択済みであることを意味する。この ADR はその選択を
明文化する。

## Decision

新しい判断・調査が出たとき、次の順で当てはまる器に書く。

1. **時間で腐る事実**(外部サービスの preview/GA ステータス、ツールの
   open issue 一覧、バージョン番号など、時間の経過だけで古くなる具体値)
   → Investigation record(`docs/<topic>.md`)または `research` スキルの
   出力に書く。ADR・設計文書からは**リンクするだけ**で、本文に埋め込まない。
   取得日を明記する。
2. **単一の重要な決定**(Nygard の 5 領域 — structure / non-functional
   characteristics / dependencies / interfaces / construction
   techniques — のいずれかに触れ、覆すコストが高い)
   → `docs/adr/NNNN-<slug>.md`。1 ADR = 1 決定。**単一開発者スコープの
   決定も対象とする**(Zimmermann 側の選択。上記 Context 参照)。
   accepted 後は immutable。覆すときは新 ADR を起こして `Status` に
   `Superseded by ADR-NNNN` と書き、旧 ADR からも新 ADR へリンクする。
3. **1 つの hook/skill/tool の設計根拠**で、今後も更新され続けるもの
   → `docs/claude/<name>.md`(ADR-0007 §5 の対応原則のまま)。living
   document として扱い、重要な決定に触れるなら該当 ADR へリンクする
   (ADR 側からの逆リンクは任意)。
4. 上記のどれにも当たらない(自己完結で影響が小さい、既存の docs で
   カバー済み、一時的な回避策・PoC・実験)
   → 新規文書を作らず、既存の docs かコードコメントに収める。

出典の記載規約(すべての器に共通):

- 一次情報を引いたときは、成果物に**URL と取得日**を残す。取得日はページ
  自体の更新日ではなく「自分が読んだ日」。ページに更新日の記載があれば
  それも併記する。
- URL 単独では参照として不十分。著者・タイトル・(判明する範囲で)公開日を
  併記する。

## Alternatives considered

### 4 つの器を統合して 1 種類にする(棄却)

Investigation record と `research` スキルの出力を ADR に統合する案も検討
したが、Microsoft WAF と Zimmermann がいずれも「決定は単体で読めること」
「時間変化する事実は別文書」を明示しており、統合すると ADR が
"design guide"(Microsoft WAF が避けるべきとする形)になる。棄却。

### 単一開発者スコープの決定は ADR にしない(棄却)

joelparkerhenderson の基準を字義どおり適用する案。既存 ADR-0001〜0007 の
大半が対象外になり、このリポジトリが 5 年近く運用してきた実務と矛盾する。
Zimmermann の立場を明示的に採用することで、既存の運用を裁定として固定する
方を選んだ。

## Consequences

- 新しい設計判断・調査が出たとき、まず「腐るか / 決定か / 設計文書か」の
  順で当てはめる。判断に迷ったら「後で GA/バージョンアップで古くなる具体値
  を含むか」を最初のフィルタにする。
- 既存の `docs/claude/*.md` は本 ADR の対象外(ADR-0007 §5 が引き続き
  正本)。遡及的な移行は求めない。
- `config/claude/CLAUDE.md` の調べ方の規律に出典必須の 1 項目を追加する
  (このリポジトリ限定ではなく全プロジェクト適用)。

## Verification

ドキュメントのみの変更のため実行時検証はなし。`docs/README.md` と
ルート `CLAUDE.md` の Architecture Decision Records 一覧にこの ADR への
参照を追加したことを確認する。
