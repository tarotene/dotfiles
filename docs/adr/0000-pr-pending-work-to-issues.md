# ADR-0000 — PR 本文の人待ちチェックボックスを廃し、人の確認を後続 Issue へ払い出す

- Status: Accepted
- Date: 2026-09-30
- Issue: No-Issue(セッション内の会話から直接起票)

## Context

社内の別リポジトリ(このリポジトリの対象外)で観測された実例(2026-09-30):
ある PR が、ホスト側のテストは通した上で `## Test plan` に次の体裁の項目を
残したままマージされた。

```
- [x] cargo test — 実行済み
- [x] pre-push フック通過
- [ ] 実機での受入確認は担当者の実施時に確認(本PRはホスト側の修正のみ)
```

直近マージ済みの同一作者の別 PR でも同型(未チェック複数件)が観測された。
このフローは「人の確認が終わればチェックが埋まりマージできる」という体裁を
取るが、GitHub 自身は未チェック task list でマージを止めない(GitHub Docs,
"About tasklists",
<https://docs.github.com/en/get-started/writing-on-github/working-with-advanced-formatting/about-tasklists>、
2026-09-30 取得 — issue の tasklist について記述するが PR のマージ阻止には
触れていない)。マージを実際に止めるのは第三者アクション(`mheap/
require-checklist-action`, <https://github.com/mheap/require-checklist-action>、
"fails a pull request if there are any incomplete checklists in the issue
body and/or comments"、2026-09-30 取得)で、観測した実例のリポジトリには
それが導入されていなかった。つまり現行フローは慣行であって機械強制では
なく、実際には「確認者の予定が PR の寿命を決める」だけの結果になっていた。
base が進むほど rebase 負債が積み、確認が遅れるほど PR は古くなる。

このリポジトリには既に、人手の残タスクを後続 Issue に払い出す仕組み
(`handoff` スキル、残タスク1件 = 子Issue1件)がある。ただし発火はユーザー
の中断指示時のみで、PR 作成一般には及んでいない。また PR 本文の `##
要確認` 節(`pr-description` スキル §6、`pr-confirm-guard.sh` が機械検査)
は、逆に「人間でなければ実行できないブロッキング項目を PR 本文に手順付き
で残す」ことを公認しており、上記のような未チェック task list の温存を
禁じていなかった。

## Decision

### D1: 規範は共有 canon 1 箇所に置く

`config/agents/AGENTS.md`「実装タスクの完了定義」に「PR 本文に未チェックの
task list を残さない」「人の確認が要る残作業は後続 Issue に払い出す」の
2項目を追記する。Claude Code 側の配線は `config/claude/CLAUDE.md`、Codex
側は `config/codex/AGENTS.codex.md` に、それぞれ1文だけ足す(内容の複写は
しない)。

### D2: `## 要確認` を「手順を書く節」から「Issue へのポインタ専用の節」に転換する

`pr-description` スキル §1 のスケルトンを改め、`## 要確認` には払い出した
後続 Issue への参照だけを `- #N — <一言>` の形で書く。手順・完了条件・
ブロッキング理由は Issue 本文(`handoff` スキル §5 の Human テンプレ:
`## やること` / `## なぜ人手か` / `## 手順` / `## 完了条件`)に持たせる。
`handoff` は中断ハンドオフという別の発火条件を持つ仕組みだが、Issue 起票の
書式(1タスク=1Issue、`handoff:human` ラベル)はそのまま再利用する——ラベル
は対象リポジトリに存在するときだけ付け、無ければ新設しない(会社リポでの
ラベル管理を増やさない)。親 Issue は、この PR が `Closes` する Issue が
あればそれを使い、無ければ親無しで単独起票する。

### D3: 機械強制は既存の `pr-confirm-guard.sh` を拡張して担う

判定を「要確認各項目の3要素(理由・手順・完了確認)AND」から次の2つに
置き換える。

```
(i)  本文全体に未チェックの task list(`- [ ]`)が1つでもあれば違反
(ii) `## 要確認` の各項目に Issue 参照(#N または issues URL)が無ければ違反
```

`pr-gate.sh`(Stop hook)に新しい `G_*` は足さない——投稿後の検出になり
表現不可能性で劣る。新規 guard スクリプトも作らない——判定エンジンの複製で
単一正本が割れる(ADR-0035 D1)。Codex には `config/codex/hooks/
pr-confirm-guard.sh` を、`pr-title-guard.sh` (Codex adapter) と同型の
薄い adapter として足す。

既存手段: config/claude/hooks/pr-confirm-guard.sh — 拡張: 同ファイル
(本文抽出・節切り出し・項目分割のエンジンをそのまま使う。GitHub Action
`mheap/require-checklist-action` は方向が逆(人がチェックするまでマージを
止める道具)で不採用——感触で外した、分析ではない)。

### D4: 機械強制は全リポジトリで発火させ、owner スコープを持たない

`pr-confirm-guard.sh` の既存の owner スコープ(`tarotene/*` 限定、
`pr-title-guard.sh` の ADR-0031 D4 を流用していた)を撤去する。D4 の根拠
「squash title = main の履歴という、tarotene 配下の merge 設定に依存する
契約」は本規則には当たらない——本規則は「自分が書く PR 本文に人待ち作業を
残さない」という書き手側の規律であり、`attribution-guard.sh`(自分の投稿
本文の規律、owner スコープを持たない)と同じ類型である。ADR-0031 D4 自体は
改めない(`pr-title-guard.sh` は従来どおり `tarotene/*` 限定)。

### D5: 本文タグ型の例外は設けない

未チェック task list の禁止に `Draft` / `Handoff: #N` のような本文タグ型の
例外は作らない。escape hatch は既存の環境変数 `PR_CONFIRM_GUARD_ALLOW=1`
のみ。正しい代替(後続 Issue への払い出し)は常に実行可能であり、タグ型の
例外を許すとチェックボックスと同じ「本文に人待ちを残す」抜け道が復活する。

### D6: 判定の対象は「未チェック task list の存在」と「要確認の Issue 参照」であり、内容の妥当性は判定しない

払い出した Issue が実在するか、その手順が妥当かは機械検査しない
(`G_visual` が「証跡の有無」までに留め「対比として十分か」を検査しない
のと同じ二層分担)。内容の正しさは `pr-description` スキル(LLM の判断)の
責務にする。

## Alternatives considered

- **`pr-gate.sh`(Stop hook)に新判定 `G_task` を足す**: PR 投稿後の検出に
  なり、未チェック task list が一度は GitHub 上に存在してしまう(D3 参照)。
  allowlist も `tarotene/dotfiles` のみで、`tarotene` 以外が owner の対象
  リポジトリを最初から捉えられない。不採用。
- **GitHub Action(`mheap/require-checklist-action` 系)の導入**: 方向が
  逆——「人がチェックするまでマージを止める」道具であり、「人待ちの
  チェックボックスを本文に存在させない」という本決定の狙いに合わない。
  対象リポジトリごとの workflow 追加も要る。不採用。
- **owner スコープを維持したまま `tarotene/*` に限定する**: 依頼の動機で
  ある「自分が owner を tarotene 以外に持つリポジトリでの PR」を捉えられず、
  規則が空文になる。不採用(D4)。

## Consequences

- `config/claude/hooks/pr-confirm-guard.sh` の判定基準・owner スコープ・
  selftest を刷新する。
- `config/codex/hooks/pr-confirm-guard.sh` を新設し、`home/modules/
  claude.nix` に Codex 版の登録を追加する。
- `config/claude/skills/pr-description/SKILL.md`・`cases.md` を更新する。
- `docs/claude/pr-confirm-guard.md`・`docs/claude/pr-description.md` を
  更新する。
- Copilot 版 adapter は作らない(依頼は Codex のみ)。
- `docs/claude/pr-gate.md` の運用表の `PR_GATE_MAX_BLOCKS` 既定値の陳腐化
  (`5` と書かれているが実際は `6`)は本決定と無関係のため対象外とし、
  wrap-up inbox に別途記録する。

## 執行点

- config/claude/hooks/pr-confirm-guard.sh
- config/codex/hooks/pr-confirm-guard.sh
- home/modules/claude.nix
- .github/workflows/ci.yml

## Verification

- `bash config/claude/hooks/pr-confirm-guard.sh --selftest`
- `bash config/codex/hooks/pr-confirm-guard.sh --selftest`
- `shellcheck -S error config/claude/hooks/pr-confirm-guard.sh
  config/codex/hooks/pr-confirm-guard.sh`
- `scripts/decision-colocation-check --selftest` および本 ADR 自身の導入 PR
  で CI の `decision-colocation-check` required check が green になること
  を自己適用の実測として確認する。
