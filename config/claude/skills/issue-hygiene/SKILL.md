---
name: issue-hygiene
description: オープン Issue が増えて出自(同一 ADR / PR / 構想)ごとの構造が一覧から見えなくなったとき、GitHub の sub-issues 機能で親子関係を明示し、腐った tracking Issue を清算する定期衛生管理の手順。Issue 整理・tracking Issue 整理・Issue を束ねる・sub-issue 化・tracking を清算・腐った tracking Issue・昇格候補の洗い出し、organize open issues, group issues into a tracking issue, clean up stale tracking issue, sub-issues cleanup、といった文脈で使う。新しい Tracking Issue を書く/更新する側の規約は `tracking-issue` スキルを使う(こちらは事後の棚卸し・清算側)。
---

open Issue が積み上がると、同じ出自(同一の設計判断・同一の先行 PR・同一の構想)から生まれた複数の Issue が、一覧上はただの兄弟として並んでしまい、依存関係や完了条件が見えなくなる。このスキルは、その構造を GitHub の機能で表現し直し、かつ「作っただけで放置され腐る tracking Issue」を作らないための定期手順を定式化する。

## 1. クラスタ抽出

- open Issue 一覧を、ラベル・本文中の相互参照(同一 ADR / 同一先行 PR / 同一構想への言及)を根拠に出自でクラスタリングする
- 「たまたま同じ時期に起票された」だけでは束ねる根拠にならない。出自(単一の意思決定・単一の先行作業)が同一であることを本文の記述で確認する
- bot が管理する Issue(依存関係ダッシュボード等)や、判断抜きに閉じられない停滞 Issue はクラスタ候補から除外する

## 2. 実装方式: ネイティブ sub-issues を使う

- 親子関係は GitHub の sub-issues 機能で表現する。`gh issue edit <親番号> --add-sub-issue <子番号>[,<子番号>...]`(`gh` 2.94.0 以降)。GraphQL の `addSubIssue` mutation を直接叩く必要はない。**本文にチェックボックスで子一覧を書く方式は使わない** — 子がクローズされても本文のチェックが自動では更新されず、全子決着済みなのに本文だけが未更新のまま open で残る「腐った tracking Issue」を生む(この失敗は実際に一度起きている)
- ネイティブ sub-issues なら進捗(◯/N 件)が GitHub 側で自動集計されるため、本文の手動更新は不要になる
- 親には `tracking` ラベルを付ける。リポジトリに未定義なら `gh label create tracking` で先に作る

## 3. 傘の粒度

- 出自が同一なら 1 本の傘にまとめる。2 件程度の小さなクラスタごとに別々の傘を乱立させない — 傘が増えるほど「どの傘に何が入っているか」を追う調整コストが増える
- 逆に、出自がばらばらな停滞 Issue 群(定期的に見かけるが互いに無関係な chore 等)を無理に一つの傘にまとめない。束ねる意味があるのは「一緒に完了を追跡したいから」であって、「open Issue の数を減らしたいから」ではない

## 4. 傘の本文の書き方

親を立てる・書く・更新する側の規約(本文スケルトン・禁止事項・findability・クローズ条件)は `tracking-issue` スキルが正本として持つ。このスキルは事後の棚卸し・清算に専念する。清算対象の親を書き直すときも `tracking-issue` の本文スケルトンに合わせる。

## 5. 腐敗の清算

既存の tracking Issue を棚卸しし、次のいずれかに該当するものを清算する:

- **子が全て決着済み(クローズ)なのに親が open のまま**: 本文のチェックボックス等の陳腐化した記述を実態に合わせて更新し、`completed` 理由でクローズする。決定を後から追えるよう、「計画時は子だったが実装しなかった」項目には理由(not planned の経緯)を本文に一言残す
- **子の一部がまだ残っている**: 清算せず、残作業を正しく子として繋ぎ直してから open のまま維持する

## 6. ラベル補完

クラスタ化のついでに、子 Issue のラベルが欠けていれば既存のラベル体系の範囲で補う。**子 Issue に新しいラベルを作らない** — ラベル体系の拡張は今回のスコープではなく、別途判断すべき事項。親に付ける `tracking` ラベル自体は `tracking-issue` スキルの findability 規約で必須(このスキルのスコープ外の判断ではない)

## 7. 昇格候補の洗い出し

「割れたのに親に昇格していない Issue」は定期棚卸しの対象。次の 3 経路で洗い出す:

- 本文に子 Issue を指すチェック(`- [ ] #NN` 形式)を含む open Issue — `tracking-issue` 禁止事項に反したまま残っている疑いがある
- `Closes`/`Fixes`/`Resolves` で 2 本以上の PR が紐づいている open Issue(`gh pr list --repo <owner>/<repo> --state all --json number,closingIssuesReferences` で Issue 番号ごとに件数を数える)— 実質複数子に割れているのに sub-issue 化されていない
- `gh issue list --json number,subIssuesSummary,parent` で親も子も持たない Issue のうち、本文中の相互参照(同一 ADR / 同一先行 PR / 同一構想への言及)からクラスタが疑われるもの(§1 と同じ根拠)

洗い出した候補は §1 のクラスタ抽出に合流させ、`tracking-issue` の本文スケルトンで書き直す。

## 8. 触らないもの

- bot が管理する Issue(依存関係ダッシュボード等)
- クラスタに属さない停滞 Issue — ユーザーの明示判断なしに閉じたり束ねたりしない

## 9. 完成の定義

- 傘の sub-issues クエリが、想定した子を過不足なく返す
- 清算対象と判定した tracking Issue がクローズされている
- open Issue 一覧を読んだときに、どの Issue がどの構想に属するかが(傘のラベル or 親子リンクから)追える
- §7 の 3 経路で洗い出した候補が、いずれも `tracking-issue` の本文スケルトンに沿って昇格または明示的に見送られている

## サニタイズ

このスキル自体をリポジトリ間で使い回す前提のため、本文に固有のリポジトリ名・Issue 番号・URL を書き込まない。追記する事例やチェックリストの拡張時のサニタイズ規則は `skill-gardening` を参照する。
