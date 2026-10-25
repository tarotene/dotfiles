# ADR-1005 — CI の gate job を ci.yml に入れる基準を決め、検出と部品で全リポジトリに揃える

- Status: Accepted
- Date: 2026-10-25
- Issue: No-Issue(`/grill-me` セッション中の「repo-charter や github-audit は CI の具体的な構成まで
  確認しているか」という問いから始まり、全リポジトリの実測と 9 件の裁定を経て本 ADR に至った)
- Amends: ADR-591(D1 の「ブロッキング性は ci.yml の内外という配置で表す」に、どの job を内に置くかの
  基準を足す。D1〜D8 の決定は変えない)

## Context

2026-10-25 に、repo-charter と github-audit が CI の構成をどこまで確かめているかを読んだ。

確かめていたこと(ADR-591):

- required は全リポジトリ一律 `CI passed` + `PR title`。
- ci.yml の有無、集約 job `ci-passed` の有無、`ci-passed.needs` が他の全 job を束ねていること、
  `quality.json` が単一正本と一致すること(`crates/pr-contract-check/src/workflows.rs` の
  `judge_workflows`。PR ごとの `workflow-naming-check` と github-audit の `workflows` ドメインが同じ関数を使う)。
- CI が 1 つも無いリポジトリを黙認しないこと(ADR-0020 §3 の `ci-absent`)。

確かめていなかったこと:

1. **どの job を ci.yml に入れるか**の基準が、どこにも書かれていない。個別の裁定は、共有 macOS キュー待ちを
   理由に altair のビルドを外へ出した 1 件だけ(`.github/workflows/darwin-build.yml`、ADR-468)。
   このため「不要な job まで required になる」「必要な job が non-required のまま残る」「gate の設計が悪い」を、
   リポジトリごとに見つけて直している。
2. **`ci-passed` の判定の形**。`if: always()` と、落ちた job を失敗に変える step を、判定が見ていない。
   後者が消えると、gate job が落ちても `CI passed` は緑になり、required が空振りする。
3. **gate job が 0 個**(`ci-passed` だけ)の ci.yml。全ドメインが ok になる。
4. ci.yml の**外へ出した job の main での失敗**。github-audit の `annotations` ドメインは warning だけを拾い、
   failure は「赤は CI 自身が出す」として拾わない(`crates/github-audit/src/domains/annotations.rs`)。
   外へ出した job が main で落ちても、Actions の赤表示と通知メールしか気付く手段が無い。
5. 言語別テンプレ(rust / typst / astro)に、stacked PR の中間段で重い job を飛ばす仕組み(ADR-468)が無い。
6. repo-charter の自己検証(§9)は `github-audit charters naming` だけで、CI のドメインを走らせない。

全リポジトリの現状(2026-10-25、非アーカイブの自リポジトリの `ci.yml` を `gh api` で走査): gate job を持つ
リポジトリは全部 `if: always()` と失敗判定 step を持つ。gate job 0 個が 2 リポジトリ、ci.yml なしが 2 リポジトリ。

dotfiles の ci.yml の job の所要時間(PR の成功 run 直近 15 本、`gh api repos/<r>/actions/runs/<id>/jobs`、
2026-10-25): 最も重い `Rust workspace` が p75 約 5.7 分・最大約 12.7 分、`Build vega` / `Build arcturus` が
p75 約 5 分・最大約 10 分、ほかは 2 分以内。

## Decision

### D1: ci.yml(blocking)に入れる job は、2 段の基準で決める

- ① **必要条件**: 落ちたまま main に入ると、main の不変条件(ビルドが通る・テストが通る・整形済み・
  秘密が無い、など)が壊れること。
- ② **除外条件**: 次のどれかに当たる job は、①を満たしても ci.yml の外へ出し、main への push 後の検査で拾う。
  - flaky(同じ commit で結果が揺れる)
  - 外部の状態や共有キューに依存する(macOS runner、外部 API、時刻など)
  - 所要時間が上限を超える(D2)

①と「外部依存か」は job の意味の判断で、機械では決められない。作成時と triage で、この基準を読んで人が
判断する。理由のコメントを job ごとに書かせることはしない(書き忘れた job が、理由の無いまま黙って残るため)。

先行例: Kubernetes SIG Release, "Release Blocking Jobs and Criteria"
<https://github.com/kubernetes/sig-release/blob/master/release-blocking-jobs.md>(取得 2026-10-25、日付記載なし)。
blocking の条件(入れる理由の記録、自前の資源だけで動く、p75 が 120 分以内、同じ commit で 3 回連続 pass、
責任者がいる、週の pass 率 75% 以上)を閉じた一覧で持ち、満たさないものは informing に回す。
差分: 異なる — k8s はリリースを止める定期 job の条件。ここはマージ前の job に当てるので、責任者と自前資源の
条件は個人のリポジトリ群では常に満たされるため落とし、「main の不変条件を守るか」を必要条件に置く。
軸: 検出のみ — ①は意味の判断で、機械で表現不可能にも検出もできない。時間と flaky だけを D5 で検出する。

外した候補:

- job の種類の表(build / test / lint / fmt / secret scan は required、他は参考)で決める — 感触で外した。
  性質(flaky・外部依存)を見ないので、重い build や外部依存の job も入ってしまう。
- ①だけにする(flaky でも遅くても required) — 感触で外した。待ちと偶発的な赤を受け入れることになり、
  ADR-468 が計測した待ち行列の問題を再び持ち込む。
- job ごとに「守る main の不変条件」を閉語彙のコメントで必須にする — 人が書き続ける記録を増やす。
  書き忘れの壊れ方(理由の無い job が残る)が、検出を置かない場合と変わらない。

### D2: 時間上限は、その job の直近の成功 run の p75 で 10 分とする

dotfiles の実測(Context)で、最も重い required の job が p75 約 5.7 分。10 分は、今の required を
すべて収め、これ以上重い job が入ってきたときに外へ出すか分割するかを迫る値として置く。

先行例: 同上(k8s は p75 120 分)。
差分: 異なる — k8s の値は定期実行の job 向けで、マージ前の待ちには使えない。dotfiles の実測に余裕を足した値にする。
軸: 検出のみ — Actions の履歴から job ごとに測る(D5)。

### D3: ci.yml の外の検査 workflow は、既定ブランチへの push でも走らせる

PR(`opened` / `synchronize`)で走る検査 workflow(`ci.yml` と `pr-title.yml` を除く)が、既定ブランチへの
push では走らないとき、D4 の検出(main での失敗を拾う)がその workflow について何も拾えない。この組み合わせを
finding にする(`advisory-workflow-no-push:<file>`)。外にあること自体は finding にしない(D1 ②で出した
ものは正当に外にある)。PR が `closed` のときだけ走る workflow は検査ではないので対象外とする。

PR のコンテキスト(`github.event.pull_request`・`github.event.number`・`gh pr `)を使う workflow も
対象外とする。プレビュー・plan のコメント・PR 本文のリンク検査のように、PR が無い push では意味のある実行が
できないため。除かないと誤報になる: 2026-10-25 に非アーカイブの自リポジトリ 24 件を走査して、この形の
workflow が 5 件あり、除外が無いと 5 件すべてが誤報で、除外の登録(人が書き続ける記録)が要る。

先行例: 同上(k8s の informing job は定期実行で main を見る)。dotfiles の `darwin-build.yml` は push と PR の
両方で走る(取得 2026-10-25)。
差分: 異なる — k8s は定期実行、ここは push 起動で main を見る。
軸: 検出のみ — GitHub に「push でも走れ」を強制する宣言は無いので lint する。

### D4: 外へ出した job の main での失敗は、github-audit が既定の実行で拾う

既定ブランチで workflow ごとの最新の完了 run の結論が `failure` なら finding にする
(`default-branch-run-failed:<workflow>`)。`annotations` ドメインと同じ runs 一覧の取得を使い、結論だけを読む。
runs 一覧は 1 リポジトリあたり 1 回で足りるので、既定の実行に入れる。

先行例: ADR-961(annotations ドメイン)。
差分: 異なる — annotations は warning を拾い failure を拾わない(「赤は CI 自身が出す」)。ci.yml の外の
workflow は required でないので、赤が出ても PR は止まらない。ここでは CI 自身の赤では足りない。
軸: 還元 — 新しい記録を増やさず、Actions の履歴から導く。

### D5: p75 と flaky は、opt-in のドメインで検出する

- `gate-job-slow:<job>` — ci.yml の job の、直近の成功 run での所要時間の p75 が 600 秒を超える。
- `gate-job-flaky:<job>` — 同じ head SHA で、同じ job が failure と success の両方を出した(再実行で通った)。

run ごとの job 一覧を読むので高くつく(`annotations` と同じ理由、ADR-961 D2)。`Domain::OPT_IN` に置き、
名前を指定したときだけ走らせる。

先行例: 同上(k8s は「同じ commit で 3 回連続 pass できる」を事前に試す)。
差分: 異なる — 追加の実行はせず、実際の再実行の履歴から事後に見る。
軸: 検出のみ — 履歴の読み取りで判定する。

### D6: `ci-passed` の判定の形を、共有の判定関数で検査する

`needs` が空でない(gate job が 1 つ以上ある)`ci-passed` に、次の 2 つが無ければ finding にする。

- `ci-passed-not-always` — job 直下の `if:` に `always()` が無い。gate job が落ちると `ci-passed` 自体が
  skipped になる。
- `ci-passed-failure-step-missing` — `needs.*.result` と `failure` を含む失敗判定の式(コメント行を除く)が無い。
  gate job が落ちても緑になる。

`judge_workflows` に入れるので、PR ごとの `workflow-naming-check` と github-audit が同じ判定になる。
今の全リポジトリはどちらも満たしており(Context)、この検査で止まる PR は無い。

先行例: ADR-591 D1(判定の形の定義)。
差分: 一致 — 定めた形を、`needs` の網羅と同じ判定関数で検査する。
軸: 検出のみ — GitHub に集約 job の形を宣言する手段は無い。

### D7: gate job 0 個は、監査だけの finding にする

`ci.yml` に `ci-passed` 以外の job が無いとき `ci-gate-empty` を出す。PR ごとの検査には入れない: 該当の
リポジトリ(core 型の最小形を播いたもの)の PR が全部止まるため。CI が無いことを黙認しない `ci-absent` と同じく、
triage で {gate job を足す(D1 ①を読んで決める) / 誘導 Issue / exempt} を選ばせる。

先行例: ADR-0020 §3(`ci-absent`)。
差分: 一致 — 黙認せず、人間裁定に上げ続ける。
軸: 検出のみ — 中身の要否は人が決める。

### D8: 言語別テンプレの重い job に、stacked PR の中間段 skip を入れる

stack の軸(その PR が stack の中間段か)だけを汎用の composite action に切り出し、rust / typst / astro の
ci.yml テンプレの、重い job(目安: p75 2 分超)の `if:` で使う。path の軸は、テンプレが既に持つ変更検出の
step に任せる。dotfiles の `stack-position`(build_host / rust の出力を持ち、dotfiles のパス一覧を埋め込む)は
その action を呼ぶ形にし、挙動は変えない。

先行例: ADR-468(stacked PR の中間段では重い CI ジョブを skip する)。
差分: 異なる — ADR-468 は dotfiles だけで、部品も dotfiles のパスを埋め込んでいる。stack の軸だけを
切り出し、PR title の composite action(ADR-591 D3)と同じ形で他リポジトリへ配る。
軸: 還元 — stack の判定を 1 つの composite action に置き、各リポジトリは参照するだけにする。

### D9: repo-charter は、自己検証に workflows と rulesets を足す

`github-audit charters naming` を `charters naming workflows rulesets` に広げる。インタビューは増やさない。
core 型(テンプレ無し)では `ci-gate-empty` が出るので、D1 ①を読んで job を足すか exempt にする。

先行例: config/claude/skills/repo-charter/SKILL.md(今は charters と naming だけ)。
差分: 異なる — CI のドメインも同じ自己検証で見る。
軸: 還元 — 既存の監査コマンドに引数を足すだけ。

## Consequences

- 「どの job を ci.yml に入れるか」を、リポジトリごとの判断でなく、この ADR の基準で決められる。
  triage の finding も、基準の語(①・②・D2 の 10 分)で処分を書ける。
- ①(main の不変条件)と外部依存の判定は、機械化しない。基準を読む人の判断が残る。
- 検出は手動の `github-audit` のまま(定期実行は無い)。同じ見落としが繰り返されたら、ADR-543 の昇格として
  定期実行を検討する。
- 実装は 4 段の stacked PR に分ける: 段 1(D1〜D2 の基準、D6、D7、D9)、段 2(D3、D4)、段 3(D5)、
  段 4(D8)。各段は、その段で実体が追加・変更された `## 執行点` を追記する。

## 執行点

- crates/pr-contract-check/src/workflows.rs
- crates/github-audit/src/domains/workflows.rs
- config/claude/skills/repo-charter/SKILL.md
