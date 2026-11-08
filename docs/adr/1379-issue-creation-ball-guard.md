# ADR-1379 — `gh issue create` を、ボールのラベルと本文の先頭行を持たせないと作れなくする

- Status: Accepted
- Date: 2026-11-08
- Issue: No-Issue(本人の Issue 台帳の不変条件を、事後の検査だけでなく作成時にも守らせる設計の依頼。台帳を持つリポジトリの名前と番号は、この公開リポジトリには書かない)

## Context

本人は GitHub の Issue を台帳として使う。未完了の Issue は次の 2 つの不変条件を満たす。

1. ボール保持者を、排他ラベル `ball:me` / `ball:them` / `ball:agent` のちょうど 1 つで持つ。
2. 次に動く契機 `next: YYYY-MM-DD` を持つ。置き場は、非公開リポジトリの Issue では本文の先頭の行。公開リポジトリの Issue では、本文に日付を一切書かない
   (日付は別の非公開の台帳が持つ)。ラベル名の `ball:*` は公開側に付けてよい。

この不変条件は事後に検査するだけで、作成時には守らせていない。ボールを持たない Issue を `gh issue create` で作れてしまい、作ったあとに直す往復と、
直されないまま残る Issue が出る。エージェントが Issue を起票する経路は、ほとんどが `gh issue create` である。

## Decision

### D1. `gh issue create` を PreToolUse で検査する新しい crate `issue-create-guard` を、hook-router の規則に載せる

対象は対象リポジトリの owner が `tarotene` のときだけ(`-R/--repo`、無ければ作業ディレクトリの remote。他の org・第三者のリポジトリは対象外)。
ボールのラベルが `--label` にちょうど 1 つなければ deny し、理由文に書き直し方(`--label ball:me`)を書く。ラベルに `ledger-status`(機械が書く常設の状態 Issue)があれば免除する。

- 本命: hook-router の規則に載る新しい crate(`pr-followup-guard`・`pr-title-guard` と同じ型)。作成の呼び出しを実行前に弾くので、ボールを持たない Issue を `gh issue create` では作れない。
- 対抗馬: GitHub 側の仕組み(Issue テンプレート・Issue forms、作成後に走る Actions)。白紙から 3 軸で比べる。表現不可能性: Issue forms の `labels` は「テンプレートで作られた Issue に自動で付くラベル」で、
  取得した資料にラベルの必須化・排他選択の記述はなく、フォームは Web UI の入力に対するものとして書かれている。`gh issue create` を縛る仕組みとして書かれていない。作成後の Actions は、ボールのない Issue がいったん存在する。
  還元性: 作成前に弾く本命は、事後の検査(すでにある)が担えない「ボールを持たない Issue を作らせない」を担う。先進性: 差が無い。本命が勝つ。
- 外した候補: 事後の検査だけを続ける案。作成時に守らせる目的を満たさない(現状そのもの)。`pr-followup-guard` に足す案。判定対象が PR 本文で、Issue 作成のラベルと本文先頭行は別の契約になる。
  1 crate 1 契約の既存の構成から外れる。分析で外した。
- 既存手段: 自前 — 却下: `pr-followup-guard` の拡張(別の契約)、GitHub の Issue forms(上記)、事後検査だけの維持(目的を満たさない)。解析は `command-inspection`・`github-command`・`git-repository` を再利用する。
- 先行例: `pr-followup-guard`(PR 本文を作成時に検査する PreToolUse の hook)`docs/adr/598-pr-pending-work-to-issues.md`・`docs/agents/pr-followup-guard.md` (取得 2026-11-08)
- 先行例: GitHub Docs, "Syntax for issue forms" https://docs.github.com/en/communities/using-templates-to-encourage-useful-issues-and-pull-requests/syntax-for-issue-forms (取得 2026-11-08。公開日・著者は取得した資料に記載なし)
- 差分: 一致 — hook-router の規則の型、トークン単位の解析、fail-open の縮退は `pr-followup-guard` と同じ。異なる — 発火を owner が `tarotene` のリポジトリに限る(`pr-title-guard` の ADR-0031 D4 と同じ限定)。
- 軸: 検出のみ — `gh issue create` の経路は作成前に弾けるが、`gh api repos/…/issues -X POST`・Web UI・MCP からの作成は見ない(事後の検査が拾う)。

### D2. 本文の先頭の行の規則を、リポジトリの公開/非公開で逆にする

`--body` / `--body-file` の本文について、非公開リポジトリなら先頭の非空行が `next: YYYY-MM-DD`(暦に実在する日付)でなければ deny する。
公開リポジトリなら、`next:` `due:` `postponed:` で始まる行があれば deny する(日付を公開に載せない)。公開/非公開は `gh repo view <owner/repo> --json visibility` で引く。

- 本命: 公開/非公開を実行時に `gh` で引く。リポジトリの公開/非公開は GitHub が持つ事実で、複写しない(単一正本)。
- 対抗馬: 公開/非公開のリポジトリ名の一覧を設定に持つ案。白紙から比べる。表現不可能性: 一覧は GitHub の事実の複写で、リポジトリの公開/非公開を変えると食い違う。
  公開リポジトリの名前を公開リポジトリのファイルに持つなら問題は小さいが、非公開リポジトリの名前を載せられない(この公開リポジトリには書けない)。本命が勝つ。
- 既存手段: 採用 — `gh repo view --json visibility`(`detect-drift` の `visibility_permits_filing` が同じ出力を読む)。
- 先行例なし: 探した範囲は、この公開リポジトリの `crates/`(`rg "isPrivate|visibility"`)。本文の先頭行を公開/非公開で逆に検査する hook は見つからなかった。公開/非公開を引く先行は `detect-drift` だけで、判定の向きが違う(あちらは PRIVATE のときだけ許す)。
- 軸: 還元 — 一覧を持たず、GitHub の事実を引く。
- 縮退: 公開/非公開を引けないときは、本文の検査を飛ばしてラベルだけを検査する(fail-open)。判定できない呼び出しを止めない。本文の中身が分からないとき(シェルの展開、読めないファイル)も同じ。

## Consequences

- `tarotene` 配下のリポジトリで `gh issue create` を呼ぶと、ボールのラベルが無い呼び出しは実行前に deny される。理由文のとおり `--label ball:me` などを足して呼び直す。
- 専用の逃げ道の環境変数は設けない。免除は `ledger-status` のラベルだけで、ほかの Issue は誰が次に動くかを決めてから作る。
- `gh api` や MCP からの作成は止まらない。再発したら、その経路を規則に足す。
- 未決: 作業ログ(`worklog`)の `mail` で「自分がやる」と決める裁定は、Issue の参照なしでは受け付けない、という検査は入れていない。`mail` の行の文法(`crates/worklog/src/mail.rs`)に、
  誰がやるかの欄が無い(`decide` は「やる」と決めた事実だけを持ち、担当は行に現れない)ためで、検査を足すには文法に欄を足す必要がある。この PR では入れない。

## 執行点

- `crates/issue-create-guard/src/lib.rs`
- `crates/hook-router/src/rules.rs`
- `pipeline-map.toml`
- `config/agents/issue/vocabulary.json`
