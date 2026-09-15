# ADR-0012 — プロンプトでの「敵対的レビュー」要求を、先行例接地 + 監査に機構化する

- Status: Accepted
- Date: 2026-09-15
- Issue: No-Issue(grill-me セッション中に発見・裁定)

## Context

依頼者は Claude Code へのプロンプトで、設計判断を含む依頼のたびに
「敵対的レビューにかけて」「先行例・文献を調査してから」という文言を
手書きで足す習慣がある。過去 1 か月(2026-08-16〜09-15、405 セッションの
transcript を対象にした実測)で、この文言を含む依頼は約 20 件あり、
9 月に増加していた。依頼者自身がこの習慣を「毎回書くのが疲れる」と
表現し、常設のシステムプロンプト(`~/.claude/CLAUDE.md`)へ組み込めないか
という相談が本 ADR の起点である。

「文献調査」側は既に `config/claude/CLAUDE.md`「発明する前に先行例を
確認する」節に常設化されている(`docs/claude/global-claude-md.md`)。
ただし義務は「参照するかどうかの判断を明示する」までで、成果物の中に
先行例がどう現れるべきかは定義していない。「敵対的レビュー」側は
`copilot-plan-review.sh`(ExitPlanMode gate、`docs/claude/
copilot-plan-review.md`)が唯一の実装だが、対象は implementation-readiness
(R/S/I/T の 4 boolean)であり、「採った設計判断が確立されたやり方から
理由なく逸れていないか」は見ていない。

そこで、依頼そのものを実装する前に、「プロンプトへの `敵対的レビュー`
指示の常設化」という前提を先行研究で検証した(2026-09-15、background
agent による文献調査)。

## 文献調査の結果(取得日 2026-09-15)

**外部フィードバックのない自己批評は、推論・設計タスクでは改善しない:**

- Huang, Chen, Mishra, Zheng, Yu, Song, Zhou. "Large Language Models
  Cannot Self-Correct Reasoning Yet." ICLR 2024.
  <https://arxiv.org/abs/2310.01798> — 外部信号なしの自己修正は推論
  タスクの精度を改善せず、しばしば悪化させる。
- Kamoi, Zhang, Zhang, Han, Zhang. "When Can LLMs Actually Correct
  Their Own Mistakes? A Critical Survey." TACL 2024.
  <https://aclanthology.org/2024.tacl-1.78/> — プロンプトされた LLM
  自身のフィードバックによる自己修正の成功例は、自己修正に例外的に
  向いたタスク以外に存在しないとする批判的サーベイ。
- Stav, Berlowitz, Orner, Kraus. "When Does Intrinsic Self-Correction
  Help? A Task-Sensitive Analysis." arXiv, 2026-06.
  <https://arxiv.org/abs/2606.23196> — 効くのは「明示的制約との照合」
  「複雑推論の再訪」「戦略比較」の条件下に限られるという task-sensitive
  な結論。
- Chen et al. "The Self-Correction Illusion: Role Relabeling Gates
  Explicit Error Flagging." arXiv, 2026-06.
  <https://arxiv.org/abs/2606.05976> — 誤り内容を変えずに役割ラベルを
  「自分の思考」から「他者」に変えるだけで明示的修正率が大幅に上昇する。
  自己批評の失敗は能力ではなく帰属の問題。

**独立性は同一モデルの複製では得られない:**

- Panickssery, Bowman, Feng. "LLM Evaluators Recognize and Favor Their
  Own Generations." NeurIPS 2024. <https://arxiv.org/abs/2404.13076> —
  自己認識能力と自己選好バイアスに線形相関。
- Bertalanič, Fortuna. "The Cost of Consensus: Isolated Self-Correction
  Prevails Over Unguided Homogeneous Multi-Agent Debate." arXiv,
  2026-04. <https://arxiv.org/abs/2605.00914> — 同一モデル複数体の
  無構造議論は同調・合意崩壊で、隔離された自己修正に劣る。

**常設指示(system prompt)は減衰する:**

- Li, Liu, Bashkansky, Bau, Viégas, Pfister, Wattenberg. "Measuring and
  Controlling Instruction (In)Stability in Language Model Dialogs."
  COLM 2024. <https://arxiv.org/abs/2402.10962> — system prompt の遵守は
  数ターン以内に有意にドリフトする。
- He et al. (Meta). "Multi-IF." arXiv, 2024-10.
  <https://arxiv.org/abs/2410.15553> — 過去ターンで出した指示の遵守が
  ターン数につれ単調に低下する。

**Anthropic 公式ガイダンス:**

- Anthropic. Claude Code Docs, "Best practices."
  <https://code.claude.com/docs/en/best-practices>(2026-09-15 取得)—
  「Bloated CLAUDE.md files cause Claude to ignore your actual
  instructions」と明記。adversarial review の節では、fresh context の
  subagent に diff と受入基準だけを渡す形を推奨し、「gaps を探せと
  言われたレビュアーは健全な成果でも何か報告する」ため報告範囲を
  正確性に影響するものだけに絞れと注記している。
- Anthropic. Claude Code Docs, "Memory (CLAUDE.md)."
  <https://code.claude.com/docs/en/memory>(2026-09-15 取得)— CLAUDE.md
  は「context として扱われ、強制される設定ではない」。確実に実行させ
  たい動作は PreToolUse/Stop hook に置けと明記。
- Anthropic. Claude Platform Docs, "Prompting Claude Opus 5."
  <https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/prompting-claude-opus-5>(2026-09-15 取得)
  — 「do not use subagents to verify or double-check your own work」、
  明示的な double-check 指示はコストのみ増やすとの記述がある。

「文献・先行例を先に確認せよ」という指示自体の効果を直接測った研究は
見つからなかった。最も近いのは CRITIC(Gou et al., ICLR 2024,
<https://arxiv.org/abs/2305.11738>)が示す「ツール接地の批評はツールなし
自己批評に一貫して勝る」という間接的傍証のみである。

## Decision

1. **プロンプトの都度指示を、常設のシステムプロンプト上の「敵対的に
   見よ」という抽象指示に変換しない。** 文献が支持しないだけでなく、
   Anthropic 自身が「抽象的な批判指示は false positive を量産する」と
   警告している。
2. **代わりに、著者(プランを書く Claude)が設計判断ごとに先行例へ
   接地し、成果物(Plan)の中に検証可能な形で残す**。「先行例を調べたか」
   を LLM の自己申告に委ねず、`## 先行例との対比` 節という具体的で
   検証可能な出力形式を要求する(`config/claude/CLAUDE.md` 追記、
   `config/claude/skills/precedent-grounding/SKILL.md`)。
3. **「敵対的レビュー」は、既存の文脈を切った批評者(`copilot-plan-review.sh`
   の lens A)にこの節を監査させる形で機構化する。** 新しい独立 lens を
   立てず既存 lens A に統合し、premium request を増やさない。批評者が
   有効とみなす指摘は「出典を示せる確立されたやり方との理由なき逸脱」
   だけに絞り、出典のない「もっと良い案があるかもしれない」型の指摘は
   対象外とする — CRITIC の知見(ツール/資料接地の優位)と Anthropic の
   注記(報告範囲の限定)の両方に沿う。
4. **形式面(節が存在するか、出典・取得日・差分の各要素があるか)は
   LLM を呼ばない機械 gate で強制する。** `plan-scope-gate.sh` と同型の
   決定論的 judge を新設する(段2、別 PR)。内容面(出典が本当に主張を
   支えるか)は LLM でしか判定できないため lens A に残す。
5. **適用範囲は Plan mode の設計判断に限定する。** 過去の発話には
   「この状態が正当か」を敵対的レビューで検証してほしい、という
   *主張の検証*型の依頼も含まれていたが、これは異なる問題であり本 ADR
   の対象外とする。

## Alternatives considered

- **CLAUDE.md に「常に敵対的に見よ」という抽象指示を書く** — 依頼の
  文言をそのまま常設化する最も素直な案だが、文献(Huang, Kamoi)が
  同一コンテキストでの自己批評は推論・設計タスクで改善しないと示し、
  Anthropic 自身も抽象的な批判指示を推奨していない。棄却。
- **批評者(Copilot critic)にネットワークを与えて自ら先行例を検索
  させる** — 独立性は上がるが、`docs/claude/copilot-plan-review.md`
  が既に確立した「read-only と network-safe は別の境界」(prompt
  injection 経由の外部送信リスク)を破る。著者が接地し批評者が監査する
  形で同等の効果を、境界を破らずに得られるため採らない。
- **lens A ではなく新規の独立 lens(例: lens P)を追加する** — 観点の
  独立性は上がるが、ラウンド 1 が並列 3 本になり premium request が
  セッションあたり 1 回増える。先行例接地は lens A が既に担う「前提の
  誤り」「既存パターンとの矛盾」と同軸のため、統合で十分と判断した。
- **形式検査を plan-scope-gate.sh に経路C として追加する** — hook
  登録の変更が不要で selftest 基盤を共用できる利点はあるが、
  `docs/claude/scope-inventory.md` と該当 skill が「スコープ」に特化した
  設計文書になっており、先行例接地の検査を混ぜると両方の説明が濁る。
  ADR-0007 の「遡及的な一括リネームはしない」の精神に沿い、新規 hook
  として独立させる。

## Consequences

- 全ての Plan は `## 先行例との対比` 節、または非自明な設計判断が
  ないことを示す免除行(`先行例: 該当なし — <理由>`)のいずれかを
  持つことが機械的に強制される(段2 実装後)。
- 依頼者はもはや依頼文に「敵対的レビュー」「文献調査」を手書きする
  必要がない。書いても害はないが、常設の仕組みと重複するだけになる。
- copilot-plan-review の premium request 消費・ラウンド数・gate
  severity は変更しない(lens A への統合のため)。
- 「主張の検証」型の依頼への機構化は本 ADR の対象外として残る
  (段1 PR の非スコープ節、wrap-up inbox 参照)。
- モデル世代が進みプロンプティングのベストプラクティスが変わった場合
  (本 ADR が引く Anthropic ドキュメントの Opus 5 系の注記のように)、
  この設計もその都度再検証が必要。`docs/claude/precedent-grounding.md`
  に生きた設計理由として残し、ADR 本文は決定の記録として不変に保つ
  (ADR-0008)。

## Verification

- `bash config/claude/hooks/copilot-plan-review.sh --selftest` が
  lens A 拡張後も green であること。
- `hms .` 適用後、新セッションで `~/.claude/skills/precedent-grounding/
  SKILL.md` と `~/.claude/CLAUDE.md` の追記行が見えること。
- 段2(形式 gate)実装後、節のない Plan が ExitPlanMode で deny される
  こと。
