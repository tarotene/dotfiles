# ADR-0000 — 日本語の文書は、Gemini の書き直しを通した証拠を記録して出す

- Status: Accepted
- Date: 2026-11-08
- Issue: No-Issue(`/grill-me` セッション中に発生した依頼。「日本語のあらゆる文書を生成する際に Gemini CLI のフィルタを通すようにしたい」)

## Context

Claude と Codex が書く日本語には、翻訳調、硬い語、読み手に通じない内部用語が残りやすい。別系統のモデルに書き直させたい、という依頼である。

`/grill-me` のグリルで、次を本人が裁定した。

- 目的は**書き直し**(モデルの出力を最終稿にする)。指摘だけ返す案・合否を判定する案・内容を検証する案は採らなかった。
- 対象は**人が後で読む日本語の文書すべて**(文書ファイル、GitHub の本文・コメント、Gmail・Slack の下書き、Claude Docs、コミットメッセージ)。チャットの返答とコードのコメントは外す。
- 個人の側は、送った文面が学習に使われてよい。会社の側は、学習に使われない経路を使いたい(会社では別に Gemini を契約している)。今回は個人の側だけを設計する。

調べて分かったこと(取得 2026-11-08):

- Gemini CLI の「Login with Google」は 2026-06-18 に廃止された(Google, Gemini Code Assist for individuals privacy notice)。残る認証は API キーと Vertex AI。
- API の無料枠は、送った内容が製品の改善に使われ、人が読みうる(Google, Gemini API Additional Terms of Service、更新 2026-03-23)。無料枠で使える最新の汎用モデルは `gemini-3.8-flash`(Google, Gemini API Pricing)。
- 固定中の nixpkgs の `llm` は 0.30、`llm-gemini` は 0.31 で、`gemini-3.8-flash` を呼べない(対応は llm-gemini 0.34 以降)。

## Decision

### D1: 呼び出しは Gemini CLI でなく `llm` + `llm-gemini` にする

「日本語を入れて日本語を出す」フィルタに、ファイルを読む・コマンドを走らせる・Web を見る道具は要らない。Gemini CLI はそれらを内蔵したエージェントで、止めるには設定で外すしかなく、文書に紛れた指示文に乗っ取られる余地も残る。`llm` は標準入力を受けて文字列を返すだけで、道具を渡さない限り、そういう動きが構造的にできない。中のモデルは同じ Gemini である。

### D2: 明示のコマンド `ja-polish` で通し、通した段落のハッシュを記録して、関所が照合する

hook が書き込み・投稿の中身を黙って差し替える案は採らない。Claude の記憶とファイルの中身が食い違い、次の Edit が「元の文が見つからない」で失敗する。Edit は断片しか渡らないので文脈も欠ける。モデルの応答は数秒から数十秒かかり、ルータ(`hook-router`、1 規則の持ち時間 10〜20 秒)にも載らない。

記録の単位は**段落**(空行で区切られた塊)。鍵は、空白をそろえた本文の SHA-256 の先頭 32 桁。関所は、投稿の本文(PreToolUse)と、文書ファイルの変更した段落(Stop)で、記録に無い日本語の段落を見つけたら止める。関所の実装は後続の段で入る。

### D3: 記法は伏せ字で守り、戻りを照合する

送る前に、コード・URL・参照・見出しの行・末尾の生成元の行・タグの接頭辞を伏せ字にし、戻りで同数が残っているかを確かめる。数値の集合と、行頭の記法(見出し・箇条書き・引用・表)の並びとインデント、分量(4 割〜2 倍)も照合する。外れた段落だけ 1 回再依頼し、それでも外れたら元のまま素通しにする。そのうえで Claude が差分を読み、意味がずれていたら、出力を手で直さず下書きを直して通し直す(記録と中身を一致させるため)。

### D4: 書き直しの基準は、private の文体規約をその場で読む

`writing-style-hub` が指す `docs/style/writing.md` を実行のたびに読み、指示文に載せる。写しを持たないので、規約を直せばフィルタも従う。公開のこのリポジトリには規約の中身を置かない。解決できないときは、基準なしでは送らない(素通しにする)。

### D5: 既存の文書は、追加・変更した段落だけを書き直す

基点(git なら origin の既定ブランチとの merge-base、git の外は基点なし)に同じ段落がある場合は触らず、前後の段落は文体を合わせるための参考として渡す。承認済みの文を巻き込まない。

### D6: 失敗は素通しにして、記録し、後から知らせる

無料枠には利用上限があり、通信やキーの不調も起こる。関所が拒否し続けると、PR もメールの下書きも出せなくなる。失敗した段落は元のまま `skipped` で記録し、gate イベントに `fail-open` を書く。`gate-failopen-advisory` が、続けて 3 回のとき次のセッションの冒頭で知らせる(`copilot-plan-review` と同じ扱い)。本人の文をそのまま代理投稿する(`No-Attribution:`)本文は対象外。

### D7: 有効にするのは個人の identity だけ。キーは llm の置き場に置き、sandbox から隠す

`dotfiles.jaPolish.enable` を個人の identity で立てる。会社の機体には道具も設定も配らない(クラウドの AI 道具は personal の identity にだけ置く方針、#9 と同じ)。設定ファイルの有無が有効かどうかの唯一の印で、関所は無ければ何も検査しない。キーは `llm keys set gemini` で各機体に一度入れ、llm の保存先を `agentPermissions.secret_paths` に足して、Claude の sandbox と Read から隠す。`ja-polish` はキーと Google への通信に届く必要があるので、`gh` と同じく sandbox の外で動かす。暗号化(ADR-0022 の YubiKey 方式)は、鍵が挿さっていない間は毎回素通しになるので採らない。

### D8: llm は overlay で commit 固定する

`nix/ja-polish-llm.nix` で、`llm-gemini` 0.35、それが要る `llm` 0.32、`llm` 0.32 が要る `sqlite-utils` 4.2.1 と `condense-json` 1.1 を固定する。`llm` の最新(0.37.1)は `openai>3` を要求するが nixpkgs は 2.33.0 なので、0.32 を選んだ。nixpkgs が追いついたら外す(herdr の overlay と同じ扱い)。

## Alternatives considered

- **Gemini CLI を使う** — 「Login with Google」が廃止済みで、API キー認証になる。内蔵の道具を設定で外す形(検出・設定頼み)になる。次点。
- **Antigravity CLI(`agy`)** — 感触で外した。非対話での挙動・データの扱いを一次資料で確かめられていない。分析ではない。
- **Python で `llm` をライブラリとして呼ぶ** — 子プロセスが要らないが、記録の型が関所(Rust の hook-router の規則)と 2 言語に分かれる。
- **hook で内容を黙って差し替える** — D2 のとおり。
- **有料の API キー・Vertex AI** — 個人の側は学習に使われてよい、という裁定で不要。会社の側の送り先として後続で検討する。

## Consequences

- 日本語の文書を書くたびに `ja-polish` を通す手順が 1 つ増える(共有の規範 `config/agents/AGENTS.md` に書いた)。差分を読む往復が増える。
- 無料枠の上限や不調で通せないことがある。止めない設計なので、素通しの文書が出ることがある。続くと次のセッションで知らせる。
- 4 つの Python パッケージを固定するので、nixpkgs の追従を見て外す作業が残る。
- 各機体で `llm keys set gemini` の手作業が一度要る。
- 会社の機体はこの機構の外にある。

## 執行点

- `crates/ja-polish/src/main.rs`
- `crates/ja-polish/src/polish.rs`
- `crates/ja-polish/src/mask.rs`
- `crates/ja-polish/src/records.rs`
- `home/modules/ja-polish.nix`
- `nix/ja-polish-llm.nix`
- `home/modules/claude.nix`
- `home/modules/agent-permissions.nix`
- `crates/gate-failopen-advisory/src/main.rs`

## 先行例との対比

- D1: 道具を持たない `llm` を採った。先行例: Google, Gemini Code Assist for individuals privacy notice https://developers.google.com/gemini-code-assist/resources/privacy-notice-gemini-code-assist-individuals (取得 2026-11-08)。差分: 異なる — 依頼の Gemini CLI でなく、同じモデルを道具なしで呼べる `llm`。
- D3: 伏せ字で守り戻りを照合する。先行例: DeepL, Translating XML https://developers.deepl.com/docs/xml-and-html-handling/xml (取得 2026-11-08)。差分: 異なる — API の機能でなく、送る側で伏せ字にして照合する。
- D5: 変更した段落だけを対象にする。先行例: reviewdog https://github.com/reviewdog/reviewdog (取得 2026-11-08)。差分: 異なる — 指摘の絞り込みでなく、書き直しの対象の絞り込みに使う。
- D6: 失敗は素通しにして後から知らせる。先行例: `docs/agents/copilot-plan-review.md`、`docs/agents/gate-failopen-advisory.md` (取得 2026-11-08)。差分: 一致。
