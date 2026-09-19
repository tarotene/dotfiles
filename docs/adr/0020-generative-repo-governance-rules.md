# ADR-0020 — リポジトリ統制を生成側規則(閉じた文法+語彙)に反転する

- Status: Accepted
- Date: 2026-09-19
- Issue: No-Issue(grill-me セッション中に発見・裁定)

## Context

ADR-0015 の運用(`github-audit` → `github-audit-triage`)を実地に回した結果、
5 ドメインとも構造は同じだった: リポジトリは自由に生成され、監査は事後に
それを検査するだけ。この構造には根本的な限界がある — 検査項目を増やすほど
運用負荷が増える一方、生成側は相変わらず自由なので、次の新規リポジトリでも
同じ drift が再発する。実例(具体的にどのリポジトリかは
`docs/claude/public-publish-guard.md` の方針により本 ADR には書かない):
`naming-descriptive` 候補として一見自然に見えた名前が、実際には「片付ける」
という完了しうる行為を表す語であり、`naming-pj`(期限付きプロジェクト)で
あるべきだった。これは検査ルールの穴ではなく、生成側(命名時の判断)に
「行為名詞を器の名前として使わない」という制約が最初から無かったことが
原因であり、インシデントのたびに検査項目を1つ追加するやり方では収束しない。

同様の限界は rulesets ドメインにもある: `required_status_checks` を CI の
有無から導出する設計(本 ADR §2)は正しい縮退だが、CI が存在しないリポジトリを
単に「対象外」で黙認すると、CI を生やす動機が構造的に消え、永久に
`not-applicable` のまま固定化する。

## Decision

ADR-0014(命名クラス 4 種の語彙)を維持しつつ、次の生成側規則を追加する。

1. **命名: クラス別に閉じた文法+語彙。** キーワード(種別語・prefix・
   ドメイン)は閉集合、識別子スロットは字句規則(`[a-z0-9-]` kebab の
   英語)のみで拘束する。
   - `naming-descriptive` = `<対象>-<種別語>`。種別語は**恒久的な器の
     名詞のみ**の閉集合とし、完了しうる行為名詞(cleanup, migration,
     consolidation 等)は採録しない — 行為は `naming-pj` へ強制する。
     初期集合は `config/github-audit/descriptive-species.tsv` に置く。
     追加は本ファイルの改訂 PR 経由。
   - `naming-codename` = **default-deny + 鋳造レジストリ**。テーマ語彙は
     定めない。新規 codename は `config/github-audit/codename-registry.tsv`
     への追記 PR が先行必要(`^[a-z]{3,12}$`・一語・既存リポ名との前方
     一致禁止)。監査は「`naming-codename` 宣言リポ ⊆ レジストリ」を
     決定的に検査する(cutoff の前後を問わず、全 codename 宣言リポに
     適用する — 過去に宣言されたものも含めて閉集合の外に置かない)。
   - `naming-pj` = `pj-<対象>`(対象スロットは字句規則のみ、閉集合なし)。
   - `naming-site` = 保有ドメインの閉集合
     (`config/github-audit/site-domains.tsv`)。
2. **grandfather は `createdAt` で決定論化する。** リポジトリの
   `createdAt` が本 ADR の Date 以前 → 現行 ADR-0014 の緩いパターン
   (字句規則のみ、種別語・ドメインの閉集合チェックなし)で判定する
   (遡及リネームなし、ADR-0007 と同型)。以後に作成される新規リポジトリ
   のみ、上記の閉じた文法をそのまま機械判定する。coder-registry の
   包含検査(1 で述べた)だけは、grandfather の有無に関わらず全 codename
   宣言リポに適用する — 過去のクラス選択そのものではなく、クラスの
   閉じた運用(登録簿の存在)を今から始めることが目的だから。
3. **rulesets: baseline を CI 有無から導出する。ただし CI 不在を
   サイレントに容認しない。** `required_status_checks` は
   `.github/workflows` が非空のリポジトリにのみ要求する。CI がない
   リポジトリは `not-applicable` で黙認せず、`ci-absent` として drift
   報告を続ける(renovate ドメインの `not-applicable` 導出と同じ「適用
   可能性を導出する」発想だが、そこで止めず「なぜ CI がまだ無いか」を
   人間裁定に上げ続ける点が異なる — CI が生えない限り要求が発火しない
   導出は、放置すると永久に発火しない)。`github-audit-triage` は
   `ci-absent` の組ごとに {最小 CI 播種 PR / 播種誘導 Issue の起票 /
   exempt} の三択を一括レビュー表で提案し、人間が GO で裁定する。
4. **検証原理は盲再導出(blind re-derivation)。対象は名前と purpose 文
   (= description の正本)の両方。** 既存の名前・purpose 文の妥当性は、
   検査ルールを増やして判定するのではなく、「実物を知らない体でコード
   ベース(責務)だけを見せられ、本 ADR のルールのみで命名 / purpose 文を
   書くなら何と書くか」を再導出し、実物との一致度で判定する。高一致は
   例外的に許容し、不一致は改名・purpose 文改稿の提案として人間裁定に
   載せる。ルールは少なく保ち、インシデントごとに検査項目を増やす運用は
   しない — 再導出という検証原理そのものが、個別の検査ルールでは拾え
   ない「命名時の判断の質」を継続的に見る手段になる。purpose 文への
   適用は charters ドメインの棚卸し(別セッション)で行う。
5. **topics: 統制名空間のみ閉じる。** ガバナンス意味を持つ topic は
   接頭辞付き名空間(現状 `naming-*`)に限定し、名空間内は閉集合とする。
   名空間外の自由 topic(技術領域タグ等)は許容し、監査対象外のままとする。
6. **description は現行の逐語ミラー規則(ADR-0013)を維持する**
   (既に生成規則が閉じている)。

## Alternatives considered

- **インシデントのたびに検査項目を追加する** — Context の実例のような
  不一致が見つかるたびに新しい検査(例: 「-cleanup で終わる名前を
  禁止」)を書き足すやり方。個別の穴は塞げるが、ルール数が際限なく増え、
  次の未知の不一致には無力(そもそも「行為名詞」という一般化に到達しない)。
  棄却 — 本 ADR は逆に、閉じた文法+盲再導出という 2 つの一般原理に
  還元し、ルール数を増やさない方針を採る。
- **CI 不在リポジトリを `not-applicable` として renovate ドメインと
  同様に扱う** — 一貫性はあるが、CI が生える動機を監査側が構造的に
  消してしまう。棄却 — `ci-absent` として drift 報告を続け、triage の
  三択に載せる。
- **命名クラスの再導出を機械化する(LLM 呼び出しを監査に組み込む)** —
  ADR-0015 の「監査は LLM を呼ばない」原則(判定は `github-audit`、LLM は
  `github-audit-triage` のみ)と矛盾する。棄却 — 盲再導出は
  `github-audit-triage` 側の一手順として置く。

## Consequences

- `scripts/github-audit` の naming/rulesets ドメイン判定ロジックが本 ADR の
  規則を参照する(`docs/github-audit.md` に判定規則を反映)。
- `config/github-audit/{codename-registry,descriptive-species,
  site-domains}.tsv` が閉語彙の正本になる。private リポジトリの codename・
  site ドメインは、dotfiles が PUBLIC である制約上この repo 管理ファイルに
  書けないため、`~/.config/github-audit/*.local.tsv` のローカル overlay に
  置く(`docs/claude/public-publish-guard.md` の非公開方針を継続)。
  species トークン(cleanup 除外の理由になった一般英単語)はリポジトリ名
  そのものではないため repo 管理ファイルに置ける。
- `repo-charter` スキルの新規作成インタビューが、本 ADR の文法を作成時点
  (`gh repo create` 前)で強制するよう更新される — 「自由に生成して事後
  validation」の入口をここで塞ぐ。
- `github-audit-triage` スキルの naming 提案手順が、盲再導出+レジストリ
  seed 提案に置き換わる。
- 本 ADR の時点で存在する全リポジトリは `createdAt` が本 ADR の Date 以前
  のため grandfather 対象であり、ADR-0014 と同じ「想定内の初期状態」になる
  (codename レジストリ包含検査のみ、grandfather の有無を問わず適用)。

## Verification

- `github-audit naming rulesets --selftest` — cutoff 前後 × 各クラスの
  文法判定、codename レジストリ包含検査、CI 有無 × `ci-absent` の分岐を
  fixture で確認。
- `github-audit naming rulesets --json` を実アカウントに対して実行し、
  既存リポジトリが grandfather 規則どおりに判定されることを確認
  (2026-09-19 時点)。

## Amendment (2026-09-19 — 種別語閉集合の初期セットが実使用を過小調査していた)

Decision 1 の種別語閉集合(`descriptive-species.tsv`)の初版は、思いつきの
汎用「器」語(inventory, toolbox, config 等)から作り、`naming-descriptive`
を既に宣言している全リポジトリの末尾トークンを実際に調査していなかった。
本 ADR 自身が Decision 4 で掲げた「発明する前に先行例を確認する」原則を、
その ADR の実装物である閉集合そのものには適用し損ねていた。

末尾トークンの調査自体は行ったが、最初の一巡はトークンの表面的な語感
(「研究テーマっぽい語だから恒久的な主題だろう」)だけで採否を決め、
対応する README を実際に読まなかった。結果、複数リポジトリで使われている
ある種の末尾トークン(具体的にどのリポジトリ・どの語かは
`docs/claude/public-publish-guard.md` の方針により本 ADR には書かない)
を「研究分野という恒久的な主題」を指す種別語として採録しようとしたが、
これは誤りだった。README を読むと、該当リポジトリはいずれも
**完了・凍結した研究の archive**(Scope の Out に明示的に「新規研究の
拒否」「content-frozen」「this research is complete」「archive of a
closed research period, not an active research program」とある)であり、
その研究テーマを恒久的に研究し続けているわけではなかった。

研究テーマそのものを表す語(現象名・分野名)を種別語として個別に採録
するのは、テーマの数だけ語彙が際限なく増える設計であり、「閉じた語彙」
という本 ADR の目的そのものと矛盾する(cleanup を締め出すために閉じた
語彙を作ったのに、今度は主題語を1つずつ追加する形で同じ穴が別の形で
開く)。これらのリポジトリの実体を正しく表す器語は、主題そのものでは
なく「凍結された研究記録の入れ物」を指す `archive` である — 対象がどの
研究テーマであっても語彙を増やさずに表現できる。

`primer`(入門文書)・`coursework`(講義教材一式)は主題語ではなく
文書・教材の「型」を指す器語であり(`guidebook`/`template` と同種)、
この問題を抱えないため採録を維持する。

**追加決定**: `descriptive-species.tsv` に `archive` を追加し、研究
テーマそのものを表す語は追加しない(採録基準の文言はそのまま「恒久的な
器の名詞のみ」を維持 — 広げる必要はなかった)。該当リポジトリは
grandfather 対象であり今回のリポジトリ横断調査で新たに drift するもの
ではないため改名は不要だが、将来これと同じ「完了した研究の archive」を
新規作成する場合は `<対象>-archive` の形を使う。

もう一件、末尾トークンが序数(語ではない)になっている宣言済みリポジトリ
が1件見つかった。これは種別語では拾えない「番号付き連番」命名の可能性が
あり、`naming-pj` か番号スロットの追加検討が要るが、grandfather 対象
であり今回新たに drift するものではないため、本 Amendment の対象外と
し、将来の `repo-charter` 個別インタビュー送りの候補として記録するに
留める。
