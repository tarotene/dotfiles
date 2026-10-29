# model-equivalence — (モデル × エフォート)の乗り換え先を、根拠付きで引く

opusplan(Plan 役 = Opus、実行役 = Sonnet)と同じ使用感を、codex-plan(Codex)で得るために、
「どの (モデル × エフォート) に乗り換えても差を感じにくいか」を、信頼できる第三者の公開ベンチだけで判定する。
判定は役(Plan 役・実行役)ごとに行い、最後に組の対応表を出す。結果は自分だけが見る単一の HTML ページになる。

- 正本のデータ: `crates/model-catalog/catalog.toml`(価格・文脈長・出典、ベンチの定義、測定値、役、履歴の種類との対応)。
- 判定と収集: `crates/model-equivalence`(`lens`・`compare`・`observations`・`render`)。
- 取り込み: `crates/model-catalog`(`check`・`fetch-aa`)。AA が測っている提供元・モデルを、コードを変えずに自動で登録する(「自動登録」の節)。

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

許容幅の既定値は、速さが `catalog.toml` の `[equivalence]` の値(±20%)。能力は**役ごと**に決める(下の「分解能」)。ページのスライダーで動かせ、
「既定に戻す」は表示中の役の既定に戻す。

**分解能。** 公開ベンチは課題数が少なく、同じ点数でも区間が広がる(Terminal-Bench 4.0 は 66 課題で ±14pt ほど、AA-LCR は 100 問で
±12pt ほど、HLE は 2158 問で ±2.5pt ほど)。許容幅がこの分解能より狭いと、差が無くても「同等」と言えない。各ベンチの分解能は
判定の根拠欄に出る。能力の許容幅を ±5pt に固定すると、多くの行が「データ不足」になり、Plan 役では「同等」が 1 つも出なかった(#1150)。

**役ごとの既定(#1150)。** 能力の既定の許容幅は、役の判定に使うベンチの分解能の最大値(点数 0.5 のとき。`judge::role_resolution`)を
スライダーの刻み(0.5pt)に切り上げた値で、`[equivalence]` の `capability_margin_points`(±5pt)が下限。
履歴の内訳で判定に使うベンチが絞られる役は、絞ったあとのベンチで決める。いまのカタログでは、Plan 役が約 ±12pt、実行役が約 ±14.5pt。
「この許容幅より狭くは言えない」ことは、ページの許容幅のスライダーの下に常に文で出し(許容幅が分解能より狭いと、その旨も足す)、
CLI の `compare` も役の見出しに許容幅と分解能の最大値を出す。`--margin-points` を渡すと、全部の役がその値になる。
同等を言い過ぎないために、区間の計算は従来どおり独立 2 標本(同じ課題での対応を無視)。対応のある検定にして区間を狭める案(Issue の D)は、
課題ごとの結果が公開されているか未確認のため、この変更では扱わない。

## 履歴の内訳の使い方

`model-equivalence lens` が、自分のマージ済み PR を種類に分けて、種類ごとの件数と期間だけを書く(タイトル・リポジトリ名・
ファイル名は残さない)。分類は決定論的で、LLM を使わない。規則は `catalog.toml` の `[category.*]` の宣言が持つ(コードに
prefix の表は無い)。

**種類を決める順序**

1. **PR タイトルの conventional prefix**(`prefixes`)。型の一覧は Conventional Commits 1.0.0(
   <https://www.conventionalcommits.org/en/v1.0.0/>、取得 2026-10-28)が「`fix:` と `feat:` 以外の型も使ってよい。例として
   `build:`・`chore:`・`ci:`・`docs:`・`style:`・`refactor:`・`perf:`・`test:` を勧める」と書くものに合わせた。`revert`(取り消し)と
   慣用の別名(`feature`・`bugfix`・`hotfix`・`doc`・`tests`)を足している。`style` は構造・書式を直す仕事としてリファクタ、
   `chore` は依存・雑務として設定・インフラ、`revert` は壊れた変更を戻す修正としてバグ修正に入れる。`feat(x):`・`feat!:`・
   大文字小文字の違いは同じ型として読む。同じ prefix を 2 つの種類に宣言すると、読み込みで拒否する。
2. **変更ファイルの名前**(`files`)。prefix で決まらなかった PR(prefix が無い、または宣言に無い型)にだけ、PR の変更ファイルを
   REST(`repos/OWNER/REPO/pulls/N/files`、1 回 100 件)で取る。prefix で決まった PR には取りに行かない(呼び出しは prefix で決まらない PR の数だけ。
   従来の表で「その他」が 15.5% だった実機の履歴〔2026-10-27、1915 件、#1151〕でも、取る PR はその一部になる)。**全ファイルが同じ 1 つの種類に当たるときだけ**その種類にする。
   `dirs`(`docs/`・`.github/`・`tests/` など。パスの先頭か、任意の `/` の直後から始まる一致。最も長い一致が勝つ)を先に見て、
   無ければ拡張子(`md`・`nix`・`yml` など)で決める。混在、どれにも当たらないファイルがある、ファイルが無いときは `other` のまま。
3. どちらでも決まらなければ `other`。

ファイルの取得に失敗した(レート制限、権限、削除されたリポジトリ)PR、または 100 件に届いて全体を見られない PR は、その PR を
`other` にして続ける(全体は失敗させない)。`lens.json` の `by_files` が「ファイルで決まった件数」、`file_failures` が「取れずに
`other` にした件数」で、多いときは `other` の中身が分類の限界ではなく取得の失敗によることが分かる。

**運用**

- 既定の対象は自分のアカウントだけ。組織は `--owners` で明示したものだけを足す(会社の業務リポジトリを巻き込まないため)。
  新しい組織は `model-equivalence lens --list-orgs` で名前を確かめ、`--owners 自分のアカウント,組織` に足して再実行する。
  再実行は、履歴の使い方が変わったとき(担当や仕事の種類が変わった)でよく、定期実行はしない。
- **会社の業務リポジトリ(#1151 の扱い)。** 初手では数えない(既定のまま)。数えるなら会社の機体の上で、件数だけを分類する。
  `lens` はタイトルを読んでメモリ上で分類するだけで、書き出すのは件数と期間(`lens.json`)だけ。タイトル・リポジトリ名・ファイル名は
  どこにも残さない(テストで縛っている)。手順は、会社の機体で `model-equivalence lens --list-orgs` から対象の組織名を選び、
  `model-equivalence lens --owners 自分のアカウント,組織名 --out 機体内の保存先/lens.json` を実行し、`counts` の数値だけを持ち出す。
  ただし、業務リポジトリのタイトルをローカルの集計ツールが読むことが社内規程で許されるかは、本人にしか決められない。裁定は #1192
  (「会社の機体で、履歴の内訳に業務リポジトリを数えてよいかを決める」)で取り、結果をここに 1 行で記録する。
  **裁定(2026-10-29、#1192): 本人が規程を読み、業務リポジトリのタイトルをローカルの集計ツールで読み、件数だけを残すことは規程で許可されると判断した。**
  実際に数えるかどうかは別で、数えるときは、上の手順を会社の機体で 1 手順ずつ行う(`lens.json` を持ち出す前に `counts` の数値だけを目で確かめる)。まだ数えていない。
- 検索の上限(1 クエリ 1000 件)で切り詰められないよう、7 日ごとの範囲に区切って数える。どれかが上限に達したらエラーにする。
- **実行役** は、履歴で 10% 以上を占める種類に対応するベンチだけで判定する。**Plan 役** は種類を問わず、設計・推論・長い文脈の読解に
  効く力として同じベンチで判定する。
- 種類とベンチの対応(`[category.*]`)は、仕事そのものを測るもの(direct)と、近い力を測っているだけの代理(proxy)を分ける。
  SWE-bench 系は、現行モデルに値のある行が無い(更新停止)ので載せられず、バグ修正・機能追加・テストは代理になる。
  対応するベンチが無い種類は、判定から黙って外さず、「測れていない割合」としてページに出す。
- 窓の長さ(既定 365 日)と、最近の仕事を重く見るかは、この段では変えていない(`--days` で変えられる)。実機の内訳が
  窓によってどう動くかを見てから決める。

## データ(Artificial Analysis)

- 主データは Artificial Analysis(AA、第三者の計測機関)の API。Claude と GPT-6.1 Sol を low〜max の 5 段階ずつ、別の行で測っている。
  ハーネスは mini-swe-agent に固定(Claude Code・Codex では測っていない)。他社のモデルも同じ形で載る。
- 価格を載せていないモデル(Claude Haiku 5.5、GPT-6.1 Terra)は、測定値だけを持ち、費用の比較(codex-plan)には出ない。`catalog.toml` の `[[model]]` は、価格の欄が全部そろったときだけ費用の比較に出る。
- 取り込みは `model-catalog fetch-aa`。AA の行 ID(uuid)で突き合わせる(AA 公式が、名前や slug は変わりうるので ID を主キーに
  するよう推奨している)。生成ブロックだけを書き換え、手登録の部分には触れない。規則は次の「自動登録」の節。
- 鍵は環境変数 `ARTIFICIAL_ANALYSIS_API_KEY`、無ければ login keyring(`secret-tool`、application=model-catalog
  credential=artificial-analysis-api-key)から読み、標準入力で `curl` に渡す(`ps` に出ない)。リポジトリには置かない。
- 利用条件: AA への出典表示が必要(ページの脚注とデータの出典欄に出す)。上限は 1 日 1000 リクエスト。
- 採点の偏り: AA-LCR と HLE は GPT 系の LLM(GPT-5.6 Luna (medium))が等価判定をしている。Claude の答えを GPT が採点する構図で、
  この 2 つでは GPT が有利に出る偏りがありうる(大きさは未確認。「未確認だった点の確認」参照)。ページの出典欄に書く。
- 純正ハーネス(Claude Code・Codex)での値は、Terminal-Bench 公式の leaderboard にある(max のみ)。AA の値との差の向きが
  モデルごとに揃わない(2026-10-27 時点で、Opus 5.5 は +5.2pt、Sonnet 5.5 は −1.8pt、GPT-6.1 Sol は +2.1pt)ので、補正に使わず、
  注記として並べる。

## 自動登録(#1148)

`catalog.toml` の `[discovery] aa_release_since`(公開日の下限)以降の、AA の全提供元・全モデルを、`fetch-aa --write` が
生成ブロックの中へ `[[provider]]`・`[[model]]`・`[[measurement]]` として書く。新しい提供元・モデル・エフォートが出ても、
コードは変えない。生成ブロックは毎回、前回のブロックと応答から作り直す(同じ入力なら同じ文字列。2 回目の実行で差分は出ない)。

| 規則 | 内容 |
|---|---|
| 対象 | 公開日が `aa_release_since` 以降で、役の判定に使うベンチ(`[role.*].benchmarks`)のどれかに値がある行。既に自動登録した行は、日付の外に出ても、値が null になっても残る。`[discovery]` が無ければ、新しい登録はしない。 |
| 手登録の保護 | 生成ブロックの外の `[[provider]]`・`[[model]]` は読むだけ。手登録の `aa_ids` の行は、測定値だけを更新する。手登録と同名(大文字小文字・空白を無視した基本名が一致し、提供元の `aa_creator` も合う)の別の行は、取り込まずに報告する。手登録の部分へ、自動登録済みと同じ ID のモデルを足したら、手登録が優先され、自動登録側が外れる。 |
| エフォート | 名前の末尾の括弧の、カンマの前の先頭語が、閉じた語彙(low・medium・high・xhigh・max・minimal・reasoning・non-reasoning)に当たるときだけエフォートとし、括弧を外した名前を基本名にする。それ以外(`(Dec '24)`・`(Preview)`・`(Adaptive Reasoning, …)` など)は名前の一部のままで、エフォートは `default` の 1 行。公式の段階名かの確認は別(「未確認だった点の確認」の 2)。 |
| まとめ方 | (提供元, 基本名)で 1 モデルにまとめる。同じ (基本名, エフォート) の行が複数あれば、公開が新しい方を取り(同日なら ID の大きい方)、取らなかった行を報告する。 |
| 提供元 | 手登録の提供元の `aa_creator` に合えばそこへ(Anthropic・OpenAI の新モデルは既存の提供元に入る)。無ければ、提供元名のスラッグで新しい `[[provider]]` を作る。 |
| ID の安定 | 前回の生成ブロックの AA の行 ID を逆引きし、同じモデル ID を再利用する(名前が変わっても別モデルにならない。表示名は追従する)。新しいモデルは基本名のスラッグ。手登録・既存と衝突したら提供元の ID を付け、それでも衝突したら番号を付ける。 |
| 引退 | 応答に 1 行も無くなった自動登録のモデルは `status = "retired"` で残り、応答に戻れば active に戻る。応答に無い行の最後の値は、行ごとの取得日(`retrieved`)つきで持ち越す。ページは、取得から 90 日を超えた値のセルを薄くし、根拠の欄に日数を出す。手登録のモデルの `status` は手登録の部分にあるので書き換えず、応答に 1 行も無いときは報告だけをする。 |
| 価格 | 自動登録のモデルは価格を持たない(費用の比較には出ず、ページには「価格は未登録」と出る)。 |

実 API を呼ばずに確かめるため、テストは `crates/model-catalog/tests/fixtures/aa-small.json` と、テスト内で作った 330 行の応答を使う。

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

## 近さの地図(#1147)

判定は「基準 1 点に対する閾値判定」なので、基準を選ばないと全体の配置が見えない。(モデル × エフォート)同士の近さを距離で定義し、
2 次元の地図(SVG の散布図)にして、まとまり・同じモデルのエフォートを上げたときの動きを見えるようにする。総合点は作らない
(ベンチごとの尺度も誤差も違い、どのベンチで負けているかが消える)ので、座標も 1 つの点数に潰さない。実装は `crates/model-equivalence/src/map.rs`。

**距離の定義(L∞)。** 成分は、役の判定に使うベンチ(履歴で絞った後のもの)と、出力速度・最初のトークンまでの時間。2 つの行の距離は、
成分ごとの「差 ÷ 尺度」の最大値。

| 成分 | 差 | 尺度 |
|---|---|---|
| 能力のベンチ(割合、課題数 n あり) | 点数の差(pt) | max(許容幅 δ, 分解能)。分解能は、2 点の中点の点数 p で、n 課題のときの差の 90% 区間の半幅(判定の根拠欄の分解能と同じ式 `judge::resolution_points`) |
| 能力のベンチ(課題数なし、または割合でない指数) | 点の差 | 許容幅 δ |
| 速さ | `ln(候補 / 基準)` の絶対値(出力速度も待ちも、向きに依らず同じ値) | `ln(1 + 許容比)` |

- 距離 1 は、その成分の差が「許容幅、または分解能のうち広い方」ちょうど、を表す。**距離が 1 以下でも「同等」ではない**(同等は、差の 90% 区間が ±δ の内側に収まるかで決める。距離は点推定の差の比較)。
- 値の欠けた成分は距離から外す。**両方に値のある成分が k 本未満の組は距離を出さない**(k = `MIN_COMMON` = 3。判定の根拠が薄すぎるため。
  この値は本リポジトリの推奨値で、文献から採ったものではない)。k は、能力のベンチと速さを合わせた成分の本数で数える。距離を出せない組が出たら、
  `render` が標準エラーに警告を出し、ページにも「何行を載せず、何組を補ったか」を書く。
- 分解能を基準側でなく中点で測るのは、距離を 2 点に対して対称にするため(判定の分解能は基準の点数で測る)。判定の式を変えない。
- 成分の部分集合が組ごとに違うので、厳密な距離の公理(三角不等式)は保証しない。対称と、自己距離 0 は保証する(テストで縛る)。
- L∞ にする理由: 役の判定が「1 つでも弱ければ弱い」(最悪の成分で決まる)と束ねているのに合わせた。平均や二乗和にすると、1 つのベンチで大きく負けている点が
  近く見えてしまう。この定義は本リポジトリ独自で、先行例は無い(下の先行例の節)。

**2 次元への射影(古典的 MDS)。** 距離行列を二重中心化し、上位 2 つの固有値・固有ベクトルを取って座標にする。固有ベクトルは、直交化つきの累乗法で
自前実装した(外部の線形代数 crate は足さない)。同じ入力は同じ座標になるよう、初期ベクトルを決定的な擬似乱数にし、固有ベクトルの符号を
「絶対値が最大の成分を正にする」に固定する。軸の向きに意味は無い。距離を直接出せない組があるときは、距離を出せた組だけを辺とみなした最短経路で補い
(補った組の数はページに出す)、辺でつながらない行は地図に載せない。

**stress の読み方。** 地図の歪みは Kruskal の stress-1(`sqrt(Σ(d − d̂)² / Σ d²)`、d = 元の距離、d̂ = 地図の上の距離)で測る。直接の距離がある組だけで計算する。
0 に近いほど、地図が距離を保つ。**近さの地図は 2 次元への圧縮で歪む。stress が大きいときは、地図の近さを信じず、距離の表(ページの「基準に距離の近い順」)を信じる。**
ページは stress が 0.2 を超えたら警告する。この 0.2 は本リポジトリが置いた目安で、Kruskal の解釈の表から採ったものではない(原典は未取得)。

**ページ。** 距離の尺度は、役の既定の許容幅と、能力だけを 2 倍にした「広め」(`render::wide_margins`)の 2 通りに固定し、Rust が座標と stress を事前計算して埋め込む(スライダーで MDS を作り直さない)。
許容幅のスライダーが変えるのは、点の形と色(基準に対する判定)。基準は黒い二重丸で、基準を変えると判定の色が変わり、点を押すと根拠(下)と基準との距離が出る。
「この点を基準にする」で、選んだ点を基準に切り替えられる。地図は役ごとに別(Plan 役と実行役は使うベンチが違う)。点の形は判定ごとに違う(色だけに頼らない)。
同じモデルのエフォートは線で結ぶ。座標は行数 × 2 の数値だけなので、サイズは行数に比例する(30 行で 52KB → 69KB、360 行で約 160KB → 約 200KB。上限 500KB をテストで固定)。
距離の式は、選んだ基準との距離を出すために JS にも置き、役ごとの既定の基準 1 点 × 全候補の距離を `golden.dist` として埋め込んで、読み込み時に突き合わせる(判定・区間の golden と同じ方式)。

固定の fixture での stress(2026-10-28 の手元の実行): 実カタログ(30 行)で Plan 役 0.042・実行役 0.041(既定の尺度)。330 行の疑似データ(`render.rs` のテストで作る一様乱数に近い値)では
0.18〜0.27 で、成分が互いに独立に散らばるデータは 2 次元に収まりにくい。実データで stress が小さいのは、能力と速さが少数の方向に載っていることを示すが、行が増えたときに保たれるかは未確認。

**先行例。**

- 「ベンチの点数が低次元の能力空間に載る」ことの根拠: S. Y. Ruan, C. J. Maddison, T. Hashimoto, "Observational Scaling Laws and the Predictability of Language Model Performance",
  arXiv:2405.10938。要旨に "language model performance is a function of a low-dimensional capability space" とある(<https://arxiv.org/abs/2405.10938>、取得 2026-10-28。公開日は取得結果に記載なし)。
  これは、少数の次元でモデルの配置を描いてよい、という動機の裏付けで、**MDS や L∞ 距離の根拠ではない**。
- MDS の原典(W. S. Torgerson, 1952。J. B. Kruskal, 1964、stress の定義と解釈)は **未取得**。古典的 MDS(二重中心化 + 固有値分解)と stress-1 の式は、一般に知られた形で実装し、
  式の細部を原典と照合していない。上のテストは、正三角形・直線上の点・正方形で stress が 0 に近いこと(既知の小例)で縛っている。
- 距離の定義(判定の区間・分解能の考え方を流用し、役の束ねの「1 つでも外れたら弱い」に合わせた L∞)は、本リポジトリ独自。同じ目的の先行する距離は探していない
  (探した範囲: 上の論文の要旨だけ。距離の定義に関する文献の検索は未実施)。

## ページ

実地観測を含める場合は、各機体の native transcript から集計してからページを生成する。

```sh
model-equivalence observations --days 14 --out /tmp/observations.json
model-equivalence render --observations /tmp/observations.json --out /tmp/model-equivalence.html
```

`--days` の既定は 14 日。Codex と Claude Code の記録について、成功した PR 作成の
呼び出しと結果を照合し、到達数・壁時計・本人の往復を集計する。Plan と実行のモデルを
組で示し、実行中のモデル・推論量の変更は比較対象から除く。5 件未満の PR 到達は
件数不足として示す。割当てありと観察のみは別にする。

取得できない時間は `null`、ページでは `—` と欠測件数で示す。Claude の PR 到達までの
稼働時間と TTFT は未取得であり、0 とみなさない。入力の本文は本人の発言と PR 作成の
成否を識別するために一時的に読むが、出力型には本文・リポジトリ名・URL・スレッド ID を
持たせない。この観測は同じ仕事を無作為に再実行した比較ではなく、公開ベンチの判定を
補正しない。収集を指定しない場合もページの観測欄を残し、未収集と表示する。

`model-equivalence render --lens <lens.json> --out <page.html>` が、データとスクリプトを埋め込んだ単一の HTML を作る。ローカルで開く完全な文書が既定で、Artifact として公開するときは、公開側が文書の枠を付けるので `--fragment` で断片を出す。

- 基準の (モデル × エフォート)を選ぶと、全組合せが 4 値のバッジで並ぶ。並びは基準に対する判定で決まり(同等以上のある提供元・モデルが上)、基準を変えると変わる。「基準と同等以上だけを表示」で絞り込める。セルを押すと、ベンチごとの差の区間と許容幅の帯が出る。
- 近さの地図(上の「近さの地図」の節)。基準に距離の近い順と、点の少なさ・欠損・圧縮の歪みへの注記も出る。
- 役の切り替え、許容幅のスライダー、Plan 役 + 実行役の組の対応表、履歴の内訳、データの出典と限界(90 日を超えた出典は薄く表示)。
- 判定の規則と区間の計算(Wilson・Newcombe の式、境界の比較、速さの比、役の束ね)は、スライダーを動かすために JS にも書かれている。
  全組合せの区間を Rust が事前計算して埋め込む方式は、組合せ数(行数の 2 乗)に比例して増えて破綻した(30 行で 560KB、360 行なら数 MB)ので、
  ページが、選んだ基準 1 点 × 全候補だけを都度計算する(30 行で 52KB、360 行で約 160KB)。
  Rust との一致は、役ごとの既定の基準 1 点 × 全候補について、2 通りの許容幅での判定と、判定に使うベンチごとの差の区間(小数 4 桁に丸めた値)を
  `golden` として埋め込み、ページが読み込み時に自分の計算と突き合わせて縛る。ずれたら警告を出す。埋め込みは行数に比例する。
  突き合わせの件数は `<html data-selfcheck="一致/総数">` にも出る。
- JS の核(`CORE BEGIN`〜`CORE END`)は DOM に触れず、`cargo test`(`render.rs`)が node で実行して golden と突き合わせる(360 行、12 通りの基準、
  埋め込みを書き換えたときに検知することまで)。node が無い環境ではこの突き合わせだけを省く(ブラウザの読み込み時の突き合わせは残る)。
- 列(エフォート)の並びは、語彙の段階順 → `default` → その他。モデル名・提供元で絞り込める。先頭列は固定で、表は横に動かせる。
- 生成した HTML は、自分だけが見る private なページとして公開する。公開先の URL は private な場所に置き、このリポジトリには書かない。

## 追従

- 新しい提供元・モデル・エフォートの段階: `[discovery]` の範囲なら、`fetch-aa --write` が自動で登録する。コードは変えない。価格や、
  公式の段階名・出典を持たせたいモデルだけ、`catalog.toml` の生成ブロックの外に `[[model]]`(と `aa_ids`)を手で足す(手登録が優先される)。
- 引退したモデル: 自動登録のモデルは `fetch-aa` が `status = "retired"` にして残す(以前の組合せとの比較のため)。手登録のモデルは人が
  `status = "retired"` にする。ページに「提供終了」と出る。
- 測定値の更新と自動登録: `model-catalog fetch-aa --file crates/model-catalog/catalog.toml --write`。標準出力に、新しく登録したモデル・
  retired にしたモデル・応答に無い行・同じ名前とエフォートの衝突・手登録と同名で取り込まなかった行が出る。
- `catalog.toml` は `cleanCargoSource` の対象(`*.toml`)なので、更新は workspace の Rust ビルドのキャッシュを無効にする。

## 執行点

- `crates/model-catalog/catalog.toml`
- `crates/model-catalog/src/lib.rs`
- `crates/model-catalog/src/aa.rs`
- `crates/model-catalog/tests/fetch_aa.rs`
- `crates/model-equivalence/src/judge.rs`
- `crates/model-equivalence/src/map.rs`
- `crates/model-equivalence/src/render.rs`
- `crates/model-equivalence/page.html`
- `home/modules/agents.nix`
- `pipeline-map.toml`

## 参考文献

- R. G. Newcombe, "Interval estimation for the difference between independent proportions: comparison of eleven methods",
  Statistics in Medicine 17(8):873-890, 1998. <https://pubmed.ncbi.nlm.nih.gov/9595617/>(取得 2026-10-27)
- D. Lakens, "Equivalence Tests: A Practical Primer for t Tests, Correlations, and Meta-Analyses", Social Psychological and
  Personality Science, 2017. <https://pmc.ncbi.nlm.nih.gov/articles/PMC5502906/>(取得 2026-10-27)
- S. Y. Ruan, C. J. Maddison, T. Hashimoto, "Observational Scaling Laws and the Predictability of Language Model Performance". <https://arxiv.org/abs/2405.10938>(取得 2026-10-28)
- Artificial Analysis, "Intelligence Benchmarking Methodology". <https://artificialanalysis.ai/methodology/intelligence-benchmarking>(取得 2026-10-27)
- Artificial Analysis, "API Reference". <https://artificialanalysis.ai/api-reference>(取得 2026-10-27)
- Terminal-Bench, "Terminal-Bench 4.0 leaderboard". <https://www.tbench.ai/leaderboard/terminal-bench/4.0>(取得 2026-10-27)
- OpenAI, "Reasoning models" の `reasoning.effort`. <https://developers.openai.com/api/docs/guides/reasoning>(取得 2026-10-28)
- A. Panickssery, S. R. Bowman, S. Feng, "LLM Evaluators Recognize and Favor Their Own Generations". <https://arxiv.org/abs/2404.13076>(取得 2026-10-28)
- Aider, "Aider LLM Leaderboards"(last updated November 20, 2025). <https://aider.chat/docs/leaderboards/>(取得 2026-10-28)
- SWE-bench, "experiments". <https://github.com/SWE-bench/experiments>(取得 2026-10-28)
- Harbor, "Terminal-Bench on Harbor Hub". <https://hub.harborframework.com/datasets/terminal-bench/terminal-bench/latest>(取得 2026-10-28)
