# ADR-0000(起草中) — 既存手段の前倒し接地と、決定論への昇格導線

- Status: Accepted
- Date: 2026-09-28
- Issue: No-Issue(`/grill-me` セッション中に発見・裁定)

## Context

依頼は「同じことをフルスクラッチではなく既存の枯れた技術で実現できないか」
「非決定論的な処理(典型的には LLM による処理)を決定論的な処理に置き換え
られないか」という 2 つの問いを、気付いた・思い出したときに提起するのでは
間に合わないので日々の開発フローに埋め込みたい、というものだった。

`/grill-me` セッションで調査した結果、この 2 問(以下 Q1: 既存手段か自前か、
Q2: LLM か決定論か)を機械的に問うているのはこのリポジトリで `ExitPlanMode`
の `plan-precedent-gate.sh`(形式)と `copilot-plan-review.sh` の lens A
(内容)だけで、しかも Q1 のうち「新しい機構を追加するかどうか」の判断
(selection-grounding の重い欄が発火する `Dn`)しか対象にしていなかった。
Plan mode を経ない作業(直接編集・小修正・grill セッション)ではどちらの
問いも一切発火しない。Q2 に至っては一般原則もゲートも無く、ADR-0015
(github-audit の監査ノードは LLM を呼ばない)と ADR-0012 Decision 4
(形式は決定論 gate、内容は LLM に委ねる)という局所的な先例があるのみ
だった。つまり「気付いたときでは間に合わない」は構造的に正しい —
発火点が Plan 確定の瞬間 1 つしかなく、Q2 には発火点そのものが無い。

グリルで、Q1 と Q2 は撤収コストの非対称性から異なる方式を要することが
判明した。Q1(自前実装を既存手段に置き換える)は自前コードに依存が生える
ほど撤収コストが上がる一方、Q2(LLM/散文の手順を決定論スクリプトに置き
換える)は散文がそのまま仕様書として残るため昇格コストが小さい。この
非対称性から、Q1 は「着手の瞬間に必ず問う前倒し型」、Q2 は「安定の兆候が
出たときに問う昇格型」という異なる時間軸を採用する。

## Decision

### D1: Q1 は「新しい道具・単位の誕生」を発火点に前倒しで問う

Q1 を問う機械的な発火点を、全ファイル編集や行数の閾値ではなく「新しい
道具・単位の誕生」(shebang 付き新規ファイル、`bin/scripts/hooks/cmd`
配下の新規ファイル、パッケージマニフェストの新設)に絞る。これは Plan
mode の有無にもプロジェクトにも依存せず決定論で検出できる、最も早い
機械的な着手点である。

本命: なし — グリルで発火点候補(Issue 起票時 / Plan gate 単独 / 依頼受領時
/ 新規ファイル作成時)を同じ軸で比較して選んだ。
対抗馬: UserPromptSubmit で毎回注入(確実だがノイズが多く形骸化する)、
Plan gate 単独(Plan を経ない作業を素通しする)。
外した候補: ファイル冒頭コメントへの記録 — 感触で外した(定型文が全
ファイルに増殖して腐る予感)。分析ではない。

先行例: Dan McKinley, "Choose Boring Technology", 2015-03-30
https://mcfunley.com/choose-boring-technology (取得 2026-09-28)
差分: 一致 — 「innovation token は新しい技術を採る瞬間に消費される」を、
その瞬間(道具の誕生)に機械的に問う形で採る。
軸: 検出のみ — フルスクラッチ自体を表現不可能にはできない(Bash heredoc
でファイルを作る経路は残る)ため、着手時 deny + PR 時 diff 検査の二重
検出に留める。

### D2: Q1(前倒し型)と Q2(昇格型)を非対称に扱う

LLM・散文による処理は探索期には許容し、安定の兆候(規範違反の再発、
コードブロックの逐語反復実行)が出たときにだけ決定論スクリプト・gate へ
昇格させる。昇格しても散文は「意図とフォールバック手順」として残す —
スクリプトが失敗したとき LLM が散文に従って迂回できるようにするため。

先行例: Erik Schluntz & Barry Zhang (Anthropic), "Building effective
agents", 2024-12-19
https://www.anthropic.com/engineering/building-effective-agents
(取得 2026-09-28)
差分: 異なる — 先行例は「最初から最も単純な(決定論的な)解を選び、必要
なときだけ複雑化する」ことを説く。本決定は skill の散文のような探索期の
LLM 処理をいったん許容し、兆候で昇格させる時間軸を足した(散文が仕様書
として残り、昇格コストが小さいため)。
軸: 還元 — 昇格した仕組みは「LLM が毎回散文を解釈する」より安い手段で
同じ仕事をするときだけ残す。

裏取り(散文とスクリプトの共存という形そのもの): Dan Slimmon,
"Do-nothing scripting: the key to gradual automation", 2019-07-15
https://blog.danslimmon.com/2019/07/15/do-nothing-scripting-the-key-to-gradual-automation/
(取得 2026-09-28)。手順(散文)と自動化ステップが同じ足場に共存し、1
ステップずつ自動化する形をそのまま採る。

### D3: 共通語彙 `既存手段:` 行を新設し、Plan・着手時・PR 本文の 3 箇所で共有する

Q1 の判定を 3 値の閉語彙(`採用|拡張|自前`)にし、`自前` のときだけ
`却下:` または `探索:` を必須にする(「簡単なので自前」を通さない)。
`## 先行例との対比` 節の重い欄(`本命:`/`対抗馬:`)を持つ `Dn` にこの行を
追加で必須化する。

先行例: リポジトリ内 `config/claude/skills/selection-grounding/SKILL.md`
§3(重い欄の閉語彙設計)(取得 2026-09-28)
差分: 一致 — 既存の重い欄と同じ「閉語彙 + 発火条件付き必須化」の形を
そのまま踏襲する。
軸: 表現不可能 — 閉語彙 > 自由記述+事後 lint。

### D4: 成果物の段階(prose/scripted/gated)は宣言させず実体から導出する

「この skill は今どの段階か」を frontmatter 等に宣言させる案を採らず、
検出器が実体(スクリプト・hook 登録の実在)を直接見て判断する。

先行例なし: このリポジトリの ADR(0012・0015・0035)、skill frontmatter、
GitHub 上の "automation maturity level" 宣言スキーマを "maturity"
"stage" "declared" のキーワードで一次情報の範囲まで探したが、宣言
スキーマの先行例は見つからなかった(SRE 本 Ch.7 の 5 段階は分類であって
宣言スキーマではない)。
軸: 表現不可能 — 単一正本 > 複写+同期。宣言欄を作ると実体とのズレという
不正状態が表現可能になる。

## Alternatives considered

- Q1 と Q2 を同じ段階モデル(まず自前で作り、安定したら既存手段に
  置き換える)で扱う。棄却 — D2 の非対称性の理由(撤収コストと昇格
  コストの非対称)によりQ1側の後回しは自前依存を育ててしまう。
- 新しい `ExitPlanMode` gate を追加する。棄却 — 既存の `plan-precedent-
  gate.sh` に加算するだけで足りる(ADR-0035 D3 と同じ「既存の器に載せる」
  判断、還元性の軸)。

## Consequences

- `config/agents/AGENTS.md` に 1 節追加(Q1 前倒し・Q2 段階モデルの原則、
  `既存手段:` 語彙の参照先)。
- `config/claude/CLAUDE.md` に形式検査の配線を 1 節追加。
- `config/claude/skills/{precedent-grounding,selection-grounding,
  pr-description,skill-gardening}/SKILL.md` に `既存手段:` 文法・散文
  保持規約を追記。
- `config/claude/hooks/plan-precedent-gate.sh` に `既存手段:` の必須化
  検査を追加(本 PR で実装、後述の執行点)。形式(記載の有無)のみを
  検査し、「自前を選ぶべきだったか」という内容判断は
  `copilot-plan-review.sh` の lens A へ拡張する余地を残す(後続段。
  ADR-0035 が同じ順序で lens A 拡張を後続段に送った先例に倣う)。
- 後続の stacked PR(段2〜4)で Q1 の Write 時 gate・PR 本文検査(pr-gate
  `G_prior`)、Q2 の記録機構(`Target:` 検査・コマンドハッシュログ・
  gate イベント記録)、日次検出器を実装する。

## 執行点

- `config/claude/hooks/plan-precedent-gate.sh` — D3 の執行点(重い欄を
  持つ `Dn` への `既存手段:` 必須化、selftest 追加)。

## Verification

- `config/claude/hooks/plan-precedent-gate.sh --selftest`(重い欄あり・
  `既存手段:` 無しの `Dn` が deny、ありは pass)。
- `nix fmt`。
