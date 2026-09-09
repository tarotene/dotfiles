# tracking-issue スキル

`config/claude/skills/tracking-issue/SKILL.md` — 複数の子作業を束ねる親 Issue(Tracking Issue)を起票・更新するときの書式規約。地の文と GitHub sub-issues の二重管理を避け、更新すべき箇所を最小化する。

## 動機: 規約は既にあったが、発動する場所が無かった

`issue-hygiene` スキル §4 が既に「傘の本文の書き方」(子一覧は本文に書かない・書くのは依存/完了定義/スコープ外の 3 つ)を規定していた。しかし `issue-hygiene` の description は「Issue 整理・tracking を清算」で発動する事後衛生管理スキルであり、Issue を起票・更新する場面では引かれない。

2026-09-09 に `tarotene/dotfiles` を実測した結果:

- public リポジトリのチェックボックス付き Issue(#2 #3 #9 #10 #11 #20 #117)は、チェックボックス合計 30 個中チェック済みが 0 個。`#10` は 6 個・`#11` は 3 個とも未チェックのまま `completed` でクローズされていた。
- sub-issues 機能を使っているのは `#136 ⊃ {#137,#138}` の 1 クラスタのみ(2026-09-08 導入)。`#4`・`#98`・`#130` は明白な親候補だが未設定のままだった。`#98` は本文が空のまま 5 件(#99–#103)を実質束ねていた。
- 旧 private リポジトリの epic(`dotfiles-prime#207`)では、sub-issues 8 件が全 close なのに本文チェックボックスが全未チェックという食い違いを「意図的」と後付け宣言してクローズしていた。

原因は規約の不在ではなく「規約が発動する場所の不在」だったため、書く/更新する側の規約を `tracking-issue` として独立スキルに切り出し、`issue-hygiene` は事後の棚卸し・清算(発動語: 整理・清算・sub-issue 化)に専念させた。

## 発火条件を「事後昇格」にした理由

起票時に判定する方式(ADR・エピック級の構想なら最初から親として起票する)も検討したが、子が 1 件で終わる仕事のために空の傘を作るリスクがある。逆に発火条件を決めないと「作るべきときに作られない」も「作らなくてよいものが作られる」も両方起こる。`plan-scope-gate.sh` が要求インベントリの検査で子 2 件未満を SKIP する閾値(L168 `((total < 2))`)と揃え、「実作業が 2 つ以上の子に割れた時点」を昇格の唯一の条件にした。

## 本文スケルトンを「4 節 + 維持義務の一文」にした理由

外部の先行例(下記出典)は「子は sub-issues、本文に手書きしない」「議論を親に書かない」「設計文書との相互リンク必須」「Milestone/Project は親にだけ付ける」を裏付けたが、そのまま全部は採らなかった。特に rust-lang の `Implementation history` 節(関与した PR を全部列挙する)や、旧 epic で実際に機能していた `Audit trail`(日付付き追記ログ)は不採用にした — マージ済み PR は子の `Closes #N` から辿れ、親の timeline には sub-issue 追加イベントが残り、closed 子一覧は sub-issues が自動表示する。手で追記する意味が残るのは「なぜその順で進めたか・何を学んだか」だけで、それは PR 本文と ADR の仕事であり、親に持たせると手更新項目ゼロの原則を破って腐敗源を再導入することになる。

同じ理由で「可能な限りリアルタイムに更新する」という当初の要望は、更新頻度を上げる方向ではなく、更新契機を「方針・スコープ・完了定義が変わったとき」の 1 つに絞る方向で受けた。本文末尾の維持義務の一文(Kubernetes の `Please keep this description up to date` に相当)は、この限定そのものを宣言する。

## 禁止対象を「子 Issue へのチェックボックス参照」に限定した理由

実測した腐ったチェックボックス 30 個の大半は、`- [ ] #NN` のような子 Issue 参照ではなく、地の文の作業手順(例: `- [ ] personal-pop` のようなホスト別チェック)だった。チェックボックスを全面禁止すると、rust-lang の `Steps` 節や Kubernetes の段階別 PR チェックリストのような、外部の全先行例が使っている「Issue 化しない受け入れ条件」の書き方まで禁じてしまう。争点はチェックボックスの有無ではなく「手で更新しなければならない項目を親に置くか」なので、禁止は子 Issue への参照だけに絞った。

ただし副作用がある: `plan-scope-gate.sh` は sub-issues が 1 件以上ある親では本文の `- [ ]` を要求インベントリ抽出経路から一切読まない(sub-issues 優先、L124–139)。抽出させたい要件は `## 完了定義` に文章で書く必要がある。

## `tracking` ラベルを規約に含めた理由(doc の訂正を含む)

`issue-hygiene` を起こした時点の doc には「既存の `tracking` ラベルは廃止せず併用する」と書かれていたが、これは誤りだった。2026-09-09 に確認した時点で `tarotene/dotfiles` に `tracking` ラベルは存在せず(実ラベルは `bug`/`documentation`/`enhancement`/`good first issue`/`help wanted`/`invalid`/`question`/`wontfix`/`phase-5`/`phase-6a`/`phase-6b`/`blocked-by-upstream`/`deferred` のみ)、スキルを起こした別のリポジトリの記述がそのまま残っていたものと判明した。

親子構造は SessionStart の Issue 索引(`issue-index.sh`、Search API のメタデータのみを注入する軽量索引で、本文も sub-issues も読まない)には現れない。ラベルは索引にそのまま出るため、`tracking` ラベルを新設して findability を担保した(`gh label create tracking --repo tarotene/dotfiles` で作成)。ラベル体系全体の宣言的管理・台帳化は別の課題(組織全体のラベル運用衛生)であり、このスキルのスコープには含めない。

## gh CLI のフラグに更新した理由

`issue-hygiene` §2 は当初 GraphQL の `addSubIssue` mutation を直接叩く手順を指示していたが、2026-09-09 時点でローカルの `gh` 2.99.0 は `gh issue create --parent` / `gh issue edit --add-sub-issue` / `--parent` / `--remove-parent` を持つ(`cli/cli` v2.94.0 で追加)。GraphQL 直叩きは不要になったため、規約側の手順を `gh` フラグに更新した。

## 出典(取得日 2026-09-09)

- GitHub, Inc.「Adding sub-issues」GitHub Docs — https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/adding-sub-issues (親 1 件あたり子 100 件・階層 8 段までという制約)
- GitHub, Inc.「Best practices for Projects」GitHub Docs — https://docs.github.com/en/issues/planning-and-tracking-with-projects/learning-about-projects/best-practices-for-projects ("maintain a single source of truth ... instead of spread across multiple fields"、"The less you need to remember to do manually, the more likely your project will stay up to date.")
- GitHub, Inc.「REST API endpoints for sub-issues」GitHub Docs — https://docs.github.com/en/rest/issues/sub-issues
- GitHub Changelog「GitHub Issues & Projects – February 18th update」2025-02-18 — https://github.blog/changelog/2025-02-18-github-issues-projects-february-18th-update/ (旧 tasklist ブロックは 2025-04-30 に廃止、sub-issues への移行を明記)
- GitHub Staff 他「Evolving GitHub Issues and Projects (GA)」community Discussion #154148, 2025-03 — https://github.com/orgs/community/discussions/154148 (100 件上限を引き上げる予定はない、sub-issue は親の Projects/Milestone を自動継承)
- rust-lang「Tracking Issue template」`.github/ISSUE_TEMPLATE/tracking_issue.md` — https://github.com/rust-lang/rust/blob/main/.github/ISSUE_TEMPLATE/tracking_issue.md (hub として使う、議論は別 Issue に切り出す、繰り返せば親をロックする)
- Kubernetes「Enhancement tracking issue template」`.github/ISSUE_TEMPLATE/enhancement.md` — https://github.com/kubernetes/enhancements/blob/master/.github/ISSUE_TEMPLATE/enhancement.md ("Please keep this description up to date"、KEP との相互リンク必須)
- OpenTelemetry「Contributing: Issues」— https://opentelemetry.io/docs/contributing/issues/ ("Limit the scope of a given issue to a reasonable unit of work")
- GitHub CLI「Release v2.94.0」`cli/cli` — https://github.com/cli/cli/releases/tag/v2.94.0 、ローカル `gh --version`(2.99.0, 2026-09-01)の `--help` 出力で `--parent`/`--add-sub-issue`/`--remove-parent` を確認

未確認事項(2026-09-09 時点で公式記述を確認できなかった): 100 件上限に closed/archived な子が算入されるか、sub-issue progress の集計範囲が直下の子のみか全子孫を含むか、素の Markdown task list(旧 tasklist ブロックとは別)の将来的な廃止予定、tracking issue のクローズ条件についての GitHub・rust-lang・Kubernetes いずれの公式規約(→ 本スキルの §7 は自前で決めた)。
