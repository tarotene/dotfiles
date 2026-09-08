# scope-inventory — 依頼スコープの脱落を計画段階で塞ぐ

`config/claude/CLAUDE.md` の「複数項目の依頼は要求インベントリで受ける」節、
`config/claude/skills/scope-inventory/SKILL.md`、`config/claude/hooks/
plan-scope-gate.sh`(段 2)の 3 点セットの設計根拠。`pr-description` が skill
と `G_visual` を 1 文書で扱っている前例に倣い、skill と gate を 1 文書に
まとめる。

## なぜこれが必要か

Tracking Issue のような複数項目を含む依頼を丸ごと Plan Mode に投げると、
作業スコープの増大を気にして依頼された範囲を黙って縮小した計画を返してくる
ことが多い(Claude Code に限らない)。狙う失敗モードは 2 つ:

- **計画段階の暗黙縮小**: 要求に含まれる子タスクを計画に載せず、「スコープ外」
  「別 Issue で」節に逃がす。正当な設計判断の見た目をしているためレビューでも
  気づきにくい。
- **見積り膨張による自己検閲**: 規模を理由に要求そのものの妥当性を勝手に
  再交渉する。

対象外: 実行段階の途中打ち切り(既存の「実装タスクの完了定義」と `G_pr` が
担当)、判断の丸投げによる停止。

## 一般規範の再掲では効かない

Anthropic は既にこの処方を Claude Code の system prompt に注入している
(Anthropic, "Prompting Claude Fable 5.1" §Finish the whole task、および
"Prompting Claude Opus 5" §Task scope and over-verification、取得日
2026-09-09、
<https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/prompting-claude-fable-5-1>、
<https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/prompting-claude-opus-5>):

> the scope is the deliverable: don't quietly narrow, widen, or swap it …
> scaling it down is the user's call, not yours.

にもかかわらず日和りは起きる。したがって一般規範を CLAUDE.md に再掲するのは
無価値で、刺すべきは **plan mode 固有の具体化**(要求インベントリ + 閉じた
棄却タグ + 機械検査)だけである。補強として、BAITBENCH
(Thaman らの後継、arXiv:2608.30724、取得日 2026-09-09)は「明示的に禁止しても
ショートカット使用率は 7 種の frontier エージェント平均で 50% 超のまま」と
報告している——指示文だけでは足りないという文献側の裏取り。

## 先行例と、先行例が無い部分

OpenAI Codex の Goals 機能が、upstream ソースレベルで最も近い先行実装を持つ
(`openai/codex`, `codex-rs/ext/goal/templates/goals/continuation.md`、
取得日 2026-09-09):

> "do not redefine success around a smaller or easier task."
> "Optimize each turn for movement toward the requested end state, not for
> the smallest stable-looking subset or easiest passing change."

completion audit は「全ての明示要件・番号項目・成果物ごとに証拠を特定せよ」を
要求する。要求インベントリはこのパターンの移植。一方、**計画そのものを検査
して脱落を突き返す hook** の一次情報は、Anthropic・OpenAI・GitHub Copilot・
Cursor・Aider いずれのドキュメント/upstream にも見つからなかった(段 2 で
新規設計する)。GitHub Copilot は逆に「ユーザーがタスクを小さくせよ」という
方向の公式ガイダンスを持ち、エージェント側にスコープ保持を求める発想はない
(<https://docs.github.com/copilot/how-tos/agents/copilot-coding-agent/best-practices-for-using-copilot-to-work-on-tasks>、
取得日 2026-09-09)。

## 器の選択: 指示文 + skill (+ 段 2 で hook)

`skill-gardening/SKILL.md` の器選択表に従う:

- 「常に成立していてほしい、ごく短い原則」→ global `CLAUDE.md`
  (コンテキスト税がかかる最終手段だが、要求インベントリを書くかどうかの
  判断自体は毎回発生するので該当する)
- 「モデルが読んで従うべき判断知識」→ skill(`gh graphql` の具体クエリ、
  タグの使い分け、`Reference-Only:` の書き方)
- 「機械的に検査・強制できる違反」→ hook(段 2)

`copilot-plan-review.sh` の lens A(スコープ判定)を拡張する選択肢は採らない
——lens A は「非スコープが明確か」を見る観点で、依頼に含まれない隣接事項の
非スコープ宣言を*加点*する。これは今回塞ぎたい失敗そのものと衝突する
(下記参照)。critic はネットワークも `gh` も使えず元 Issue を読めないため、
「インベントリが元 Issue に対して完全か」を LLM 判定でも検証できない。

## 実測が否定した設計: 縮小マーカー起点の検査

当初案は「プラン中の縮小マーカー(『スコープ外』『今回は』『別 Issue』等)を
起点に検査を発火する」だった。`~/.claude/plans/*.md` の過去プラン 327 本
(2026-09-09 時点)を実測した結果、この設計は棄却した:

| マーカー | 出現プラン数 (327 本中) |
|---|---|
| 見出しとしての「スコープ外」 | 58 (18%) |
| 「やらない/含めない/扱わない」系 | 177 (54%) |
| 「将来/後続」 | 145 (44%) |

しかもその大半は正当。依頼に含まれない隣接事項を非スコープと宣言するのは
この repo の確立された作法であり(`docs/claude/plan-view.md` の「スコープ外」
節、`docs/claude/worktree-fresh-base.md` の「スコープ外(別 Issue)」節)、
マーカーは「依頼に含まれていたのに落とした項目」と「依頼に含まれない項目の
非スコープ宣言」を区別できない。`home/modules/claude.nix` が
`copilot-plan-review.sh` の gate 対象 severity を `BLOCKER,MAJOR` から
`BLOCKER` に、ラウンド数を 3 から 2 に絞った経緯(実測 deny 率 69%、review の
価値より摩擦が勝っていたため)を再現するだけになる。

代わりに、要求項目そのものの**実カバレッジ**を検査する方式に切り替えた
(段 2 で実装、詳細はそちらの追記を参照)。

## 3 タグに絞った理由

`Blocked-Upstream:` / `Obsolete:` / `User-Excluded:` の 3 つに絞った。
検討して落とした案:

- **`Conflicts:`(要求同士が両立しない)**: upstream に対応物が無く、
  「両立しない」は機械検査不能な主張で最も乱用されやすい(「両立しない」と
  言えば何でも落とせる)。要求同士の真の衝突は棄却ではなく、その場で
  `AskUserQuestion` で裁定を取るべき事象——除外した「判断の丸投げで停止」の
  裏口を塞ぐ理由と同じ。
- **自由文での棄却理由**: `No-Issue:` / `No-Visual:` と同じ「タグ + 自由文」
  形は一貫性があるが、今回は「規模を理由にするな」が目的で、自由文だと
  言い換えでいたちごっこになる。閉じたタグ集合(`permissionRules` と同じ、
  不足は PR で拡張)を選んだ。

## Issue 単位の `Reference-Only:` — プランレビューの BLOCKER 指摘に基づく設計

初版の gate 設計は、ユーザーが書いた `#N` を無条件に実装要求とみなし、その
子 Issue 全件をインベントリに要求していた。`copilot-plan-review.sh` の
プランレビュー(lens A, id: R1-A-1)がこれを BLOCKER として指摘: 「#10 の設計を
参考に、今回は設定項目 A だけ追加して」のように、参照だけの Issue も同じ扱いに
なり、依頼されていない子項目まで列挙・処分させる誤検知が生じる。**参照と
実装依頼は機械的に区別できない。**

対処として、Issue 単位の宣言を挟んだ: 参照 Issue ごとに「実装対象(→ 全子項目
を処分)」か `Reference-Only: #N — <理由>` のどちらかを 1 行書けば通る。これで
**黙って無視する形だけが deny される**という非対称性が保たれる。副次効果として、
参照した Issue を読んで対象外と判断したこと自体が計画に記録される。

## 対象エージェント

Claude Code のみ。Codex(`~/.codex/`)・Copilot(`~/.copilot/`)には現時点で
グローバル指示文を置く確立されたファイルが存在しない(`config.toml` /
`config.json` を確認済み、取得日 2026-09-09)ため、新設は新サーフェスの立ち上げ
になり今回のスコープに含めない。この判断自体を要求インベントリの語彙で言えば
`User-Excluded:`(ユーザーが明示的に除外)。

## 効果の確かめ方

指示文・skill の効き方は一度きりの `nix build` では検証できない(プロンプトへの
効き方の問題、`docs/claude/global-claude-md.md` と同じ運用)。`hms .` 適用後、
新しいセッションで複数項目の依頼を投げ、`## 要求インベントリ` が自発的に
書かれるかを観察する。弱ければ文面を PR で調整する。
