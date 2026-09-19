---
name: github-audit-triage
description: github-audit(統合 5 ドメイン監査)が報告した drift を入力に、LLM で複数リポジトリの findings を一括起草し、1 回の一括レビュー(GO/修正/除外/exempt)を経て一括 PR 化する手順(ADR-0015 の LLM ノード)。charter 一括整地・drift まとめて直す・naming/settings/renovate 一括対応・全リポ横断で直す・github-audit-triage、といった依頼で使う。bulk remediation across repos, triage audit findings, apply drift fixes across repositories、といった英語の文脈でも使う。1 リポだけを対話で適合化する場合は repo-charter を使う — こちらは横断監査の findings をまとめて消化する側。
---

`github-audit` は読み取り専用で drift を報告するだけで、直すのは
`repo-charter` の 1 リポジトリずつの対話型インタビュー(charters/naming)や
各 `*-repo-governance` スキル(settings/renovate/rulesets)しかない。多数の
リポジトリが同時に drift している状態では、「次に触るタイミングで個別
retrofit」は事実上収束しない。このスキルは、監査の findings を入力に LLM が
複数リポジトリぶんの修正を一括起草し、人間の確認は 1 回の一括レビュー(表)に
絞ることで、個別インタビューの質を落とさずに収束させる。ADR-0015 が定義する
判断ループの LLM ノードはここだけであり、`github-audit` 自体は LLM を呼ばない。

設計根拠は `docs/claude/github-audit-triage.md`。前身の charter-sweep
(#180)の設計(低確信フラグ・exempt 処分・self-verify)を継承しつつ、
完了定義を「PR 作成まで」に変更して巻き取っている — charter-sweep は
merge・merge 後のメタデータ反映まで自動で行っていたが、それは人間裁定を
経ない正本への書き込みであり ADR-0015 のループ設計と矛盾するため廃止した。

`repo-charter` / `*-repo-governance` との役割分担: このスキルは複数リポジトリ
ぶんの**起草と一括レビューの取りまとめ**を担う。README/CONTRIBUTING のスキーマ
定義・見出しリテラル・settings/renovate の基準値は `repo-charter` SKILL.md・
各 `*-repo-governance` SKILL.md を正本として常に参照する(二重定義しない)。

## 1. 入力の取得

セッション冒頭で `github-audit --json` を**新規実行**する(state の
`ledger.json` はいつ生成されたか分からないため読まない)。ユーザーが対象
ドメインを指定していれば `github-audit <domain...> --json` に絞る。
`verdict` が `drifted` または `ungoverned` のリポジトリ×ドメインの組が
作業リストになる。

## 2. 一括起草

drifted な (repo, domain) の組ごとに background subagent へ委譲する。各
subagent は `gh api`(README/CONTRIBUTING/AGENTS.md/CLAUDE.md の raw・
description・topics・settings フィールド・ファイルツリー・open Issue 一覧)
だけを読み、**clone しない**。

- 起草するのは `missing=` に挙がった項目のみ(最小差分)。ただし charters
  ドメインで purpose 文・見出しスキーマ・CONTRIBUTING の Issues 節
  (judging question / Accepted / Rejected、ADR-0017)のいずれか 1 つでも
  欠けている場合は、矛盾なく整合させた 1 セットとして起草する(purpose 文
  だけ直して Scope と噛み合わなくなる、という事故を避けるため)。
- README/CONTRIBUTING のスキーマ・見出しリテラル・禁止事項(時限記述・
  Issue 番号焼き込み・長文弁明の禁止)は `repo-charter` SKILL.md §2〜3 の
  テンプレートをそのまま使う。**品質ガードレールを外して急がない** —
  ここで規範から外れた起草をすると、次の監査サイクルでまた drift として
  戻ってくるだけで何も収束しない。
- naming ドメインの `class-undeclared`/`class-ambiguous`/`pattern-mismatch`
  は、自由裁定ではなく**盲再導出(ADR-0020)**で提案する — subagent には
  対象リポジトリの実際の名前を伏せた状態で README・ファイルツリー・open
  Issue だけを渡し、「本 ADR の語彙・文法のみで命名するなら何と付けるか」
  (クラス + 規範名)を導出させてから、実名を開示して一致度を報告させる。
  表には「導出クラス / 導出名 / 実名 / 一致度 / 処分案」を書く。処分案は
  高一致なら「宣言のみ」、不一致なら「改名 + 宣言」。`naming-codename` を
  提案する場合は、`config/github-audit/codename-registry.tsv`(PUBLIC)
  または `~/.config/github-audit/codename-registry.local.tsv`(PRIVATE、
  dotfiles には書かない)への追記案も併記する。
- rulesets ドメインの `ci-absent` は、リポジトリごとに
  {最小 CI 播種 PR / CI 播種を促す誘導 Issue の起票 / exempt} の三択を
  提案する(ADR-0020)。コードを持つリポジトリは播種 PR、記録・ノート系は
  exempt、判断が割れる場合は Issue 起票を既定の推奨にする。
- settings/renovate ドメインは、対象の `*-repo-governance` スキルの
  `apply-repo-settings.sh` / renovate テンプレートをそのまま適用する提案
  として表に書く。

## 3. 低確信フラグ(起草しないレーン)

次のいずれかに該当するリポジトリ×ドメインは起草せず、「`repo-charter` の
個別インタビュー送り」として一括レビュー表に列挙するだけにする:

- リポジトリ名と中身から読み取れる責務が食い違う(naming クラス裁定が
  改名を要する可能性がある場合を含む)
- open Issue が、そのリポジトリの目的そのものを争っている(何を作るか自体が
  未決着)
- 中身(README・コード構成)から目的文・命名クラスを一意に確定できない
  (空・スタブリポジトリを含む)

これらは `docs/claude/repo-charter.md` の pilot retrofit で改名級の構造
問題が見つかった実績がある領域であり、機械起草に任せず人間の対話に残す。

## 4. 一括レビュー表と GO 確認(1 回だけ)

drifted な組全体を 1 枚の表で提示する:

| repo | domain | 提案内容の要約 | 処分案 |
|---|---|---|---|

charters ドメインは「起草した purpose 文 / Scope 要約 / judging question」、
naming は「提案クラス」、settings/renovate は「適用するテンプレート差分」を
要約欄に書く。低確信フラグの組は要約欄を空にし、処分案を「repo-charter へ
送る」と書く。ユーザーはリポジトリ×ドメイン単位で **GO / 修正 / 除外 /
exempt** を返す。確認はこの 1 回だけで、GO 後は項目ごとに止まらない
(`wrapup-chores` と同じ「一括 triage → GO 1 回 → 一括処理」の型)。

## 5. Issue 棚卸し(charters ドメインで GO が出たリポジトリのみ)

起草時に、そのリポジトリの open Issue を新しい CONTRIBUTING の Issues 節
(ADR-0017)の Rejected 例へ照らし、合致するものを表の「close 候補 Issue」欄
(charters 行にのみ追加)に挙げておく。**close するのは GO が出た後のみ**。
close するときは、合致した Rejected 例を名指しするコメントを必ず付ける
(理由なし close は禁止 — `repo-charter`
SKILL.md §9 と同じ作法)。

## 6. 一括適用(GO 分のみ、リポジトリ×ドメインごとに)

1. scratchpad へ shallow clone(`git clone --depth 1`)
2. 作業ブランチを切り、対象ドメインの修正を適用する
3. commit → push → `gh pr create`(そのリポジトリの PR 本文規約に従う。
   このリポジトリ自身が対象なら `pr-description` スキルの 5 節スケルトン)
4. **merge はしない。PR 作成までがこのスキルの完了定義**(ADR-0015 —
   人間裁定なしの merge・メタデータ反映は正本を直接書き換えることになり、
   判断ループの設計と矛盾する)。ruleset で CI 待ちになるリポジトリも、
   単に「PR 作成済み、merge 待ち」として最終報告に列挙するだけでよい。
5. charters ドメインの description/topics 反映(`gh repo edit`)、naming
   ドメインの `naming-*` topic 反映も、**該当 PR が merge された後**に
   人間が個別に行う(この段階では行わない — README が正本、メタデータは
   鏡という関係上、README merge 前に鏡だけ書き換えると矛盾した状態が
   一時的に生まれる)。ただし naming ドメインで改名を伴わない場合(盲
   再導出の一致度が高く、宣言のみで済む場合)は対応する PR 自体が存在
   しないため、GO 直後に `gh repo edit --add-topic naming-<class>` を
   直接実行してよい — 待つ理由(README merge 前の鏡の矛盾)がそもそも
   発生しない。改名を伴う場合(`naming-pj` への移行など)は改名 PR の
   merge を待つ。

## 6a. rulesets ドメインの ci-absent 適用(GO 分のみ)

- {最小 CI 播種 PR} 選択: 対象リポジトリに最小の CI ワークフローを追加する
  PR を、そのリポジトリの通常の PR フローで作成する(merge は待つ —
  §6 4 と同じ完了定義)。
- {誘導 Issue の起票} 選択: 対象リポジトリに CI 播種を促す Issue を起票する
  (attribution フッター必須)。self-verify では「Issue 起票済み・追跡中」
  として報告する(ADR-0020)。
- {exempt} 選択: §7 の overrides.tsv に追記する。

## 7. exempt 処分

一括レビューで「このリポジトリ×ドメインには基準を適用しない」と裁定された
組は、`$XDG_CONFIG_HOME/github-audit/overrides.tsv` に
`<repo>\t<domain>\texempt` を追記する(`*` ドメインで全ドメイン免除も可、
`docs/github-audit.md` 参照)。

## 8. self-verify

最後に `github-audit`(対象にしたドメインのみでよい)を再実行し、次の
いずれかで全対象が説明できることを確認して報告する:

- `ok`(今回 PR 作成済み、または既に merge 済みで再監査に反映)
- `exempt`(今回除外)
- 依然 `drifted` だが「`repo-charter`/`*-repo-governance` へ送った」
  「PR 作成済み・merge 待ち」「Issue 起票済み・追跡中」(ci-absent、
  ADR-0020)のいずれかとして最終報告に明記されている

## 9. 注意

- private/company リポジトリ名は、このリポジトリ(公開)の成果物・会話ログ
  以外の永続物に書かない(`docs/claude/public-publish-guard.md`)。一括
  レビュー表はそのセッション内限りで、docs には残さない。
- GitHub description/topics の反映は `gh repo edit` の即時反映であり、対応する
  README/CONTRIBUTING の PR が merge されるまで実行しない(§6-5)。
- `github-audit` の実行結果(ledger)には private リポジトリ名が含まれる
  ため、ローカルの `$XDG_STATE_HOME/github-audit/ledger.json` 以外の場所に
  コピーしない。
