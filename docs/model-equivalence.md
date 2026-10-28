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
- 価格を載せていないモデル(Claude Haiku 5.5、GPT-6.1 Terra)は、測定値だけを持ち、費用の比較(codex-plan)には出ない。`catalog.toml` の `[[model]]` は、価格の欄が全部そろったときだけ費用の比較に出る。
- 取り込みは `model-catalog fetch-aa`。AA の行 ID(uuid)で突き合わせる(AA 公式が、名前や slug は変わりうるので ID を主キーに
  するよう推奨している)。生成ブロックだけを書き換え、応答に無い行と、追跡中の提供元の新しい行(新モデルの候補)を報告する。
- 鍵は環境変数 `ARTIFICIAL_ANALYSIS_API_KEY`、無ければ login keyring(`secret-tool`、application=model-catalog
  credential=artificial-analysis-api-key)から読み、標準入力で `curl` に渡す(`ps` に出ない)。リポジトリには置かない。
- 利用条件: AA への出典表示が必要(ページの脚注とデータの出典欄に出す)。上限は 1 日 1000 リクエスト。
- 採点の偏り: AA-LCR と HLE は GPT 系の LLM(GPT-5.6 Luna (medium))が等価判定をしている。Claude の答えを GPT が採点する構図で、
  この 2 つでは GPT が有利に出る偏りがありうる(大きさは未確認。「未確認だった点の確認」参照)。ページの出典欄に書く。
- 純正ハーネス(Claude Code・Codex)での値は、Terminal-Bench 公式の leaderboard にある(max のみ)。AA の値との差の向きが
  モデルごとに揃わない(2026-10-27 時点で、Opus 5.5 は +5.2pt、Sonnet 5.5 は −1.8pt、GPT-6.1 Sol は +2.1pt)ので、補正に使わず、
  注記として並べる。

## 未確認だった点の確認(#1154、2026-10-28)

起票時に「未確認」としていた 6 点を、一次資料で確かめた。確かめた手段は、公式ページの取得(本文は WebFetch の要約を通るので、
引用は逐語を求めた結果)、API リファレンス、検索。引用は取得したページの原文(英語)のまま書く。

| # | 点 | 結論 | 根拠(出典・取得日 2026-10-28) |
|---|---|---|---|
| 1 | Claude の行名の `Default Fallback` | **未確認**(意味を書いた一次資料が見つからない) | AA の方法論ページ(<https://artificialanalysis.ai/methodology/intelligence-benchmarking>)に `Default Fallback` は無い(取得結果: `NOT FOUND: Default Fallback`)。API リファレンス(<https://artificialanalysis.ai/api-reference>)のモデルの項目は `id`・`name`・`slug`・`model_creator`・`evaluations`・`pricing`・速度の欄で、`reasoning_effort` や `Default Fallback` の欄は無い。モデルのページ(`/models/claude-opus-5-5-medium`)の取得結果にも説明は無かった。検索では、Claude の全行が `(Adaptive Reasoning, <段階> Effort, Default Fallback)` と付く事実だけが確認できた(例: <https://artificialanalysis.ai/models/claude-fable-5-1>)。GPT の行には付かない。 |
| 2 | `Non-reasoning` の扱い | **未確認**(`reasoning.effort` の `none` と同じかは言えない) | OpenAI の文書(<https://developers.openai.com/api/docs/guides/reasoning>)は、"Supported values are model-dependent and can include `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, and `max`." と書き、"GPT-6.1 Sol does not support `none` or `minimal` and defaults to `medium`." と書く。GPT-6.1 Terra が `none` に対応するかは、取得した範囲に記述が無い。AA の方法論ページに `Non-reasoning` という行名の説明は無いが、"Non-reasoning models: 16,384 tokens … Reasoning models: Maximum output tokens allowed" と "Temperature: 0 for non-reasoning models, 0.6 for reasoning models" のように、AA は測定条件の設定を非推論モデルと推論モデルで分けている。これは AA の分類で、OpenAI の `none` との対応を示す文ではない。**扱いは、エフォートの段階ではなく別の軸のまま**(カタログは公式の段階名を持つ方針で、`Non-reasoning` は公式の段階名として確認できていない)。 |
| 3 | 採点者の偏り | **採点者は確認できた。偏りの大きさは未確認** | AA の方法論ページ。AA-LCR: "Model responses are evaluated using GPT-5.6 Luna (medium) as an equality checker with pass@1 scoring"。HLE: "Evaluated with an equality checker LLM prompt adapted from the original HLE paper, using GPT-5.6 Luna (medium), with pass@1 scoring"。どちらも参照解答との等価判定で、主観的な採点ではない。自己選好の研究 Panickssery, Bowman, Feng, "LLM Evaluators Recognize and Favor Their Own Generations"(<https://arxiv.org/abs/2404.13076>、公開日は取得ページに記載なし)は、"an LLM evaluator scores its own outputs higher than others' while human annotators consider them of equal quality" と述べるが、それは評価者が自分の出力を採点する場面で、参照解答との等価判定でどれだけ出るかは、この文献からは言えない。採点者を替えた他の評価は見つけていない。 |
| 4 | SWE-bench 系の再開 | **まだ現行モデルの行を確認できない** | Aider の leaderboard(<https://aider.chat/docs/leaderboards/>)は "last updated November 20, 2025" で、上位は `gpt-5 (high)` など。SWE-bench の実験リポジトリ(<https://github.com/SWE-bench/experiments>)は、"submissions must include a link to an arXiv preprint or technical report" とする方針を 2025-11-18 から適用すると書くが、新しい提出の有無は取得した範囲から読めない。swebench.com の leaderboard は取得結果にテーブルが無く(動的描画)、更新日も読めなかった。**四半期ごとに確かめる**(対応表 `[category.*]` を直接測るベンチに替える条件は変えない)。 |
| 5 | 課題ごとの結果 | **公開されていない(文書に記述なし)** | API リファレンスの取得結果は、課題・問・サンプルごとの結果と標準誤差の欄を `NOT FOUND` とした。信頼区間 `ci95` を返すのは画像・動画の Elo の endpoint だけ。方法論ページにも課題ごとの結果の公開の記述は無い(`per-question` は `NOT FOUND`)。したがって、同じ課題での対応のある検定はできず、区間は今の Newcombe の方法(独立な 2 標本)のまま。 |
| 6 | Terminal-Bench の機械可読なデータ | **安定した取得方法は確認できなかった** | tbench.ai の leaderboard ページ(<https://www.tbench.ai/leaderboard/terminal-bench/4.0>)の取得結果は列の見出しだけで、行は読めなかった。リポジトリ(<https://github.com/harbor-framework/terminal-bench>)は "Dataset & leaderboard: Terminal-Bench on Harbor Hub"(<https://hub.harborframework.com/datasets/terminal-bench/terminal-bench/latest>)へ案内するが、そのページは取得時に "Loading leaderboard…" で、API・CSV・JSON の書き出しの記述は見つからなかった。Harbor の文書(<https://docs.harborframework.com/>)にも、leaderboard を機械で読む方法の記述は見つからなかった。提出の様式(`metadata.yaml` と `result.json`)は検索結果で目にしたが、一次資料では確認できていない。**今のところ、ブラウザで読んだ表の手転記を続ける**。 |

試した手段の限界: 公式ページの多くは JS で描画され、WebFetch の取得結果は本文の一部だけだった。「見つからなかった」は「資料に無い」ではない。
点 1・2・6 は、AA の担当や Harbor の保守者に問い合わせるまで未確認として扱う(問い合わせは人の作業で、ここでは行っていない)。

## ページ

`model-equivalence render --lens <lens.json> --out <page.html>` が、データとスクリプトを埋め込んだ単一の HTML を作る。ローカルで開く完全な文書が既定で、Artifact として公開するときは、公開側が文書の枠を付けるので `--fragment` で断片を出す。

- 基準の (モデル × エフォート)を選ぶと、全組合せが 4 値のバッジで並ぶ。並びは基準に対する判定で決まり(同等以上のある提供元・モデルが上)、基準を変えると変わる。「基準と同等以上だけを表示」で絞り込める。セルを押すと、ベンチごとの差の区間と許容幅の帯が出る。
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
- OpenAI, "Reasoning models" の `reasoning.effort`. <https://developers.openai.com/api/docs/guides/reasoning>(取得 2026-10-28)
- A. Panickssery, S. R. Bowman, S. Feng, "LLM Evaluators Recognize and Favor Their Own Generations". <https://arxiv.org/abs/2404.13076>(取得 2026-10-28)
- Aider, "Aider LLM Leaderboards"(last updated November 20, 2025). <https://aider.chat/docs/leaderboards/>(取得 2026-10-28)
- SWE-bench, "experiments". <https://github.com/SWE-bench/experiments>(取得 2026-10-28)
- Harbor, "Terminal-Bench on Harbor Hub". <https://hub.harborframework.com/datasets/terminal-bench/terminal-bench/latest>(取得 2026-10-28)
