# ja-polish — 日本語の文書を Gemini に書き直させ、通した証拠を残す

設計の判断: `docs/adr/1387-ja-polish.md`。実装: `crates/ja-polish`。有効化: `home/modules/ja-polish.nix`
(`dotfiles.jaPolish.enable`。今は個人の identity だけで有効にしている。機体にも identity にも縛られない機構で、
会社への横展開の手順は「横展開」)。

## 何のためか

Claude や Codex が書く日本語には、翻訳調、硬い語、読み手に通じない内部用語が残りやすい。別系統のモデル
(Gemini)に、意味を変えない範囲で書き直させ、その結果を最終稿にする。

対象は**人が後で読む日本語の文書**: 文書ファイル(docs・ADR・skill・vault のノート)、GitHub の
本文・コメント、Gmail・Slack の下書き、Claude Docs、コミットメッセージ。**チャットの返答とコードの
コメントは対象外**(hook で書き換える手段が無く、出た後に差し戻すと 1 往復増えるため)。

## 使い方

```
ja-polish <ファイル>…          ファイルをその場で書き直す
ja-polish --stdin              標準入力の本文を書き直して標準出力へ(報告は標準エラー)
ja-polish --dry-run …          書き換えも記録もせず、差分だけ見る
ja-polish --check <ファイル>…  未通過の日本語の段落を一覧する(あれば終了コード 1)
ja-polish --no-baseline …      基点を使わず、文書全体を対象にする
ja-polish --message …          メール・メッセージの下書き用。文体規約に例文集を足した基準で書き直す
```

投稿の本文・下書き・コミットメッセージは、本文を scratchpad のファイルに書き、`ja-polish <そのファイル>`
で書き直し、直したファイルの内容を `--body-file` や `-F` に渡す(または下書きのツールに貼る)。
scratchpad は git の外なので基点が無く、本文の全体が対象になる。`ja-polish` は**単体で呼ぶ**: `<`・`>`
のリダイレクトや `$(…)` を足すと、呼び出し全体が sandbox の中に残り、キーに届かず失敗する
(`sandbox-compound-guard`、#1164)。`--stdin` は人が端末で使う形。**出力の差分は必ず読む**。意味・数値・主張の強さがずれて
いたら、出力を手で直さず、下書きを直して通し直す(記録は段落の中身で照合するため、出力を手で
直すと「通していない段落」に戻る)。

`ja-polish` は `llm` を呼び、キーと Google への通信に届く必要があるので、Claude の sandbox の外で
動かす(`home/modules/claude.nix` の `excludedCommands` に `ja-polish *`)。**Codex からは動かない**。
Codex の sandbox では通信できず、キーの置き場も読めないので、段落は「素通し」になる(下の
「Codex では動かない」)。

### Codex では動かない

Codex のセッションで `ja-polish` を実行すると、許可の確認は出ないが、段落は素通しになる(2026-11-15、vega)。

- 失敗の連鎖(Codex、`ja-polish --dry-run <ファイル>` を実行): まず `sqlite3.OperationalError: unable to open
  database file`(llm の履歴 DB。#1605 で `-d :memory:` を足して避けた)、次に
  `PermissionError: [Errno 13] Permission denied: '~/.config/io.datasette.llm/aliases.json'`(出力はホームの絶対パス)。
  どちらも、`llm` のユーザーディレクトリ(API キーの置き場)が、Codex から実行したプロセスに対して拒否されている。
- 原因: Codex の設定(`home/modules/codex.nix` の `secretPaths`)が、このディレクトリを読めないように宣言している。
  Codex 0.165.0 のソースに、コマンドの `allow` の規則は、ファイルシステムの制限を外す許可にならない、とある:
  「Only actual approval of an explicit escalation may widen the filesystem; a command allow rule does not
  authorize removing filesystem restrictions.」(`codex-rs/core/src/tools/sandboxing.rs:255-256`、OpenAI Codex
  rust-v0.165.0、取得 2026-11-15)。#1517 で入れた `allow` の規則(sandbox の外で動かす)は、拒否の宣言がある
  限り効かないので、外した。
- 決定(本人、2026-11-15): Codex では `ja-polish` を使わない。キーの拒否は、既存の方針どおり維持する。
  キーの置き場を Codex の拒否の外に作ると、無料枠の個人キーを Codex のエージェントが読めるようになるので、採らない。
- 影響: Codex で書いた文書は、書き直されない(素通し)。文書は壊れない。Claude 側で通したいときは、Claude の
  セッションで `ja-polish <ファイル>` を実行する。

## メール・メッセージの基準(`--message`)

Gmail・Slack の下書きや LinkedIn などの送信文は、`ja-polish --message <ファイル>` で通す。文書の基準との違いは 3 つ。

- 指示文に、ハブの例文集(`docs/style/messages.toml`)と、例文どおりの言い回しを保つ指示が足される(上の「何が起きるか」の 4)。
- 通した段落の記録に、`"message":true` の印が付く(`records.rs` の `Entry::message`)。印の無い古い記録は、文書の基準で通した段落として読む。
- Gmail・Slack の下書きの関所(`ja-polish-gate`)は、この印のある段落(素通しも含む)だけを通す。文書の基準で通した段落は、例文どおりの
  言い回しを保つ指示を受けていないので、下書きとしては通さない。拒否の理由は、`ja-polish --message <そのファイル>` を案内する。
  GitHub の本文・コミットメッセージ・Claude Docs は、文書の基準で通した段落でよい。
- 書き直しは、文書の基準で通した段落も、メッセージの基準でもう一度対象にする(`records::load_message_polished`)。
  `--check --message` は、メッセージの基準で通した段落だけを通過とみなす。

## 何が起きるか

1. 文書を段落(空行で区切られた塊)に分ける。フェンス(```)と front matter は 1 つの段落として、
   書き直しの対象から外す。
2. 対象の段落は、次をすべて満たすもの: 伏せ字を除いて日本語の字(かな・漢字)が 4 字以上あり、空白でない字の 2 割以上を
   占める(英語の段落に「先行例」「差分」のラベルの語が数文字入っただけのものを、対象にしない。ADR-1387 の D16)、
   記録に無い、基点に同じ段落が無い(追加・変更した段落である)。
   - 基点: git の作業ツリーの中なら、origin の既定ブランチとの merge-base にある同じパスの内容。
     `git mv` で改名してきたファイルは、元のパスの内容(改名の検出は git の `-M`)。中身を変えずに移しただけの
     ファイルは、全段落が「追加した段落」と数えられない(Stop の関所が、移しただけの文書を全段落未通過と差し戻した)。
     git の外(vault)・新規のファイル・origin が無いときは基点なしで、文書全体が対象。
     一度通した段落は記録で分かるので、基点が無くても通し直しで書き換えは起きない。
3. 記法を**伏せ字**(`⟦P0⟧`)に置き換える: 見出しの行、`>` の引用行、`Closes #N` などの参照の行、末尾の
   `🤖 Generated with …` の行、`No-Issue:` などのタグの接頭辞、`「…」`・`『…』` の引用、インラインコード、HTML タグ、
   リンクの宛先、URL、Issue/PR の参照。見出しを丸ごと伏せるのは、PR 本文の `## 課題` や
   `## 要確認` のように、別の検査が見出しの語で節を探すものがあるため。**引用を伏せるのは、逐語であるべき本人の言葉・
   原文の引用が、書き直しや textlint の前段で言い換えられる事故を、構造で消すため**(引用の中の言い回しが短く直され、
   保存した原文との照合が落ちた。F8・F18)。引用の中は、長さを問わず書き直しの対象にならない(閾値を決める根拠が無い。
   600 字までの範囲に限り、対にならない括弧が遠くの括弧まで飲まないようにする。入れ子は内側の対だけ)。引用は、
   コード片・URL より先に伏せる(先にコード片を伏せると、引用の伏せ字の元の文字列に伏せ字が入り、戻らない)。
4. `writing-style-hub` が指す `docs/style/writing.md`(private の文体規約)をその場で読み、指示文に
   載せる。写しは持たない。ハブが解決できないときは、基準なしでは送らず、段落を素通しにする。
   `--message` のときは、同じハブの `docs/style/messages.toml`(外に送る文章の例文集)も読んで指示文に足し、
   「例文どおりの言い回しは、意味が通るかぎり変えない。補助動詞は仮名。半角と全角の間に空白を入れない」を指示する。
   例文集が読めないときも、基準なしと同じく送らず、素通しにする(文書の基準だけで書き直すと、例文どおりの言い回しを書き換えるため)。
5. 対象の段落を `chunk_paras`(既定 8)個ずつの束に分け、最大 `parallel`(既定 3)本を同時に送る。
   各束は、その段落と前後の段落(参考。書き換えない)を、`llm prompt -m <model> -s <指示文>
   [-o thinking_level <値>] --no-stream -n -d :memory:` に標準入力で渡す。`-n` で llm の履歴 DB に残さない。
   `-d :memory:` は、履歴 DB をファイルでなくメモリ上に置く。`llm prompt` は `-n` でも、判定より前に履歴 DB
   (既定は llm のユーザーディレクトリの `logs.db`)を必ず開くので(llm 0.32、`llm/cli.py:769-772`、取得
   2026-11-15)、そのディレクトリに書き込めない環境では `sqlite3.OperationalError: unable to open
   database file` で落ちる。2026-11-15 に、Codex から `ja-polish` を実行した結果がこれで、書き込めない
   ディレクトリを指す `LLM_USER_PATH` で、同じ失敗を手元で再現した(`-d :memory:` を付けると、この段を通る)。
   混雑・時間切れのときの再依頼と予備のモデルは「失敗したとき」(ADR-1387 の D9〜D11)。
6. 戻ってきた段落を照合する。外れたら、その段落だけ 1 回再依頼する:
   - 伏せ字がすべて同数で残っている
   - 数値の集合(全角は半角にそろえる)が同じ
   - 行頭の記法(見出し・箇条書き・番号付き・引用・表)の並びとインデント幅が同じ
   - 分量が元の 4 割以上 2 倍以下
   - 依頼の書式(`<para>`・`<text>`・`</text>`、文脈の欄の `<before>`・`<after>`)が混ざっていない(段落の本文が、`<before>` などを元から持つときは除く)。囲みの外れた `</text>` が単独で残る応答もここで外れる(F22)
   - 行頭にあった伏せ字(`No-Visual:` などのタグの接頭辞)が、出力でも行頭にある(個数の照合は位置を見ないので、別に見る。F36)
   - 括弧の幅(`(` `)` と `（` `）` の個数)が変わっていない。リポジトリの文書の括弧の流儀を、書き直しで変えない(F7)
   - 入力に無い簡体字(日本語に出ない頻出の字の短い一覧 `SIMPLIFIED_ONLY`。F40)と、記号の実体参照(`&gt;` `&lt;` `&amp;` `&quot;`)の増加(F68)がない。字種を網羅する検査ではなく(符号化の表を持つ crate は依存に無い)、活用の崩れ(「頼まず」→「頼せず」)や意味の反転は、機械では止められず、出力の差分を読む設計のまま(ADR-1387)
   - 再依頼(2 回目)は、前後の参考(`<before>`・`<after>`)を外して送る。応答に依頼の書式が混ざる型の失敗は、参考の欄の書式をなぞることで起きるため(F17・F21・F59)
7. 通った段落は伏せ字を戻して差し替え、`records.jsonl` に書く。

## textlint の自動修正(Gemini の前後)

上の流れの前と後に、textlint の自動修正(`--fix`)を挟む(ADR-1387 の D13、`crates/ja-polish/src/textlint.rs`)。
順は「前段の textlint → Gemini → 後段の textlint」。

- 前段は、伏せ字にした段落に当てる。直した本文を Gemini に送り、戻りの照合(上の 6)も、直した本文を基準にする。
- 後段は、Gemini が書き直した段落に当てる。Gemini が持ち込む冗長表現を直す。
- Gemini が通せなかった(素通しの)段落にも、前段の直しは残る。最終の本文は前段が直したあとの本文で、記録の
  `sha` もそれ。報告には「textlint の直しだけ入った」と差分が出る。
- 有効にする規則は、`textlint-rule-preset-ja-technical-writing` の `ja-no-redundant-expression` だけ(「することが
  できる。」→「できる。」)。`prh` は、private の文体規約の隣に `docs/style/prh.yml` があるときだけ使う。
  自動で直せる範囲は狭い(2026-11-12 の実測: 句点の無い箇条書きの「…ことができる」と「通信を行う」は検出だけ)。
- textlint の直しは、Gemini の出力と同じ照合(伏せ字・数値・記法・分量)を通るものだけを採る。`textlint` が無い・
  失敗・時間切れ(30 秒)のときは、入力のまま。止めない。
- 設定の `textlint = false`(home-manager は `dotfiles.jaPolish.textlint.enable`)で切れる。環境変数
  `JA_POLISH_TEXTLINT_BIN` で実行ファイルを、`JA_POLISH_PRH_FILE` で辞書を差し替えられる(テスト用)。

## 測定(`ja-polish bench`)

思考量・モデルを、速さだけで選ばず、同じ見本で時間と質の物差しを並べて選ぶ(ADR-1387 の D11・D15、
`crates/ja-polish/src/bench.rs`)。

```
ja-polish bench crates/ja-polish/bench/samples.txt [--variants model[@level],…]
```

- 見本は `crates/ja-polish/bench/samples.txt` の 5 本(`<commit>:<path>`。commit を固定して、文書が変わっても測る対象が
  動かない)。組の既定は、今の設定のモデルを「設定の思考量・指定なし・minimal・low」(同じ組は重ねない)で、それに予備のモデルを指定なしで 1 つ。
  `--variants` で、カンマ区切りの `model` か `model@level` を指定できる。
- 書き直しは記録を使わず(すべて未通過として扱う)、何も書き換えず、記録もしない。予備のモデルと textlint の前後段は
  挟まない(モデル自身の出力を測る)。textlint は、指摘数を数えるためだけに使う(無ければ「未計測」)。
- 表の物差し: 時間(秒)、書き直し・変更なし・素通し(かっこ内は混雑・時間切れによるもの。多い組は、混雑に左右された
  比較なので、測り直す)、全角括弧の増減(文体規約に指定が無いのに半角を全角にする劣化の数。括弧の幅の変化の物差し)、
  **用語の置き換え**(書き直しが、元の語を別の語に変えた回数。`bench::WATCHED_SWAPS` の組で数える。語は 2026-11-12 の実測
  から始める: 機体→マシン、会社契約→法人契約、戻す条件→復元条件、通さない→送信してはならない。組ごとに、元の語が減り、
  持ち込まれた語が増えた回数のうち少ない方)、**変更行の割合**(答えた段落のうち、書き直しの後に、前の本文に無い行の割合。
  空行は数えず、行は空白をそろえて比べる。高いほど、直す必要が無い所まで書き換えている)、冗長表現の指摘数の前後。
  用語の置き換えの語の組は、測定の入力を増やすときに足す。辞書(prh)に載せる語の選定(本人の文体の判断)とは別。
- **取りこぼしの率**(ADR-1387 の D18): 見本の一覧の行に `<commit>:<path> <ref-commit>` と書くと、見本の段落のうち、
  `<ref-commit>` の同じパスの文書に(空白をそろえて)残っていない段落が「直すべき段落」になる。表に「取りこぼし」の列が足され、
  モデルが答えた直すべき段落のうち、本文を変えずに返した段落の割合が出る(`2/10 (20%)` なら、10 段落のうち 2 段落)。**低いほど良い**。
  上の物差しは、変更なしの段落が多いほど「安全」に見えるので、劣化したモデルが何も直さずに返しても気づけない。この列はそれを数える。
  素通しは分子にも分母にも入れない。参照を持つ見本が無ければ、列は出ない。
  参照はエージェントが直した結果で、正解ではない。この列は「直すべき所に手を付けたか」だけを測り、直し方の良し悪しは測らない
  (質は、用語の置き換え・全角括弧の増減・変更行の割合と、差分を読むことで見る)。
  参照つきの見本は `crates/ja-polish/bench/needs-fix.txt`(PR #1738 の base と head。head は `git fetch origin pull/1738/head` で取れる):
  `ja-polish bench crates/ja-polish/bench/needs-fix.txt --variants …`。
- 1 組ごとに数十秒〜数分かかる。見本を入れ替えたときと、モデルが変わったときに、測り直す。

## 記録

`~/.local/state/claude/ja-polish/records.jsonl`(`$XDG_STATE_HOME` があればその下)。1 行 1 段落:

```json
{"ts_unix":1793000000,"sha":"<段落の正規化した本文の SHA-256 の先頭 32 桁>","state":"polished","model":"gemini-3.8-flash"}
```

- `sha`: 段落の**最終の**本文(空白をそろえてから)。書き直した段落は書き直した後の本文、素通しの段落は元の本文。
- `state`: `polished`(書き直した、または直す必要が無いと確かめた)か `skipped`(素通し。`reason` つき)。
- `changed`: 書き直した段落で、モデル(と後段の textlint)が、前段の直しのあとの本文を変えたか(`true`・`false`)。
  素通しの記録と古い記録には無い。`polished` は「変えずに返した」段落も含むので、劣化したモデルが何も直さずに
  返しても通った段落に数えられる。この印と `model` から、予備のモデルが答えた割合と、変えずに返した割合を数え
  (`records::quality_rate`)、予備の割合が高いときはセッションの冒頭で知らせる(`gate-failopen-advisory`、ADR-1387 の D17)。
- 関所(投稿の PreToolUse = 「投稿の関所」、文書ファイルの Stop = 「文書ファイルの関所」)は state を区別せず、`sha` があれば通したとみなす。
  素通しで止めないための設計で、素通しは次の gate イベントが知らせる。

## 投稿の関所(`ja-polish-gate`)

`hook-router` の PreToolUse の規則(`crates/hook-router/src/rules.rs`、判定は `crates/ja-polish/src/gate.rs`)。
本文に、記録に無い日本語の段落が 1 つでもあれば、呼び出しを実行前に拒否する。LLM は呼ばず、記録を引く
だけなので、ルータの持ち時間(10 秒)に収まる。

- 見る対象: `gh pr|issue create|edit|comment` と `gh pr review` の `--body` / `--body-file`、`gh api` の
  `-f body=…` / `--input`、`git commit` の `-m` / `--message` / `-F` / `--file`。MCP は Gmail の下書き・返信・
  送信(`body`、無ければ `htmlBody`。件名は見ない)、Slack の下書き・送信(`message` `text` `body` `content`
  `markdown`)、Claude Docs の作成・更新・一括(文字列すべて)。
- 通すもの: 設定ファイルが無い機体、本文に `No-Attribution:` の行がある(本人の文の代理投稿)、
  環境変数 `SKIP_JA_POLISH_GATE=1`(ja-polish が壊れたときの逃げ道)、本文が取れないもの(`--body-file` が
  読めない、知らない形の MCP 入力)。判定不能を拒否に変えない。
- **`gh issue edit` / `gh pr edit` は、いまの本文を基点にする**(ADR-1387 の D5 の「変更した段落だけ」を、git の外の
  本文に広げた): 本文は git の外にあって基点が無く、長い Issue の本文(確認の台帳など)に 1 行足すだけでも、承認済みの
  段落ぜんぶの通過を求めていた。未通過の段落があるときだけ、`gh <issue|pr> view <番号> [-R <repo>] --json body --jq .body`
  でいまの本文を引き(上限 4 秒。CRLF は LF にそろえる)、そこにある段落は数えない。足した・変えた段落だけが対象になる。
  引けないとき(認証・ネットワーク・時間切れ)と、番号を省いた形(現在のブランチの PR)は、基点なしの従来の判定のまま。
  `create`・`comment`・`review` は、いまの本文が無いので引かない。`gh api` の本文の更新も引かない(未実装)。
- **ja-polish が動けない間も通す**(Stop の関所も同じ): 記録(`records.jsonl`)の直近 5 件がすべて素通し(`skipped`)で、
  最後の 1 件が 1 時間以内なら、動けないとみなす(API キーが無い、LLM に届かない、など。`records::unavailable`)。
  無料枠の **1 日の上限**に当たった素通し(記録の `"daily_limit":true`)は、件数を待たない: 最後の 1 件がその印のある素通しで
  1 時間以内なら、動けないとみなす(上限に当たった最初の往復では、書き直しの記録が並びに混ざって 5 件に届かず、関所が
  何も変わらない往復を求めたため。ADR-1387 の D16)。混雑・時間切れの素通しには、この印を付けない。
  本文を書いて `ja-polish` を通して出し直す往復を求めても、結果は全部素通しで、書き直しは 1 件も起きないからである
  (キーの無い機体で、コミットメッセージ・PR 本文・Issue コメント・Gmail の下書き・プランのたびに往復を求めた)。
  1 時間たつと元に戻り、次の 1 回の往復が、直ったかの確かめになる(直っていれば書き直しの記録が入り、素通しの並びが切れる)。
  動けない状態そのものは、`gate-failopen-advisory` がセッションの冒頭で知らせる。
- 本文の組み立てを自前にした理由(`github-command` の `assemble_body_text` を使わない): 断片どうしを空行で
  区切る(共有の関数は改行 1 つで繋ぐので、前後の断片と繋がって別の段落になり、記録のハッシュと合わない)。
  また、インラインの `--body` が `$(` やバッククォートを含んでも判定する(共有の関数は「中身が不明」で
  `None` にする)。なお、共有の関数は以前、`--body-file` の中身がバッククォートを含むだけで `None` にして
  いて、`pr-followup-guard` と `attribution-guard` が効いていなかった。これは共有の関数の側で直した
  (判定不能の規則を `--body` の値だけに限った)。
- 拒否の文は、`ja-polish <ファイル>` で書き直す手順と、`ja-polish` を単体で呼ぶ理由(リダイレクトで sandbox の
  中に残る)を示す。
- 対象外(意図あり・未実装): Codex の MCP ツール名(`mcp__codex_apps__…`)、Gmail 以外のメール、Google Drive の
  ファイル作成、カレンダーの説明欄。

## 文書ファイルの関所(`ja-polish-stop-gate`)と基点

セッションの終わり(Stop)に、触れた日本語の文書に、通していない段落が残っていないかを記録だけで見る
(`ja-polish stop-gate`、判定は `crates/ja-polish/src/stop.rs`)。LLM は呼ばない。

- 見るファイル(拡張子 `.md` `.markdown` `.txt`)は、次の和集合: このセッションで `Write` / `Edit` /
  `MultiEdit` した文書、cwd の git の作業ツリーで origin の既定ブランチとの merge-base から変わった、または
  未追跡の文書(上限 40 件)。各ファイルは、基点に無い(追加・変更した)日本語の段落のうち、記録に無い
  ものを数える。1 つでもあれば `{"decision":"block"}` を出し、`ja-polish <ファイル>` を促す。
- **無限に止めない**: `stop_hook_active`(block の直後の再呼び出し)では黙る。通せなかった段落は `ja-polish`
  が素通しとして記録するので、直せない状態でも止まり続けない。逃げ道は `SKIP_JA_POLISH_GATE=1`。
- **基点のスナップショット**(PreToolUse の規則 `ja-polish-snapshot`): git の外の文書(vault のノートなど)には
  merge-base が無い。`Write` / `Edit` / `MultiEdit` の前に、そのファイルの内容を
  `~/.local/state/claude/ja-polish/base/<パスのハッシュ>.snap` に残し(12 時間以内の新しいものがあれば
  上書きしない)、触れた文書の一覧を `touched/<session_id>.txt` に足す。作業ツリーには何も書かない。
  まだ無い(これから `Write` で作る)ファイルの基点は、空にする。残さないと、続く `Edit` の前(= `Write` した後の
  内容)が基点になり、`Edit` した段落だけが対象になって、残りの段落が通っていないまま `gh` の本文の関所に当たる
  (scratchpad で `Write` → `Edit` → `ja-polish` と進めたときの症状)。
  基点の優先順位は、git の merge-base、スナップショット、なし(文書全体が対象)。セッション ID を使わない
  のは `ja-polish` の CLI が ID を知らないため。基点が古い側に寄っても、対象が増えるだけで、通した段落は
  記録で飛ばすので書き換えは増えない。
- **基点があるときは、箇条書き・番号付きを項目ごとに比べる**(`blocks::split_items`): 空行で区切られた箇条書きは
  1 段落なので、承認済みの 7 項目に 1 項目足しただけで、段落全体(7 項目)が「変更した段落」になり、書き直しの対象に
  なっていた(2026-11-12 の実測)。基点があるときは、`-`・`*`・`+`・`1.`・`1)` で始まる行ごとに別の段落にして(導入の行も
  1 つの段落。記号の無い折り返しの行は前の項目に付く。ネストした項目も別。フェンスは分けない)、基点の同じ単位と比べる。
  足した・変えた項目だけが対象になり、モデルへの依頼にも、承認済みの項目の本文は入らない(隣の項目は文脈として見せる)。
  区切りは各行末の改行で、分けても元の文書に戻る。基点が無いとき(scratchpad の本文など)は、従来どおり箇条書き全体が
  1 段落。ただし、日本語の項目がすべて記録にある箇条書きは、記録にあるとみなす(基点つきで項目ごとに通した文書を、基点なしで
  見る関所が、同じ箇条書きをもう一度求めない)。**表は、書き直しの対象にしない**(空でない行がすべて `|` で始まる段落。`blocks::is_table`。
  セルの切れ目を書き直させると行頭の記法がずれ、依頼の書式も応答に混ざりやすい。2026-11-16 に、表のある計画ファイルの 8 段落が素通しになった)。
  引用(`>`)の行は、段落のままで、`>` の行を丸ごと伏せ字にして書き直しに渡さない(手順 3)。
- 登録: Stop hook は `home/modules/agents.nix` の宣言(`dotfiles.jaPolish.enable` の機体だけ)。Codex には
  Claude の宣言から導く規則でそのまま登録される。

## 書き直してはいけない文書(front matter の宣言)

第三者の問題文の逐語の写し、本人のメモ、生成物、追記専用の記録は、書き直すと中身が変わる(外部のサービスへ送れない
文章もある)。そうした文書は、先頭の front matter に `ja-polish: skip` を書く(F19・F38・F58)。

```yaml
---
ja-polish: skip
---
```

- 宣言した文書は、書き直し(`ja-polish`)・Stop の関所・`--check`・投稿の関所のすべてが対象にしない。モデルにも送らない。
  入口は `polish::pending_indexes` の 1 か所で、全経路がここを通る。
- 値の大文字・小文字と引用符は問わない。**front matter の外**(本文や、閉じる `---` の後ろ)の同じ行は、宣言と読まない。
  別の値(`enable` など)も宣言でない。
- セッション全体の逃げ道 `SKIP_JA_POLISH_GATE` との違い: あちらは、そのセッションの関所を全部飛ばす(文書を選ばない)。
  文書の性質で決まる除外には、こちらの宣言を使う。宣言は、書き写す側(問題文・レシピの取り込みなど)が、ノートを作るときに前付けへ書く。
- 計画ファイル(`~/.claude/plans`)は、対象のままである(ADR-1387 の「人が後で読む文書すべて」を保つ。2026-11-20 の裁定)。
  書き直しで変更なしだった段落も記録に載る(`Entry::polished` が、変更の有無によらず本文の sha を書く)ので、同じ段落で
  関所が繰り返し止めることは無い。計画を直すたびに、直した段落だけが新たに対象になる。表だけの段落は、対象にならない(#1623)。
- 未解決(F19): 追記専用の ADR で、書き直しが既存の段落の導入の 1 行を複製した、という報告がある。再現は取れていない
  (段落の分割 `blocks::split_with_items` の項目の扱いが原因かは未確認)。宣言で、そのような文書を対象から外せる。

## 失敗したとき

止めない(fail-open)。失敗した段落は**元の本文のまま**残り、`skipped(<理由>)` で記録される。理由の例:
`llm が見つかりません`、`llm が失敗しました: Error: No key found …`、`45 秒以内に応答がありません`、
`スタイルガイドのハブを解決できません`、伏せ字・数値・記法・分量の照合に再依頼でも外れた。

素通しにする前に、依頼の失敗を 4 種に分けて手を打つ(判定は `crates/ja-polish/src/llm.rs` の
`classify_stderr` の 1 か所。標準エラーの先頭 2000 字の語で決める):

| 種類 | 例 | 手 |
|---|---|---|
| 混雑(`Busy`) | `high demand`・429・503・`RESOURCE_EXHAUSTED`・通信の不調 | 同じモデルに最大 `retries`(既定 2)回まで再依頼(待ち 2 秒→6 秒、±25%)。通らなければ予備のモデルに 1 回 |
| 無料枠の 1 日の上限(`DailyLimit`) | `requests per day limit exceeded on Free Tier`・クォータ ID の `PerDay`(実測 2026-11-15: `Please retry in 2h38m48s`)。毎分の上限(`PerMinute`)は混雑のまま | 同じモデルは再依頼せず(間を置いても、次の再開まで通らない)、すぐ予備のモデルに 1 回(F6)。予備が無ければ 1 回で素通し(`transient`) |
| 時間切れ(`Timeout`) | `45 秒以内に応答がありません` | 同じモデルは待ち直さず、すぐ予備のモデルに 1 回 |
| 恒久(`Permanent`) | `No key found`・`llm が見つかりません`・不明なモデル名 | 再依頼も予備もなし。素通し |

予備のモデル(既定 `gemini-4-argon`)には、思考量の指定を渡さない。記録の `model` は、実際に答えた
モデル。混雑・1 日の上限・時間切れで通せなかった素通しの記録には `"transient":true` が付く(キーの未設定や照合の外れには
付かない。課金の検討を知らせる判定の入力)。1 つの束が失敗しても、ほかの束は書き直す。

**素通しの段落は、`ja-polish` の次の実行で、もう一度書き直しの対象になる**(ADR-1387 の D12)。書き直しは
`state` が `polished` の記録だけを「通した」とみなし(`records::load_polished`)、関所と `--check` は素通しも
「通した」とみなす(`records::load`)。キーを入れた・混雑が収まったあとは、同じ文書をもう一度 `ja-polish` に
通せば、素通しだった段落が書き直される。キーが無いまま再実行すると、同じ段落をまた送って素通しの記録が 1 行増える。

### 素通しの理由と、依頼・応答の記録(原因の調査)

照合([`polish::verify`])に再依頼でも外れた素通しの理由には、**答えたモデル**を添える(`…が変わりました(モデル gemini-3.6-flash)`)。
応答の書式が崩れたとき(`応答に依頼の書式が混ざっています`、`応答に該当する段落がありません`)は、さらに**応答の先頭 200 字**
(改行は `⏎`)も添える。伏せ字の欠けなどの理由には、段落の本文を載せない。

**全段落が「応答に依頼の書式が混ざっています」で素通しになる症状の原因**(2026-11-13 に 1 回、11-14 に同じセッション内で連続 3 回、
11-15 に再現): 主のモデル(`gemini-3.6-flash`)が無料枠の上限(`You exceeded your current quota`)で、再依頼でも失敗し、予備のモデル
(`gemini-flash-lite-latest`)に切り替わる。予備のモデルが、応答の書式(`<para>` だけ)を守らず、**依頼文の `<text>` の囲みごと返す**
(`<para n="1">⏎<text>⏎本文⏎</text>⏎</para>`)。囲みの残る応答は、照合が書式の混入として落とす。2026-11-15 に、`--trace` で取った
4 件の依頼と応答(主のモデルが 3 回 `exit_code` 1・`Quota exceeded`、予備のモデルが `ok`。5 段落の応答のすべてが `<text>` で
囲まれていた)で確かめた。無料枠の上限そのものは、この囲みの除去と別の問題で、課金の検討の知らせ(`gate-failopen-advisory`)の対象。

応答のうち、`<para>` の中身全体が `<text>…</text>` の囲みなら、囲みを外して読む(`llm::unwrap_text`)。上の症状は、これで通る
(同じ PR 本文が、囲みを外す前は 5 段落すべて素通し、外したあとは通った)。`<before>`・`<after>`(参考の段落)が混ざる応答は、
本文を取り違えうるので外さず、書式の混入として外れる。

`llm` へのすべての依頼と応答は、`ja-polish --trace <dir> <ファイル>`(または環境変数 `JA_POLISH_TRACE_DIR=<dir>`)で、
`<dir>` に **1 依頼 1 ファイル**(JSON、所有者だけが読める)として書ける。既定では何も書かない。書くのは、`model`・`thinking_level`・
`timeout_secs`・`outcome`(`ok`・`failed`・`timeout`)・`exit_code`・依頼文(`user`)・`stdout`・`stderr`、指示文の長さと SHA-256
(指示文そのものは、文体規約の全文を含むので書かない)。依頼文と応答には文書の本文が入る(コード・URL は伏せ字)ので、`<dir>` は
共有しない場所(scratchpad など)に置く。`JA_POLISH_LLM_BIN` を `tee` を挟むラッパーに差し替える方法は、`tee` が終了コードを
上書きして `llm` の失敗が成功に見えるので使わない(2026-11-12 の実測)。`ja-polish` は単体で呼ぶ(`--trace` は引数なので、コマンドが
`ja-polish` で始まる形のまま、sandbox の外で動く)。

`~/.local/state/claude/gate-events.jsonl` に、書き直した回ごとに `ja-polish` の `fail-open`(1 段落でも
素通しがあった)か `pass` を残す。`gate-failopen-advisory` が、`pass` を挟まずに 3 回続けて `fail-open` の
とき、次のセッションの冒頭で警告する(`GATE_FAILOPEN_THRESHOLD` で変えられる)。

## 設定とキー

- 設定ファイル: `~/.config/dotfiles/ja-polish.toml`(home-manager が置く)。`model`(既定
  `gemini-3.8-flash`。有料枠のキーが前提で、理由は下の「モデルの選択」)、`fallback_model`(既定
  `gemini-4-argon`。空の文字列で使わない)、`thinking_level`(`minimal` `low` `medium` `high`。
  既定は `low`。空の文字列は、モデルの既定に任せる)、`timeout_secs`(既定 45)。ほかに `chunk_paras`(8)・`parallel`(3)・`retries`(2)・
  `retry_base_ms`(2000)・`textlint`(既定 true)が書けるが、home-manager の option(`dotfiles.jaPolish.fallbackModel`・
  `thinkingLevel`・`timeoutSecs`・`textlint.enable`)があるのは、`chunk_paras`・`parallel`・`retries`・`retry_base_ms` を除く項目だけ。**このファイルの有無が、その機体で有効かどうかの
  唯一の印**。無ければ関所は何も検査しない(置いていない機体では、関所も LLM への送信も起きない)。環境変数 `JA_POLISH_MODEL` か、引数 `--model <名前>`(環境変数の前置が `sandbox-compound-guard` に拒否される場面の代わり。
  `--model` が環境変数より優先)で、その場だけ上書きできる。
- API キー: 各機体で一度 `llm keys set gemini` を手で実行する。キーは `llm keys path` が指す
  `keys.json`(Linux は `~/.config/io.datasette.llm/`、macOS は `~/Library/Application Support/
  io.datasette.llm/`)に入る。そのディレクトリは `agentPermissions.secret_paths` に入っていて、Claude の
  sandbox の中のコマンドと Read ツールから読めない。
- キーは Google AI Studio で発行する。**無料枠は、送った文面が Google の製品の改善に使われ、人が読み
  うる契約**(Google, Gemini API Additional Terms of Service、更新 2026-03-23、
  https://aistudio.google.com/docs/terms 、取得 2026-11-08)。**有料枠は、そうならない**(Google, "Billing"、
  https://aistudio.google.com/docs/billing 、取得 2026-11-27。原文: "ensure your prompts and responses are
  **not** used to improve Google products")。vega のキーは、2026-11-28 に有料枠に上げた(下の「モデルの選択」と
  「人手の作業」)。キーが属するプロジェクトを課金枠にするので、機体ごとにキーが別なら、機体ごとに上げる。
  この経路(個人のキー)で、勤務先の文書は通さない。会社契約の送り先は、別の経路として要件を
  確かめてから足す(「横展開」)。

### モデルの選択

既定は `gemini-3.8-flash`、思考量 `low`、予備 `gemini-4-argon`(ADR-1387 の D19)。有料枠のキーが前提で、無料枠のキーに戻すときは
`model` を `gemini-3.6-flash` に替える(下の「無料枠のとき」)。

- 経緯: 無料枠では、モデルごとに 1 日 20 リクエストの上限があり、使い切ると予備の軽量モデルに流れる。2026-11-27 の集計(vega、
  直近 7 日)で、書き直した 5,009 段落の 89% を予備(`gemini-flash-lite-latest`)が答え、何も直さずに返した段落も「通した」と
  記録されていた(D17)。
- 測定(2026-11-28、vega、`ja-polish bench crates/ja-polish/bench/needs-fix.txt`。PR #1738 の 5 本・227 段落、旧の指示文は D20 の前):

  | 組 | 指示文 | 時間(秒) | 素通し(混雑・時間切れ) | 取りこぼし |
  |---|---|---:|---:|---:|
  | 3.6-flash@既定 | 旧 | 343 | 28 (0) | 19/140 (14%) |
  | 3.8-flash@既定 | 旧 | 334 | 5 (0) | 79/155 (51%) |
  | 4-argon@既定 | 旧 | 188 | 5 (0) | 70/156 (45%) |
  | 3.6-flash@既定 | 新 | 337 | 12 (8) | 28/154 (18%) |
  | 3.8-flash@既定 | 新 | 374 | 20 (16) | 19/143 (13%) |
  | 4-argon@既定 | 新 | 185 | 4 (0) | 39/156 (25%) |
  | 3.8-flash@既定(単独で再測定) | 新 | 415 | 37 (32) | 20/130 (15%) |
  | **3.8-flash@low(単独)** | 新 | **102** | **2 (0)** | 37/158 (23%) |

  用語の置き換えと全角括弧の増減は、どの組も 0。**1 回ずつの測定で、ばらつきは測っていない**(同じ組の 2 回の測定で、素通しが 20 と 37 と
  動いた)。既定の思考量だと、3.8 は 45 秒の時間切れが多発する(素通し 37 のうち 32)。`low` は 102 秒で終わり、素通しが 2 件に減った。
  取りこぼしの割合は、答えた段落の中の割合で、素通しの段落(分母に入らない)が多い組ほど、実際の取りこぼしより小さく出る。
  直した段落の絶対数は、`low`(158 − 37 = 121)が既定(130 − 20 = 110)より多い。`minimal` は測っていない(未測定)。
- 予備を `4-argon` にした理由: 新しい指示文で、時間切れが 0 件、素通しが 4 件であり、取りこぼしが軽量モデルより少ないため。単価は高い(入力 $2・出力 $10 /
  100 万トークン。2026-12-01 から $4・$20。Google, Gemini API Pricing、https://aistudio.google.com/docs/pricing 、取得 2026-11-28。
  原文の一部は要約でしか読めていない)が、予備は主モデルが混雑・時間切れのときだけ使う。
- 単価(同じページ、取得 2026-11-28): `gemini-3.8-flash` は入力 $0.75・出力 $3.75 / 100 万トークン(2026 年末まで。2027-01-01 から
  $1.50・$7.50)。思考トークンは出力単価で計算される。月の目安は $30 前後(推定。ADR-1387 の見積もりのまま)。

#### 無料枠のとき

- 無料枠のキーでは、`gemini-3.8-flash` は混雑で応答が返らない(2026-11-12、vega、`llm prompt -m <モデル> -n --no-stream
  'こんにちは、と返してください。'` を直接実行。3.8・3.7 は 60〜120 秒待っても応答がなく、ja-polish 経由では `high demand`。3.6 と
  `gemini-flash-lite-latest` は即座に応答した)。Google AI Studio のステータスページは All Systems Operational で、同ページに「無料枠の
  リクエストは優先して切り捨てられる枠を使い、課金枠は優先して守られる」と記載がある(https://aistudio.google.com/status、取得 2026-11-12。
  後で再確認できていない: 同日にブラウザで開くと「Failed to list incidents history: authentication error」と表示され、本文が読めなかった)。
  混雑の理由自体は未確認。
- 無料枠の 1 日の上限(実測 2026-11-15、vega、`llm prompt -m gemini-3.6-flash -n --no-stream …` を直接実行):
  `You exceeded your current quota … Quota exceeded for metric: generativelanguage.googleapis.com/
  generate_content_free_tier_requests, limit: 20, model: gemini-3.6-flash, description: Generate content
  requests per day limit exceeded on Free Tier. … Please retry in 2h38m48s` が返った。無料枠は、モデルごとに
  1 日 20 リクエストで、使い切ると、その日は(次の再開まで)そのモデルが使えない。上限に当たったときの
  `ja-polish` の動きは「失敗したとき」の節を参照。
- 有料枠にしたあとの確認(2026-11-28、vega): `llm prompt -m gemini-3.8-flash` が 4 秒で応答し、無料枠の上限に当たっていた
  `gemini-3.6-flash` も応答した(課金の反映が通常 10 分以内、という公式の説明と一致する。API keys ページの「Plan」の欄が `Paid` に
  なるかは、画面で確かめる)。

## なぜ Gemini CLI でなく llm か

依頼は Gemini CLI を名指ししていたが、次の理由で `llm` + `llm-gemini` にした(中のモデルは同じ Gemini)。

1. Gemini CLI の「Login with Google」は 2026-06-18 に廃止済み(Google, Gemini Code Assist for
   individuals privacy notice、https://developers.google.com/gemini-code-assist/resources/privacy-notice-gemini-code-assist-individuals 、
   取得 2026-11-08)。残るのは API キーか Vertex AI。
2. 「日本語を入れて日本語を出す」フィルタに、ファイルを読む・コマンドを走らせる・Web を見る道具は
   要らない。Gemini CLI はそれらを内蔵したエージェントで、止めるには設定で外すしかなく、文書に紛れた
   指示文に乗っ取られる余地も残る。`llm` は標準入力を受けて文字列を返すだけで、道具を渡さない限り
   そういう動きが構造的にできない。

### llm の版の固定(`nix/ja-polish-llm.nix`)

固定中の nixpkgs の `llm` は 0.30、`llm-gemini` は 0.31 で、`gemini-3.8-flash` を呼べない(対応は
llm-gemini 0.34 以降)。そのため overlay で、`llm-gemini` 0.35 と、それが要る `llm` 0.32、さらに
`llm` 0.32 が要る `sqlite-utils` 4.2.1 と `condense-json` 1.1 を、commit で固定している。
`llm` の最新(0.37.1)は `openai>3` を要求するが nixpkgs は 2.33.0 なので、0.32 を選んだ。
4 つとも nixpkgs が追いついたら、overlay ごと外して `llm.withPlugins { llm-gemini = true; }` にする。
テストは走らせない(`llm-echo` と録画した通信が前提)。代わりにビルド時の `pythonRuntimeDepsCheck` と
`pythonImportsCheck` が走り、2026-11-08 に `llm models` が `gemini/gemini-3.8-flash` を返すことを
確かめた。

## 人手の作業(キーを入れる)

実装の PR がマージされ `hms` が当たったあと、各機体(vega・altair)で一度だけ。

1. ブラウザで https://aistudio.google.com/api-keys(「API Keys」ページ)を開き、**Create API key** を
   押す。プロジェクトが一覧に無い場合は、Dashboard → Projects の **Import projects** で取り込んでから、
   再度押す(Google, "Using Gemini API keys"、https://aistudio.google.com/docs/api-key、取得
   2026-11-12)。2026-11-12 に vega でこの手順により発行でき、画面表記が違うという報告は無かった。
   プロジェクトの選択画面の表記は未確認。新規作成時の名称は、`docs/personal-cloud-projects.md` の
   汎用規則(`<tool>-<github-username>`)を転用して `ja-polish-<github-username>` とする(その規則は
   OAuth クライアントを想定した表であり、API キーへの適用は転用)。
   **有料枠にする手順は、下の「有料枠にする」**(2026-11-28 に vega でこの手順を踏んだ)。
2. `llm keys set gemini` を実行し、プロンプトに貼る。
3. 端末で `echo '日本語を直すことができる。' > /tmp/t.md && ja-polish /tmp/t.md && ja-polish --check /tmp/t.md`
   を実行する(人の端末なので連結してよい)。確認する値: 書き直した差分が出て、最後に「未通過の日本語の
   段落はありません」と出る。

### 有料枠にする

キーが属するプロジェクトを課金枠に上げる(ADR-1387 の D19)。クレジットカードと事前入金が必要で、入金は返金できず 1 年で失効する
(戻せない: 外部契約)。画面の表記は、2026-11-28 に vega で見たもの。

1. https://aistudio.google.com/projects を開き、キーのプロジェクトの行の **Billing Tier** 列の **Set up billing** を押す。
2. 「Cloud 請求先アカウントの設定」で請求先アカウントを選ぶ。既存のアカウントを選ぶと「この請求先アカウントに割り当てられている
   プロジェクトの数が上限に達しています」(システムエラー)になることがある(実測)。その場合は **新しい請求先アカウントを作成** を選ぶ
   (**プロジェクトのリンクを解除** は、既存プロジェクトの課金が止まる可能性があるため避ける)。
3. **AI Studio の画面から作成すると、国の選択欄が出ず、住所が米国形式(市区郡・州・郵便番号)になる**(実測)。そこで入力を進めず、
   Cloud コンソールの https://console.cloud.google.com/billing から「アカウントを作成」を押す。国を選ぶ欄が表示され、「日本」を選べた(実測)。
   国と通貨は作成後に変更できない(Google, "Create a new Cloud Billing account"、https://docs.cloud.google.com/billing/docs/how-to/create-billing-account 、
   取得 2026-11-28)。お支払いプロファイルの種類は「個人」(作成後に変更できない)。
4. AI Studio に戻り、新しい請求先アカウントで事前入金(Prepay)する。選択できる金額は ¥800(最小。公式の最小額 $5 に相当すると読める)・¥3,200・
   ¥16,000・その他。**オートチャージは画面の既定が ON(「推奨」)だが、OFF にして単発で支払う**: ON のままだと、不具合などで
   使用量が増えた際、承認なしでカードに繰り返し請求される。「月間の上限(省略可)」の欄に `¥0.00` が入っており、「上限が無いときは空白」と
   あるため、`0` が上限なしなのか上限 0 円なのかは画面から判断できない(未確認)。オートチャージを ON にするときは、月の使用量を確認したうえで、
   月間の上限を明示的な金額に設定する。
5. 反映は支払いの確定から通常 10 分以内(公式の説明)。確認は、上の「無料枠のとき」の末尾にある `llm prompt` と、API keys ページの「Plan」欄で行う。
   残高が 0 になるとキーが停止し(HTTP 402)、ja-polish は段落を素通しにして記録する。

## 後続(意図は決まっていて、未着手)

- 会社の機体(arcturus)への横展開(#1385)。下の「横展開」の確認事項を調べるところから。

## 横展開(会社の機体・ほかの identity)

`ja-polish` は機体にも identity にも縛られない。今は個人の identity だけで有効にしているが、それは
**送り先の扱い(送った文面が学習に使われるか)を確かめ済みの経路が、個人の無料 API キーだけ**だから
で、会社を除外する理由があるわけではない。会社の側は、会社契約の Claude Code から会社契約の Gemini へ
つなげられる見込みで、Gemini 側の要件はまだ調べていない。

横展開の手順(確認事項が済んだあと):

1. 確かめる: 会社契約の Gemini の接続方法(Vertex AI、Workspace など)と認証、送った文面が学習に使われ
   ないことの根拠(契約書・公式ヘルプ)、人が読みうるか。
2. 送り先に合う `llm` のプラグインを選び(採否は保守状況・API の要件を一次ソースで確かめる)、
   `dotfiles.jaPolish.package` にその環境(`llm` と送り先のプラグイン)を、`dotfiles.jaPolish.model` に
   モデル名を指定する。
3. 会社の identity で `dotfiles.jaPolish.enable = true;` を立てる。設定ファイルが置かれて関所が有効に
   なる。`ja-polish` 本体・記録・関所・失敗時の素通しは、そのまま使える。
4. 認証の置き場が `llm keys` と違うとき(Application Default Credentials など)は、その保存先を
   `agentPermissions.secret_paths` に足す(秘密パスの基準は `docs/claude/claude-permissions.md`)。
   会社固有のパスは、このリポジトリ(公開)でなく private の wrapper に置く(ADR-0034)。

## 既存手段と代替

- 既存手段: 採用 — `llm`(https://github.com/simonw/llm)+ `llm-gemini`。記録の型・段落の分割・
  伏せ字・照合は自前(`crates/ja-polish`)。
- 対抗馬: Gemini CLI(上の理由で次点)。Google の API を Rust から直接呼ぶ(依存の版固定は要らなく
  なるが、複数のモデル・プロバイダを同じ呼び方で切り替えられる `llm` の利点を失う。会社側の送り先を
  後で差し替える前提と合わない)。
- 先行例: DeepL, Translating XML(https://developers.deepl.com/docs/xml-and-html-handling/xml 、
  取得 2026-11-08)は、構造を保つ翻訳 API で `ignore_tags` に訳さない部分を指定する。本機構は API の
  機能に頼らず、送る側で伏せ字にして戻りを照合する。reviewdog の `filter-mode: added`
  (https://github.com/reviewdog/reviewdog 、取得 2026-11-08)は、追加・変更した行だけに指摘を絞る。
  本機構は同じ絞り込みを書き直しの対象に使い、前後の段落を参考として渡す。
