# model-equivalence — (モデル × エフォート)の乗り換え先を、根拠付きで引く

opusplan(Plan 役 = Opus、実行役 = Sonnet)と同じ使用感を、codex-plan(Codex)で得るために、
「どの (モデル × エフォート) に乗り換えても差を感じにくいか」を、信頼できる第三者の公開ベンチだけで判定する。
判定は役(Plan 役・実行役)ごとに行い、最後に組の対応表を出す。結果は自分だけが見る単一の HTML ページになる。

- 正本のデータ: `crates/model-catalog/catalog.toml`(価格・文脈長・出典、ベンチの定義、測定値、役、履歴の種類との対応)。
- 判定と収集: `crates/model-equivalence`(`lens`・`compare`・`render`)。
- 取り込み: `crates/model-catalog`(`check`・`fetch-aa`)。

## 何を主張しないか

- **同じ課題を解き直した結果ではない。** 公開ベンチの数値から、(モデル × エフォート)の関係を崩さずに引いただけで、
  自分の仕事での使い心地を測っていない。実地の使い心地は、`codex-plan compare` の観測(`docs/codex-plan.md`)が別の材料になる。
- **隠れた変数がある。** ハーネス(Claude Code・Codex)の違い、プロンプトの癖、レート制限は、この判定に入っていない。
- **「差が無い」ことを「同等」と言わない。** 点数が同じでも、課題数が少ない公開ベンチでは区間が広く、同等と言えない。

## 判定の規則

| 観点 | 規則 | 出典 |
|---|---|---|
| 能力 | ベンチごとに、差の 90% 区間を許容幅 ±δ と比べる(TOST)。区間が ±δ の内側なら「同等」、全体が −δ より下なら「弱い分野あり」、+δ より上なら「上回る」、またぐか作れなければ「データ不足」。境界ちょうどは内側と言わない。 | Lakens 2017 |
| 区間 | 2 つのモデルを独立な 2 標本とみなし、Wilson 区間を組み合わせる Newcombe の方法で作る。同じ課題での対応を無視するので、区間は広めに出る(同等を言い過ぎない側)。 | Newcombe 1998 |
| 速さ | 出力速度と最初のトークンまでの時間を、点推定の「良さの比」で見る。1/(1+δ)〜(1+δ) の内側なら「同等」。AA は区間を公開していないので「点推定のみ」と書く。 | — |
| 役 | 総合点は作らない。判定に使うベンチすべてを見る。1 つでも「弱い分野あり」があれば「弱い分野あり」、無ければ「データ不足」が 1 つでもあれば「データ不足」、全部「上回る」なら「上回る」、それ以外は「同等」。能力のベンチが 0 件なら、速さが同等でも「データ不足」。 | — |
| 組 | Plan 役と実行役の判定を同じ規則で束ねる(両方が同等以上のときだけ「同等」)。 | — |

許容幅の既定値は、`catalog.toml` の `[equivalence]`(能力 ±5pt、速さ ±20%)。ページのスライダーで動かせる。

**分解能。** 公開ベンチは課題数が少なく、同じ点数でも区間が広がる(Terminal-Bench 4.0 は 66 課題で ±14pt ほど、AA-LCR は 100 問で
±12pt ほど、HLE は 2158 問で ±2.5pt ほど)。許容幅がこの分解能より狭いと、差が無くても「同等」と言えない。各ベンチの分解能は
判定の根拠欄に出る。既定の ±5pt では、多くの行が「データ不足」になる。これは壊れているのではなく、公開データでは言えない、という結果。

## 履歴の内訳の使い方

`model-equivalence lens` が、自分のマージ済み PR のタイトルを読み、conventional prefix(`feat`・`fix`・`refactor`・`docs`・`ci` …)で
種類に分けて、件数と期間だけを書く(タイトルもリポジトリ名も残さない)。分類は決定論的で、LLM を使わない。

- 既定の対象は自分のアカウントだけ。組織は `--owners` で明示したものだけを足す(会社の業務リポジトリを巻き込まないため)。
- 検索の上限(1 クエリ 1000 件)で切り詰められないよう、7 日ごとの範囲に区切って数える。どれかが上限に達したらエラーにする。
- **実行役** は、履歴で 10% 以上を占める種類に対応するベンチだけで判定する。**Plan 役** は種類を問わず、設計・推論・長い文脈の読解に
  効く力として同じベンチで判定する。
- 種類とベンチの対応(`[category.*]`)は、仕事そのものを測るもの(direct)と、近い力を測っているだけの代理(proxy)を分ける。
  SWE-bench 系は、現行モデルに値のある行が無い(更新停止)ので載せられず、バグ修正・機能追加は代理になる。
  対応するベンチが無い種類は、判定から黙って外さず、「測れていない割合」としてページに出す。

## データ(Artificial Analysis)

- 主データは Artificial Analysis(AA、第三者の計測機関)の API。Claude と GPT-6.1 Sol を low〜max の 5 段階ずつ、別の行で測っている。
  ハーネスは mini-swe-agent に固定(Claude Code・Codex では測っていない)。他社のモデルも同じ形で載る。
- 取り込みは `model-catalog fetch-aa`。AA の行 ID(uuid)で突き合わせる(AA 公式が、名前や slug は変わりうるので ID を主キーに
  するよう推奨している)。生成ブロックだけを書き換え、応答に無い行と、追跡中の提供元の新しい行(新モデルの候補)を報告する。
- 鍵は環境変数 `ARTIFICIAL_ANALYSIS_API_KEY`、無ければ login keyring(`secret-tool`、application=model-catalog
  credential=artificial-analysis-api-key)から読み、標準入力で `curl` に渡す(`ps` に出ない)。リポジトリには置かない。
- 利用条件: AA への出典表示が必要(ページの脚注とデータの出典欄に出す)。上限は 1 日 1000 リクエスト。
- 採点の偏り: AA-LCR と HLE は GPT 系の LLM が採点している。Claude の答えを GPT が採点する構図で、この 2 つでは GPT が有利に
  出る偏りがありうる(大きさは未確認)。ページの出典欄に書く。
- 純正ハーネス(Claude Code・Codex)での値は、Terminal-Bench 公式の leaderboard にある(max のみ)。AA の値との差の向きが
  モデルごとに揃わない(2026-10-27 時点で、Opus 5.5 は +5.2pt、Sonnet 5.5 は −1.8pt、GPT-6.1 Sol は +2.1pt)ので、補正に使わず、
  注記として並べる。

## ページ

`model-equivalence render --lens <lens.json> --out <page.html>` が、データとスクリプトを埋め込んだ単一の HTML を作る。ローカルで開く完全な文書が既定で、Artifact として公開するときは、公開側が文書の枠を付けるので `--fragment` で断片を出す。

- 基準の (モデル × エフォート)を選ぶと、全組合せが 4 値のバッジで並ぶ。セルを押すと、ベンチごとの差の区間と許容幅の帯が出る。
- 役の切り替え、許容幅のスライダー、Plan 役 + 実行役の組の対応表、履歴の内訳、データの出典と限界(90 日を超えた出典は薄く表示)。
- 判定の規則(境界の比較と役の束ね)は、スライダーを動かすために JS にも書かれている。重い計算(区間、速さの比)は Rust が
  事前に済ませて埋め込み、Rust が出した判定(2 通りの許容幅)をページが読み込み時に自分の判定と突き合わせる。ずれたら警告を出す。
  突き合わせの件数は `<html data-selfcheck="一致/総数">` にも出る。
- 生成した HTML は、自分だけが見る private なページとして公開する。公開先の URL は private な場所に置き、このリポジトリには書かない。

## 追従

- 新しいモデル・エフォートの段階: `catalog.toml` に `[[model]]`(と `aa_ids`)を足す。コードは変えない。
- 引退したモデル: `status = "retired"` にして残す(以前の組合せとの比較のため)。ページに「提供終了」と出る。
- 測定値の更新: `model-catalog fetch-aa --file crates/model-catalog/catalog.toml --write`。
- `catalog.toml` は `cleanCargoSource` の対象(`*.toml`)なので、更新は workspace の Rust ビルドのキャッシュを無効にする。

## 執行点

- `crates/model-catalog/catalog.toml`
- `crates/model-catalog/src/lib.rs`
- `crates/model-equivalence/src/judge.rs`
- `crates/model-equivalence/src/render.rs`
- `crates/model-equivalence/page.html`
- `home/modules/agents.nix`
- `pipeline-map.toml`

## 参考文献

- R. G. Newcombe, "Interval estimation for the difference between independent proportions: comparison of eleven methods",
  Statistics in Medicine 17(8):873-890, 1998. <https://pubmed.ncbi.nlm.nih.gov/9595617/>(取得 2026-10-27)
- D. Lakens, "Equivalence Tests: A Practical Primer for t Tests, Correlations, and Meta-Analyses", Social Psychological and
  Personality Science, 2017. <https://pmc.ncbi.nlm.nih.gov/articles/PMC5502906/>(取得 2026-10-27)
- Artificial Analysis, "Intelligence Benchmarking Methodology". <https://artificialanalysis.ai/methodology/intelligence-benchmarking>(取得 2026-10-27)
- Artificial Analysis, "API Reference". <https://artificialanalysis.ai/api-reference>(取得 2026-10-27)
- Terminal-Bench, "Terminal-Bench 4.0 leaderboard". <https://www.tbench.ai/leaderboard/terminal-bench/4.0>(取得 2026-10-27)
