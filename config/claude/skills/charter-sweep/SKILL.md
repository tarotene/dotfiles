---
name: charter-sweep
description: github-audit-charters が報告した drift を入力に、LLM で全 drifted リポジトリの README charter を一括起草し、1 回の一括レビュー(GO/修正/除外/exempt)を経て一括適用する手順。charter 一括整地・drift まとめて直す・charter 一括適用・全リポ charter 対応・charter-sweep、といった依頼で使う。bulk charter remediation, sweep drifted repos, apply charter across repos、といった英語の文脈でも使う。1 リポだけを対話で適合化する場合は repo-charter を使う — こちらは横断監査の findings をまとめて消化する側。
---

`github-audit-charters` は読み取り専用で drift を報告するだけで、直すのは
`repo-charter` の 1 リポずつの対話型インタビューしかない。ADR-0013 導入時点
のように**全リポが drifted**な状態では、「次に触るタイミングで個別 retrofit」
は事実上収束しない。このスキルは、監査の findings を入力に LLM が全リポの
charter を一括起草し、人間の確認は 1 回の一括レビュー(表)に絞ることで、
`repo-charter` インタビューの質(特に低確信リポでの構造的な気づき)を落とさず
に収束させる。設計根拠は `docs/claude/charter-sweep.md`、ADR-0013 Amendment
を参照。

`repo-charter` との役割分担: このスキルは README/description/topics の
**起草と適用**をまとめて回す。charter スキーマ自体の定義・見出しリテラルは
`repo-charter` SKILL.md §2 を正本として常に参照する(二重定義しない)。

## 1. 入力の取得

セッション冒頭で `github-audit-charters --json` を**新規実行**する
(state の `ledger.json` はいつ生成されたか分からないため読まない)。
`verdict=drifted` のリポと `missing` の一覧が作業リストになる。

## 2. 一括起草

drifted リポごとに background subagent へ委譲する。各 subagent は
`gh api`(README raw / description / topics / ファイルツリー /
`CONTEXT.md`・`vision.md` があれば内容 / open Issue 一覧)だけを読み、
**clone しない**。

- 起草するのは `missing=` に挙がった項目のみ(最小差分)。ただし
  purpose 文・`## Scope`・`## Issue litmus` のいずれか 1 つでも欠けている
  場合は、3 点を矛盾なく整合させた 1 セットとして起草する(purpose 文だけ
  直して Scope と噛み合わなくなる、という事故を避けるため)。
- README のスキーマ・見出しリテラル・GitHub メタデータ反映コマンドは
  `repo-charter` SKILL.md §2〜3 のテンプレートをそのまま使う。

## 3. 低確信フラグ(起草しないレーン)

次のいずれかに該当するリポは起草せず、「`repo-charter` の個別インタビュー
送り」として一括レビュー表に列挙するだけにする:

- リポジトリ名と中身から読み取れる責務が食い違う
- open Issue が、そのリポジトリの目的そのものを争っている(何を作るか自体が
  未決着)
- 中身(README・コード構成)から目的文を一意に確定できない(空・スタブ
  リポを含む)

これらは `docs/claude/repo-charter.md` の pilot retrofit で改名級の構造
問題が見つかった実績がある領域であり、機械起草に任せず人間の対話に残す。

## 4. 一括レビュー表と GO 確認(1 回だけ)

drifted リポ全体を 1 枚の表で提示する:

| repo | 起草した purpose 文 | Scope 要約 | Issue litmus 判定問 | close 候補 Issue | 処分案 |
|---|---|---|---|---|---|

低確信フラグのリポは「起草した purpose 文」欄を空にし、処分案を
「repo-charter へ送る」と書く。ユーザーはリポ単位で **GO / 修正 / 除外 /
exempt** を返す。確認はこの 1 回だけで、GO 後は項目ごとに止まらない
(wrapup-chores と同じ「一括 triage → GO 1 回 → 一括処理」の型)。

## 5. Issue 棚卸し(GO 対象リポのみ)

起草時に、そのリポの open Issue を新しい Issue litmus の棄却例へ照らし、
合致するものを表の「close 候補 Issue」欄に挙げておく。**close するのは
GO が出た後のみ**。close するときは、合致した棄却例を名指しするコメントを
必ず付ける(理由なし close は禁止 — `repo-charter` SKILL.md §6 と同じ作法)。

## 6. 一括適用(GO 分のみ、リポごとに)

1. scratchpad へ shallow clone(`git clone --depth 1`)
2. 作業ブランチを切り、README を編集
3. commit → push → `gh pr create`(そのリポジトリの PR 本文規約に従う。
   このリポジトリ自身が対象なら `pr-description` スキルの 5 節スケルトン)
4. merge を確認する(ruleset で CI 待ちになるリポは PR を残したまま
   metadata 反映を保留し、最終報告に「metadata 保留」として明記する —
   黙って直接 push で反映しない)
5. merge 確認後、`gh repo edit --description "<purpose 文と一字一句一致>"
   --add-topic <topic>...` でメタデータを反映する(README が正本、
   description は鏡 — 順序が先に README merge、後に description なのは
   このミラー関係から必然)

## 7. exempt 処分

一括レビューで「このリポには charter を持たせない」と裁定されたリポは、
`$XDG_CONFIG_HOME/github-audit-charters/overrides.tsv` に
`<repo>\texempt` を追記する(監査コマンドの既存の免除機構、
`docs/github-audit-charters.md` 参照)。

## 8. self-verify

最後に `github-audit-charters` を再実行し、次のいずれかで全リポが
説明できることを確認して報告する:

- `ok`(今回適用済み)
- `exempt`(今回除外)
- 依然 `drifted` だが「`repo-charter` へ送った」「metadata 反映保留」の
  いずれかとして最終報告に明記されている

## 9. 注意

- private/company リポジトリ名は、このリポジトリ(公開)の成果物・
  会話ログ以外の永続物に書かない(`docs/claude/public-publish-guard.md`)。
  一括レビュー表はそのセッション内限りで、docs には残さない。
- 起票済みのメタデータ(GitHub description/topics)は `gh repo edit` の
  即時反映であり、README の PR merge を待たずに実行しない。
