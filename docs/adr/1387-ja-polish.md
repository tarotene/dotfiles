# ADR-1387 — 日本語の文書は、Gemini の書き直しを通した証拠を記録して出す

- Status: Accepted
- Date: 2026-11-08
- Issue: No-Issue(`/grill-me` セッション中に発生した依頼。「日本語のあらゆる文書を生成する際に Gemini CLI のフィルタを通すようにしたい」)

## Context

Claude と Codex が書く日本語には、翻訳調、硬い語、読み手に通じない内部用語が残りやすい。別系統のモデルに書き直させたい、という依頼である。

`/grill-me` のグリルで、次を本人が裁定した。

- 目的は**書き直し**(モデルの出力を最終稿にする)。指摘だけ返す案・合否を判定する案・内容を検証する案は採らなかった。
- 対象は**人が後で読む日本語の文書すべて**(文書ファイル、GitHub の本文・コメント、Gmail・Slack の下書き、Claude Docs、コミットメッセージ)。チャットの返答とコードのコメントは外す。
- 個人の側は、送った文面が学習に使われてよい。会社の側は、会社契約の Claude Code から会社契約の Gemini へつなげられる見込みだが、Gemini 側の要件(学習への利用など)はまだ確かめていない。別途調べる。今回は、送り先の扱いを確かめ済みの個人の側で有効にし、会社へは同じ形で横展開できるようにしておく。

調べて分かったこと(取得 2026-11-08):

- Gemini CLI の「Login with Google」は 2026-06-18 に廃止された(Google, Gemini Code Assist for individuals privacy notice)。残る認証は API キーと Vertex AI。
- API の無料枠は、送った内容が製品の改善に使われ、人が読みうる(Google, Gemini API Additional Terms of Service、更新 2026-03-23)。無料枠で使える最新の汎用モデルは `gemini-3.8-flash`(Google, Gemini API Pricing)。
- 固定中の nixpkgs の `llm` は 0.30、`llm-gemini` は 0.31 で、`gemini-3.8-flash` を呼べない(対応は llm-gemini 0.34 以降)。

## Decision

### D1: 呼び出しは Gemini CLI でなく `llm` + `llm-gemini` にする

「日本語を入れて日本語を出す」フィルタに、ファイルを読む・コマンドを走らせる・Web を見る道具は要らない。Gemini CLI はそれらを内蔵したエージェントで、止めるには設定で外すしかなく、文書に紛れた指示文に乗っ取られる余地も残る。`llm` は標準入力を受けて文字列を返すだけで、道具を渡さない限り、そういう動きが構造的にできない。中のモデルは同じ Gemini である。

### D2: 明示のコマンド `ja-polish` で通し、通した段落のハッシュを記録して、関所が照合する

hook が書き込み・投稿の中身を黙って差し替える案は採らない。Claude の記憶とファイルの中身が食い違い、次の Edit が「元の文が見つからない」で失敗する。Edit は断片しか渡らないので文脈も欠ける。モデルの応答は数秒から数十秒かかり、ルータ(`hook-router`、1 規則の持ち時間 10〜20 秒)にも載らない。

記録の単位は**段落**(空行で区切られた塊)。鍵は、空白をそろえた本文の SHA-256 の先頭 32 桁。関所は、投稿の本文(PreToolUse)と、文書ファイルの変更した段落(Stop)で、記録に無い日本語の段落を見つけたら止める。投稿の関所は `hook-router` の PreToolUse の規則(`ja-polish-gate`)で、文書ファイルの関所は Stop の hook(`ja-polish stop-gate`)で、git の外の文書の基点は書き込み前に残すスナップショットから取る。

### D3: 記法は伏せ字で守り、戻りを照合する

送る前に、コード・URL・参照・見出しの行・末尾の生成元の行・タグの接頭辞を伏せ字にし、戻りで同数が残っているかを確かめる。数値の集合と、行頭の記法(見出し・箇条書き・引用・表)の並びとインデント、分量(4 割〜2 倍)も照合する。外れた段落だけ 1 回再依頼し、それでも外れたら元のまま素通しにする。そのうえで Claude が差分を読み、意味がずれていたら、出力を手で直さず下書きを直して通し直す(記録と中身を一致させるため)。

### D4: 書き直しの基準は、private の文体規約をその場で読む

`writing-style-hub` が指す `docs/style/writing.md` を実行のたびに読み、指示文に載せる。写しを持たないので、規約を直せばフィルタも従う。公開のこのリポジトリには規約の中身を置かない。解決できないときは、基準なしでは送らない(素通しにする)。

### D5: 既存の文書は、追加・変更した段落だけを書き直す

基点(git なら origin の既定ブランチとの merge-base、git の外は基点なし)に同じ段落がある場合は触らず、前後の段落は文体を合わせるための参考として渡す。承認済みの文を巻き込まない。

### D6: 失敗は素通しにして、記録し、後から知らせる

無料枠には利用上限があり、通信やキーの不調も起こる。関所が拒否し続けると、PR もメールの下書きも出せなくなる。失敗した段落は元のまま `skipped` で記録し、gate イベントに `fail-open` を書く。`gate-failopen-advisory` が、続けて 3 回のとき次のセッションの冒頭で知らせる(`copilot-plan-review` と同じ扱い)。本人の文をそのまま代理投稿する(`No-Attribution:`)本文は対象外。

### D7: 有効にするかは設定 1 行で決め、今は個人の identity だけで立てる。キーは llm の置き場に置き、sandbox から隠す

`dotfiles.jaPolish.enable` を、今は個人の identity だけで立てる。送り先の扱い(学習への利用)を確かめ済みの経路が個人の無料 API キーだけだからで、クラウドの AI 道具を確かめないまま会社の機体へ配らない方針(#9)に沿う。機構は機体にも identity にも縛られず、`package`(llm と送り先のプラグイン)と `model` を持つので、会社への横展開は、要件を確かめたあとに会社の identity で enable を立てて送り先を合わせるだけで済む(`docs/agents/ja-polish.md` の「横展開」)。設定ファイルの有無が有効かどうかの唯一の印で、関所は無ければ何も検査しない。キーは `llm keys set gemini` で各機体に一度入れ、llm の保存先を `agentPermissions.secret_paths` に足して、Claude の sandbox と Read から隠す。`ja-polish` はキーと Google への通信に届く必要があるので、`gh` と同じく sandbox の外で動かす。暗号化(ADR-0022 の YubiKey 方式)は、鍵が挿さっていない間は毎回素通しになるので採らない。

### D8: llm は overlay で commit 固定する

`nix/ja-polish-llm.nix` で、`llm-gemini` 0.35、それが要る `llm` 0.32、`llm` 0.32 が要る `sqlite-utils` 4.2.1 と `condense-json` 1.1 を固定する。`llm` の最新(0.37.1)は `openai>3` を要求するが nixpkgs は 2.33.0 なので、0.32 を選んだ。nixpkgs が追いついたら外す(herdr の overlay と同じ扱い)。

## Amendment (2026-11-12 — 素通しと待ち時間を減らす: 失敗の分類・再依頼・予備のモデル・分割と並列)

Issue: No-Issue(`/grill-me` セッション中の依頼。依頼文の「ja-polish が不安定だったり遅かったりすることが多い」と「対策・代替案を調査してほしい」から)。

調べて分かったこと(vega、2026-11-12。`~/.local/state/claude/ja-polish/records.jsonl` の 2882 件):

- 書き直しの成功は 0 件で、全件が素通しだった。内訳は、API キー未設定が 2873 件(11-09〜11-12 13:52 UTC)、120 秒の時間切れが 6 件、`high demand`(混雑中)の拒否が 3 件。後ろの 2 種は、どちらも `gemini-3.8-flash` で発生した。
- 遅さの原因は 3 つ。1 回の依頼に文書の全段落を含めていること(705 回の依頼で、段落数は p50 が 3、p90 が 7、最大が 189)。思考量を指定していないこと(`gemini-3.6-flash` の既定は medium であり、minimal まで下げられる。Google, "Thinking" https://aistudio.google.com/docs/thinking 、取得 2026-11-12)。通信が失敗しても再依頼しないこと。
- 無料枠の依頼は、混雑時に優先して切り捨てられる枠で処理される、という記述が `docs/agents/ja-polish.md` にある(Google AI Studio の状態ページからの引用として、別の作業で 2026-11-12 に書かれたもの)。今回、その状態ページ(https://aistudio.google.com/status)をブラウザで開いたが、「Failed to list incidents history: authentication error」と表示されて本文が読めず、引用は**再確認できていない**(未確認)。別の一次資料として、Priority 推論の文書では優先枠を「strictly non-sheddable」(切り捨てられない)と書いている(Google, "Priority inference"、https://aistudio.google.com/docs/priority-inference 、取得 2026-11-12。調査の委譲先が逐語で引用したもの)。混雑中の拒否と時間切れは、無料枠が切り捨てられ得ることと整合する(推論。混雑の理由そのものは未確認)。

### D9: 失敗を 3 種に分け、混雑だけ間を置いて再依頼し、通らなければ予備のモデルに 1 回だけ送る

依頼の失敗を、エラーの型に応じて `Busy`(混雑・利用上限。429・503・`high demand` など)、`Timeout`(時間内に応答が返らない)、`Permanent`(キーが無い、`llm` が無い、モデル名が違う)に分ける。判定は `llm` の標準エラーの先頭 2000 字を確認する 1 か所(`classify_stderr`)に置く。

- `Busy` は、同じモデルに最大 2 回まで再依頼する。待ちは 2 秒、6 秒の順に 3 倍し、±25% のゆらぎを付ける(Google, "Troubleshooting" の、指数バックオフにジッタを足す推奨に基づく。https://aistudio.google.com/docs/troubleshooting 、取得 2026-11-12)。
- `Timeout` は、同じモデルへ再依頼しない。応答が返らないモデル(無料枠の混雑)を待ち直しても、`timeout_secs` を何度も使い切るだけだからである。すぐ予備のモデルへ切り替える。1 回の待ちは 120 秒から 45 秒に短縮した。
- 主のモデルで通らなければ、予備のモデル(既定は `gemini-flash-lite-latest`)に 1 回だけ送る。思考量の指定は予備には渡さない(予備が同じ選択肢を持つとは限らないため)。記録の `model` には、実際に応答したモデルを記録する。
- `Permanent` は再依頼も予備モデルへの切り替えも行わない。キーが無い場合、予備に送っても同じ失敗になるためである。
- 一時的な原因(`Busy`・`Timeout`)で通せなかった素通しには、記録に `"transient":true` を付ける。課金の検討を知らせる判定(段 4)が、キーの未設定や照合の外れと分けて集計するための印である。

### D10: 対象の段落を 8 段落ずつの束に分け、最大 3 本を同時に送る

1 回の依頼が大きいほど応答が遅く、時間切れにもなりやすい。束の大きさ(`chunk_paras`)と同時の数(`parallel`)は設定で変更できる。束ごとに独立して再依頼や予備モデルへの切り替えを行うため、1 つの束が失敗しても、ほかの束は書き直せる。結果は束の番号順に配置するため、完了する順番に関わらず段落の並びは変わらない。同時に送る数が増えると、無料枠の毎分の上限に当たりやすくなる(無料枠の数値は公式ページに載っておらず未確認)。上限に当たれば `Busy` として D9 が受ける。

### D11: 思考量は設定で渡し、既定は固定の見本の測定で決める

`thinking_level`(`minimal` `low` `medium` `high`)を設定可能にし、llm の `-o thinking_level` に渡す。既定値は、測定用サンプルを入れる段階(段 3)で、応答時間・照合の通過率・textlint の指摘数の増減を測定して決定するまで、指定なし(モデルの既定)とする。速さだけで選ぶと、書き直しによってかえって文が硬くなる傾向(2026-11-12 の観察では「だけ」→「のみ」、「包む」→「ラップする」)が強まっても気づけないためである。

### 執行点

- `crates/ja-polish/src/llm.rs` — 失敗の 3 種(`FailureKind`)と、標準エラーからの判定(`classify_stderr`)、`-o thinking_level` の受け渡し
- `crates/ja-polish/src/polish.rs` — 再依頼とバックオフ、予備のモデル(`call`)、束への分割と並列(`polish`)
- `crates/ja-polish/src/config.rs` — 設定の項目(`fallback_model`・`thinking_level`・`timeout_secs`・`chunk_paras`・`parallel`・`retries`・`retry_base_ms`)と既定値
- `crates/ja-polish/src/records.rs` — 一時的な原因の素通しに付ける印(`transient`)
- `home/modules/ja-polish.nix` — 設定ファイルに出す option(`fallbackModel`・`thinkingLevel`・`timeoutSecs`)

## Alternatives considered

- **Gemini CLI を使う** — 「Login with Google」が廃止済みで、API キー認証になる。内蔵の道具を設定で外す形(検出・設定頼み)になる。次点。
- **Antigravity CLI(`agy`)** — 感触で外した。非対話での挙動・データの扱いを一次資料で確かめられていない。分析ではない。
- **Python で `llm` をライブラリとして呼ぶ** — 子プロセスが要らないが、記録の型が関所(Rust の hook-router の規則)と 2 言語に分かれる。
- **hook で内容を黙って差し替える** — D2 のとおり。
- **有料の API キー・Vertex AI** — 個人の側は学習に使われてよい、という裁定で不要。会社契約の Gemini の要件を確かめたあと、会社の側の送り先として検討する(#1385)。
  2026-11-12 の再検討でも、無料のまま続ける(本人の裁定)。有料枠は、送った文面を製品の改善に使わず(Google, Gemini API Additional Terms of Service、2026-03-23 発効、https://aistudio.google.com/docs/terms 、取得 2026-11-12)、切り捨てられにくい。`gemini-3.6-flash` は 100 万トークンあたり入力 $0.75・出力 $3.75(2026 年末まで。2027-01-01 から倍。Google, Gemini API Pricing、https://aistudio.google.com/docs/pricing 、取得 2026-11-12)。月の目安は $30 前後(推定。日本語のトークン換算が粗く、倍半分の幅がある)。**再訪の条件**: 直近 7 日の、一時的な原因による素通しが段落の 5% を超えたら、セッションの冒頭で知らせる(段 4)。
- **OpenRouter の予備モデル列**(`models` 配列、失敗時に次のモデルへ。https://openrouter.ai/docs/guides/routing/model-fallbacks 、取得 2026-11-12) — 送り先が増え、無料のまま使うには `:free` のモデルだけを並べる必要がある。`llm-openrouter` から配列を渡せるかは未確認。**再訪の条件**: D9 の再依頼と予備のモデルを入れても、一時的な原因による素通しの割合が 5% を超え続けるとき。
- **ローカルのモデル**(llama.cpp の SYCL、vega の Meteor Lake の iGPU) — 送信が無いので、会社の機体(#1385)の送り先の候補になる。llama.cpp の文書(docs/backend/SYCL.md、取得 2026-11-12)は、Meteor Lake の内蔵 GPU で 7B の q4 が毎秒 13〜16 トークンと書く。9 段落(出力 1,500 トークン前後)で 1.5〜2 分の見込み(推定)で、今の Gemini より遅い。日本語の書き直しの質の一次資料は取得できず、未確認。**再訪の条件**: 会社の機体の送り先を決めるとき(#1385)。
- **ほかの提供元**(Mistral・Groq・Cerebras) — Groq の公式ページは 403 で読めず、Cerebras は試用のみで毎分 5 回(https://inference-docs.cerebras.ai/support/rate-limits 、取得 2026-11-12)。**再訪の条件**: OpenRouter と同じ。
- **Batch API・Flex 推論** — 待ちが分から 24 時間で、対話の CLI に向かない(Google, Batch API https://aistudio.google.com/docs/batch-api 、Flex https://aistudio.google.com/docs/flex-inference 、取得 2026-11-12)。

## Consequences

- 日本語の文書を書くたびに `ja-polish` を通す手順が 1 つ増える(共有の規範 `config/agents/AGENTS.md` に書いた)。差分を読む往復が増える。
- 無料枠の上限や不調で通せないことがある。止めない設計なので、素通しの文書が出ることがある。続くと次のセッションで知らせる。
- 4 つの Python パッケージを固定するので、nixpkgs の追従を見て外す作業が残る。
- 各機体で `llm keys set gemini` の手作業が一度要る。
- 会社の機体では、送り先の要件を確かめて enable を立てるまで有効にならない(#1385)。
- 混雑・時間切れでは、予備のモデルの分だけ、1 つの束の待ちが延びる。時間切れは、同じモデルで待ち直さないので、最悪が 45 秒 + 予備の 45 秒 = 90 秒で、以前の 120 秒より長くならない。混雑の拒否は、すぐ返るのが普通なので、再依頼の待ち(2 秒 + 6 秒)と予備の 45 秒までで 1 分に満たない。拒否が毎回遅く返るときは、この見積りを超える(未観測)。

## 執行点

- `crates/ja-polish/src/main.rs`
- `crates/ja-polish/src/polish.rs`
- `crates/ja-polish/src/llm.rs`
- `crates/ja-polish/src/config.rs`
- `crates/ja-polish/src/mask.rs`
- `crates/ja-polish/src/records.rs`
- `home/modules/ja-polish.nix`
- `nix/ja-polish-llm.nix`
- `home/modules/claude.nix`
- `home/modules/agent-permissions.nix`
- `home/modules/agents.nix`
- `crates/gate-failopen-advisory/src/main.rs`
- `crates/ja-polish/src/gate.rs`
- `crates/ja-polish/src/stop.rs`
- `crates/ja-polish/src/snapshot.rs`
- `crates/hook-router/src/rules.rs`

## 先行例との対比

- D1: 道具を持たない `llm` を採った。先行例: Google, Gemini Code Assist for individuals privacy notice https://developers.google.com/gemini-code-assist/resources/privacy-notice-gemini-code-assist-individuals (取得 2026-11-08)。差分: 異なる — 依頼の Gemini CLI でなく、同じモデルを道具なしで呼べる `llm`。
- D3: 伏せ字で守り戻りを照合する。先行例: DeepL, Translating XML https://developers.deepl.com/docs/xml-and-html-handling/xml (取得 2026-11-08)。差分: 異なる — API の機能でなく、送る側で伏せ字にして照合する。
- D5: 変更した段落だけを対象にする。先行例: reviewdog https://github.com/reviewdog/reviewdog (取得 2026-11-08)。差分: 異なる — 指摘の絞り込みでなく、書き直しの対象の絞り込みに使う。
- D6: 失敗は素通しにして後から知らせる。先行例: `docs/agents/copilot-plan-review.md`、`docs/agents/gate-failopen-advisory.md` (取得 2026-11-08)。差分: 一致。
- D9: 混雑は指数バックオフで再依頼し、通らなければ予備のモデルに 1 回送る。先行例: Google, Gemini API "Troubleshooting" https://aistudio.google.com/docs/troubleshooting (取得 2026-11-12)。差分: 異なる — 公式は再依頼までで、別のモデルへの切り替えは OpenRouter の model fallbacks(https://openrouter.ai/docs/guides/routing/model-fallbacks 、取得 2026-11-12)にならって足した。時間切れは同じモデルを待ち直さない点も公式と違う(待ちの上限を 1 束で 90 秒前後に抑えるため)。軸: 還元。
- D10: 段落を束に分けて並列に送る。先行例なし: Gemini API の rate-limits・troubleshooting・batch-api・caching の各ページ(取得 2026-11-12)に、対話の依頼を分割する指針は無かった。軸: 還元。
- D11: 思考量は設定で渡し、既定は測定で決める。先行例: Google, Gemini API "Thinking" https://aistudio.google.com/docs/thinking (取得 2026-11-12)。差分: 一致 — 遅延を下げるには thinking_level を下げる、という公式の指針どおり。値は測定で決める。軸: 還元。
