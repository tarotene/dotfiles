# メール裁定ページの形式を変えるときの決まり

メール裁定のページは、固定の 1 枚の Artifact を一括処理のたびに同じ URL へ再公開する。ページの `db` に
は過去の裁定が残り続けるので、形式が黙って変わると、残った文書の読み方がずれる。形式は、データ(その回の
項目の表)と分けて、このディレクトリに置く。

| ファイル | 持つもの |
|---|---|
| `template.html` | ページの形式。差し込み点は `__DATA__`・`__OPTIONS__`・`__FORMAT_VERSION__` の 1 か所ずつ |
| `options.json` | **選択肢の正本。** 項目の種類(`kind`)ごとに、ボタンの語、保存先へ書く値(`verdict`・`owner`)、押した後の「あなた」と「私」の分担を持つ。画面・保存先の形・読む側の契約は、この表から導く |
| `items.schema.json` | 生成器への入力(その回の項目の表)の形 |
| `verdicts.schema.json` | 保存先(`db`)に書く文書の形 |
| `build.sh` | 入力を検証し、テンプレートに差し込んで HTML を出す。`--selftest` が検査を回す |
| `schema-validate.jq` | `build.sh` が使う検証器(JSON Schema の部分集合。対応外のキーワードは違反) |
| `fixtures/` | `--selftest` の架空の入力 |

組み立ては `bash build.sh --data <items.json> --out <page.html>`。

## 決め方のモデル(形式 5)

画面の部品を、意思決定とその受け渡しの語彙から導く。項目ごとの裁定は、次の 2 つの軸で決まる。

- **どうするか(`verdict`)**: `decide`=やる、`pass`=やらない、`later`=あとで、`other`=その他。`other` は、どの選択肢にも当てはまらないときで、**記入欄(`note`)がそのまま本人の指示になる**。
- **誰がやるか(`owner`)**: `decide` のときだけ。`me`=本人、`agent`=おまかせ。

項目の種類(`kind`: `reply`・`notice`・`task`・`fact`・`report`)ごとに、この 2 軸の組を、その種類で意味の通る語のボタンにする(`options.json`)。たとえば同じ `pass` でも、返信なら「返信しない」、通知の確認なら「心当たりあり(対応不要)」になる。同じ語が、種類によって別の意味に読まれないようにするため。

- 各ボタンには、押した後の分担(「あなた」「私」)が付き、項目の枠の中と、ページ上部の「この選択のあとの分担」に出る。移譲(おまかせ)の中身と、本人に残る操作(送信前の確認など)を、決める前に見えるようにする。
- 推奨は `items[].rec_option`(選択肢の id)で指す。推奨のボタンに印が付く。選ぶのは本人で、推奨を既定にはしない。
- `other` は、記入欄が空のあいだ「指示待ち」で、読む側は裁定として読まない。`later` は何も記録しない(7 日で再通知)。
- 「選択を外す」は、`verdict` を空文字に戻す(読む側は裁定として読まない)。

読む側(机の手順 2b-0)は、保存先の文書を `worklog` の出来事に次のように写す(語の意味の正本は `options.json`)。

| `verdict` / `owner` | 出来事 |
|---|---|
| `decide` / `me` | `decide`(印 `owner:me`)。反映は本人が行い、エージェントが読み戻して `apply` で閉じる |
| `decide` / `agent`、`option` が `send-agent`(おまかせで送る) | `decide`(印 `owner:agent send:yes`)。次の机で送信直前の画面・全文・宛先を本人に見せ、OK を得たら `confirm` を書き、送ってから `apply` で閉じる。`confirm` の前の `apply` は `worklog` が拒否する |
| `decide` / `agent`、それ以外の `option`(調べてもらうなど) | `decide`(印 `owner:agent`)。机の中で実行し、`apply` で閉じる。操作が要る手続きは本人に渡す |
| `pass` | `pass` |
| `later` | 書かない |
| `other` | `instruct`(要旨に指示の結論)。エージェントの番になる。`note` の指示を机で読み、`decide`・`apply`・`pass`・`move` のうち指示に合うものを書く。読めなければ、本人に聞く |

語彙の版 4(`config/agents/mail-lifecycle/vocabulary.json`)で、担い手(`owner:`)・送信の印(`send:yes`)・指示の事象(`instruct`)・送信前の確認の事象(`confirm`)が入った。要旨の先頭の「おまかせ:」は要らない(担い手は `owner:` の印が持つ)。読む側(別リポジトリの机の手順)は、この表のとおりに写す。

### 事実の確認(`fact`)と完了報告(`report`)(形式 5、#1321)

裁定のあとにチャットで 1 件ずつ聞いていた、本人しか知らない事実(「荷物を受け取れたか」)と、本人の手作業の完了
(「フィルタの反映は済んだか」)を、同じページの項目にする。保存先の形(`verdicts.schema.json`)は変えない。既存の語
(`decide`・`pass`・`later`・`other`)に写し、**何の答えか**は `option`(選択肢の id)で区別する。

| kind | 選択肢の id | `verdict` / `owner` | 意味 |
|---|---|---|---|
| `fact` | `yes`(はい) | `decide` / `me` | 事実が成り立つ。問い(`title`)は「はい」が答えとして読める形に書く |
| `fact` | `no`(いいえ) | `pass` | 事実が成り立たない |
| `report` | `done`(済んだ) | `decide` / `me` | 本人の作業が済んだ |
| `report` | `dropped`(やめた) | `pass` | もう要らなくなった |
| 両方 | `later`(まだ) | `later` | 書かない。答えが要るあいだ、次の机でもう一度出す |
| 両方 | `other`(その他) | `other` | `note` の答えや状況を机で読む。読めなければ、本人に聞く |

- 項目は、メールのスレッドでなくてもよい。鍵の媒体は `none` を使う。鍵は答えの宛先で、同じ鍵の答えは上書きされる。
- この 2 つの kind の `decide`・`pass` は、やる・やらないではなく、答えである(`options.json` の `answer: true`)。画面の
  「この選択のあとの分担」では「答えを読んで直す(確認・報告)」に分かれ、「あなたがやる」には入らない。
- 読む側(別リポジトリの机の手順)は、`option` が `yes`・`no`・`done`・`dropped` の文書を、上の表の `decide`・`pass`
  の行(`worklog` の出来事を書く)に写さない。答えを読んで、その答えで変わる下書き・記録を直す。`done` は、読める範囲で
  済んだことを読み戻してから、対応する記録を `apply` で閉じる。`option` を見ずに `verdict` だけで写す読む側は、事実の
  答えを `decide` と読み違える。読む側を同じ変更で直す(上の決まり (a) の趣旨)。
- 答えを書く先の記録(`worklog` の事象)は、語彙に「事実の答え」が無いので、暫定である(語彙の版 4 でも入っていない)。

### 差出人の扱い(`sender`)と、推奨の一括採用(形式 6、#1342)

規則の無い差出人の扱い(受信箱に残す・読み物にする・既読で保管・捨てる)を、エージェントが決めてフィルタの PR にすると、本人が項目ごとに
選ぶ場が無く、見過ごされる。同じページの項目(`kind: sender`)にする。保存先の形(`verdicts.schema.json`)は変えない。既存の語
(`decide`・`pass`・`later`・`other`)に写し、**何を選んだか**は `option` で区別する(`fact`・`report` と同じ型)。

| 選択肢の id | `verdict` / `owner` | 意味 |
|---|---|---|
| `keep`(受信箱に残す) | `pass` | フィルタにしない |
| `reading`(読み物にする) | `decide` / `me` | 読み物のフィルタの定義にする(受信箱には出さず、読み物のラベルで取っておく) |
| `archive`(既読で保管) | `decide` / `me` | 既読にして保管するフィルタの定義にする |
| `trash`(捨てる) | `decide` / `me` | 捨てるフィルタの定義にする |
| `later`(まだ) | `later` | 書かない。答えが要るあいだ、次の机でもう一度出す |
| `other`(その他) | `other` | `note` の指示を机で読む |

- この kind の `decide`・`pass` は、やる・やらないではなく、答えである(`options.json` の `answer: true`)。読む側(別リポジトリの机の手順)は、
  `option` が `reading`・`archive`・`trash` の文書を `worklog` の `decide` に写さず、答えを読んで、フィルタの定義の変更(PR 本文の表)を作る。
  `option` を見ずに `verdict` だけで写す読む側は、フィルタの答えを裁定済みと読み違える。読む側を同じ時期に直す(決まり (a))。
- ページの「推奨をすべて採用」は、まだ選ばれていない項目のうち、推奨(`rec_option`)のある項目だけを、推奨の選択肢にする。すでに選んだ項目と、推奨の無い項目、
  推奨が「その他」の項目は、そのままで、例外だけを、あとから個別に直せる。
- 入力の `excluded`(任意)は、規則ありと判定して一覧から外した候補を、根拠(どの規則か)つきで、ページの末尾に畳んで出す。本人が、外した判断に誤りが無いかを確かめられる。
  項目ではないので、保存先には何も書かない。
- 形式を 5 から 6 に上げた(`items.schema.json` の `const`、`fixtures/` の入力、公開する Artifact の description の `形式 v6`、
  `crates/issue-inbox/src/collect.rs` の `FORMAT_VERSION`)。

### 返信の下書きは、検査済みの最終稿だけ(形式 7、#1577)

Gmail の外の媒体(LinkedIn・LAPRAS・Findy)の返信は、下書きをこのページに載せ、本人が元の画面に貼る。hook で止められる口が
無いので、**ページの生成で止める**。`drafts[]` の各下書きに、次の 2 つが必須になった。

- `thread`: 返信先の会話の id(`<媒体>:<会話の印>`)。`reply-coverage check` に渡したものと同じ。`build.sh` が、下書きごとに
  `reply-coverage verify --thread <thread> --draft <text>` を呼び、会話の全文を読んだうえで、相手の依頼すべてへの応答を検査した
  最終稿であることを確かめる。検査していない下書き、検査のあとに直した下書き、保存した全文が 24 時間より古い下書きは、
  ページを作らない(`docs/agents/reply-coverage.md`)。`reply-coverage` が無い機体は、`drafts[]` のあるページを作れない。
  実行ファイルは環境変数 `REPLY_COVERAGE_BIN` で差し替えられる(`--selftest` が偽物を使う)。
- `asks`: 検査の報告にある、相手の依頼と処分(`answered`・`declined`・`deferred`・`answered_before`・`retracted`)の一覧。
  本人が、答え漏れが無いかを画面で確かめる(依頼ごとの引用と処分が、下書きの上に出る)。依頼が無い会話は空の配列。未回答は
  検査に合格しないので、値に無い。

本人がページ上で本文を直したときは、本人が見て決めたものとして扱い、再検査しない。保存先の形(`verdicts.schema.json`)は変えない
(決まり (a) の、読む側の契約の更新は要らない)。形式を 6 から 7 に上げた(`items.schema.json` の `const`、`fixtures/` の入力、
`crates/issue-inbox/src/collect.rs` の `FORMAT_VERSION`。公開する Artifact の `label` は、次の公開で `形式 v7` にする)。

### 紙と同じ順・同じ符号で打ち返す入力、種類の追加、GitHub の鍵、意味層(形式 8)

紙(机の紙の生成器が組む)とこのページを、同じ項目の表から組み、本人が紙で考えたあと、紙と同じ順・同じ符号で 1 項目 1 キーで打ち返せるようにする。

- **キー入力。** 符号は、`options.json` の並びから機械的に振る: `later` 以外の選択肢に、並びの順で `a` から。`later` は Space。画面の各ボタンに符号が出る。
  符号の文字で選ぶと、次の項目へ進む。Space は「あとで」、Backspace は一つ戻る。「その他」の符号は、メモ欄へ入力を移す(メモ欄の中では Space・Backspace を
  移動に使わず、Esc でメモ欄を出て次の項目へ)。Ctrl+Enter で確定する(1 回だけ。`その他` で指示が空の項目が残るあいだは確定させず、件数を出す)。
  カーソルのある項目の題と選んだ語は、画面の上に大きく出る。保存は、選んだ時に項目ごとに済む(確定は保存先に何も書かない)。
- **回の番号の照合。** 入力の `run_no`(任意)を、紙と画面の両方に持つ。画面は、本人が打った番号と一致するまで、キー入力を受けない(ボタンは押せる)。保存先の欄は足さない。
  `items[].stale`(任意)が真の項目だけに、「紙の版と違います」の印が付く(束ごとは刷り直さない)。`items[].rev` は紙の版の印。
- **種類の追加。** `pr`・`gh-notice`・`shelf`・`exam`・`property`。どの種類も `later` と `other` をちょうど 1 つ持つ(`build.sh --selftest` の既存の検査)。
  `property` は、形式モデルの性質の確認(承認・直す・外す)で、`answer: true`。
- **GitHub の鍵。** 媒体 `gh` を足した。鍵は `gh:<スレッド>/<受信>`、スレッドは `owner/repo` の可逆な符号で、`=` を escape にして `/` は `=2f`、`.` は `=2e`、`=` は `=3d`
  (`gh-key.jq`。往復と、文書 id の 1 対 1 を `fixtures/gh-keys.json` と `--selftest` が検査する)。受信は番号。かつての `none:<owner>-<repo>-<番号>/issue` は、`.` を含む repo 名を
  表せず、`a-b/c` と `a/b-c` が同じ形になって 1 対 1 でなかった。`verdicts.schema.json` の鍵の形が変わったので、決まり (a) のとおり、側ごとの読む側の pin の更新が要る。
- **意味層。** `options.json` の各選択肢が、GTD の行き先 `gtd`(`do`・`delegate`・`defer`・`incubate`・`reference`・`trash`・`answer` の 7 語)を持ち、各 kind が
  `gtd_required` の行き先にすべて到達できる(`--selftest` が検査する)。`gtd` は保存先に書かず、`option` の id から導く(保存先の欄は足さない)。
- **原文の全文は載せない。** 入力の `items[].source`(出典 `ref`・原本の `sha256`・任意の `text`)のうち、`text`(原文の全文)は、紙と機体ローカルの PDF だけに載る。`build.sh` が
  DATA を作る前に `text` を落とし、HTML には載らない(`--selftest` が、目印の文字列で確かめる)。
- 形式を 7 から 8 に上げた(`items.schema.json` の `const`、`fixtures/` の入力、`crates/issue-inbox/src/collect.rs` の `FORMAT_VERSION`。公開する Artifact の `label` は、
  次の公開で `形式 v8` にする)。

## ページの並べ方

- 項目は `items[].kind` ごとに 1 つの枠(pane)にまとまる。枠の順と見出しは `options.json` の `kinds` の順と `pane`。
- 下書き(`drafts[]`)は、`drafts[].key` が項目の `key` に一致すれば、その項目の枠の中の、ボタンの上に出る。下書きを読んでから裁定を押すとき、画面を行き来させないため。どの項目にも一致しない下書きだけが、下の節に残る。
- 長い根拠(160 文字超)は、1 行の要約に畳む。推奨とボタンが、根拠の下に押し出されないようにするため。

## 変えるときの決まり

(a) **保存先の形(`verdicts.schema.json`)を変えるときは、読む側を同じ変更で直す。** 読む側は朝の机の手順
(この文書では「読む側の契約」と呼ぶ)で、dotfiles の外にある。読む側の契約は別リポジトリが持ち、保存先
Schema の sha256 を pin して、食い違いを検査する。Schema を変えたら sha256 が変わるので、pin の更新も
同じ変更に含める。`sha256sum verdicts.schema.json` で取る。

**読む側の契約は、側ごとの宣言である**(ADR-1573、#1575)。個人側と会社側は、同じレイアウトの person-state リポジトリを
1 つずつ持ち、それぞれが机のハブ(`desk-hub` が解決する)の `state/reviews/daily/mail-verdict-page.md` に、自分の契約を
置く。契約が持つのは、次の 3 つ。

- 読む側の手順: 保存先の文書(`verdicts`)を、`worklog` の出来事と本人担当の作業へどう写すか(上の「決め方のモデル」の表を、
  その側の手順に写したもの)。
- 起票先: 本人担当の作業(`kind: task` の `do-me` と、`report`)を起票する Issue のリポジトリ。その側のハブのリポジトリで、
  この文書とテンプレートには書かない(側によって違う値を、公開側に固定しない)。
- 保存先 Schema の sha256 の pin: 食い違いを検査するための、その側が読んだ版の値。

したがって、**Schema を変える変更は、側の数だけ、読む側を直す**: 個人側の契約と、会社側の契約(あれば)の両方で、pin を新しい
値に更新する。片方だけ更新すると、更新していない側の机が、食い違いの検査で止まる(止まるのは安全側の失敗で、保存先の文書を
読み違えることはない)。会社側にまだ裁定ページと契約が無い間は、個人側だけを直す(会社側には、直す対象が無い)。この文書・
`items.schema.json`・`verdicts.schema.json`・`options.json`・`template.html` は、側によらず同じ 1 組を使い、側ごとの値を持たない。

未実装(意図あり): 会社側の連絡の媒体 Slack を、保存先の鍵(`verdicts.schema.json` と `items.schema.json` の `<媒体>:…` の形)に足す
こと。`worklog mail` の媒体の語彙には足した(`slack`。`docs/agents/mail-lifecycle.md`)が、鍵の形は、保存先の形の変更で、
上の「側の数だけ読む側を直す」に当たる。会社側の裁定ページと契約を作るときに、形式の版を上げて、一緒に足す。

(b) **過去の文書を読めること。** `note` の無い古い文書は、これからも読める。`verdict` が空文字の文書は
裁定として読まない。欄を足すときは任意にし、既存の欄の意味と文字種を変えない。文書 id(`key` の `:` と
`/` を `.` に、`=` を `+` に換えたもの)の導出は変えない。変えると、過去の文書と新しい文書が別物になる。
`=` の換えは 2026-11-19 に足した。db の文書 id は英数字と `_ - . ~ : @ +` だけを許し、`=` を含む GitHub の鍵(`gh-key.jq` の escape)は
それまで保存されなかった(保存された文書は 1 件も無く、過去の文書とは食い違わない)。`--selftest` が、fixture の GitHub の鍵の
docId が許される文字だけで、衝突しないことを確かめる。

(c) **形を変えたら `format_version` を上げる。** 上げる場所は 3 つ: `items.schema.json` の `const`、
`fixtures/` の入力、公開する Artifact の版の記録。版の記録は、description を渡せる公開の手段なら description に `形式 v<版>` と
書く。渡せない手段(`publish_files` と `publish_version` は description の欄を持たない。2026-11-11 に、形式 5 のまま description が残った)は、
公開時の `label` に `形式 v<版>` と書く。生成されたページには、入力の `format_version` が
`<meta name="verdict-page-format">` として入るので、これが版の正本で、description か `label` はその写しである。
公開済みのページがどの形式かは、`<meta>` から、あとで読める。

(d) **再公開の手順。** 一括処理のたびに、同じ URL へ再公開する(`Artifact` の publish に、前回の `url` と
生成した HTML のパスを渡す)。**`capabilities` は省略する。** 省略した再公開は、保存済みの宣言を変えずに引き
継ぎ、`db` の中身(過去の裁定)も再公開をまたいで残る。宣言を渡し直すと、非空のオブジェクトは「完全な宣言」
(書かなかった機能は取り消される)、`{}` は「全部の取り消し」として扱われるので、渡さない。`contract` も省略
する(省略すると現在の版が保たれ、上げるのは `contract: "latest"` を明示したときだけ)。最初の 1 回だけ
`capabilities: {db: {}}` を渡す。根拠: 実行時の機能の宣言を説明する `artifact-capabilities` スキル(runtime
contract 0.2.92、取得 2026-10-30)の冒頭「omitting `capabilities` on a redeploy carries the stored
declaration forward unchanged」と、`db` の節「keeps data outside the page」。再公開のあと、`ArtifactData`
の `list` でコレクション `verdicts` を読み、過去の文書が残っていることを確かめる。

(e) **明暗 2 テーマと幅 400px の目視は、手元で Playwright から行う(CI の対象外。ブラウザが要るため)。**
`--selftest` は構文とデータの往復までを見る。見た目は次で見る。

1. `bash build.sh --data fixtures/valid.items.json --out <絶対パス>/page.html` で組む。
2. Playwright MCP の `browser_navigate` で `file://<絶対パス>/page.html` を開く。
3. `browser_resize` で幅 400、高さ 900 にする。
4. `browser_emulate_media` で `colorScheme` を `light`、続いて `dark` にして、それぞれ `browser_take_screenshot` を撮る。
5. `browser_evaluate` で `document.documentElement.scrollWidth <= window.innerWidth` が `true`(横スクロールが無い)
   であることを確かめる。
6. 確かめるのは、文字がテーマの背景の上で読めること、ボタンと入力欄が幅 400px に収まること、`至急`・`早めに`
   の項目の色が付くこと。`file://` では `window.claude` が無いので、「保存先に接続できません」と出るのが正しい。

## 検査の範囲

`--selftest` は、スキーマに適合する入力が通ること、`key` の不正・`id` と `key` の重複・必須の欠落・余計な欄・
版の違いが落ちること、`</script>` や `<!--` を含む文字列でも生成物の `<script>` が JavaScript として構文を
通り `DATA` が入力に戻ること、テンプレートが `db` に書く文書の欄が `verdicts.schema.json` と一致すること、
`docId` の換え方を確かめる。ブラウザで動かして `db` に書く部分(クリックして保存される、など)は、この
検査の外で、Artifact を公開したあとの確認になる。
