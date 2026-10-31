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

## 決め方のモデル(形式 4)

画面の部品を、意思決定とその受け渡しの語彙から導く。項目ごとの裁定は、次の 2 つの軸で決まる。

- **どうするか(`verdict`)**: `decide`=やる、`pass`=やらない、`later`=あとで、`other`=その他。`other` は、どの選択肢にも当てはまらないときで、**記入欄(`note`)がそのまま本人の指示になる**。
- **誰がやるか(`owner`)**: `decide` のときだけ。`me`=本人、`agent`=おまかせ。

項目の種類(`kind`: `reply`・`notice`・`task`)ごとに、この 2 軸の組を、その種類で意味の通る語のボタンにする(`options.json`)。たとえば同じ `pass` でも、返信なら「返信しない」、通知の確認なら「心当たりあり(対応不要)」になる。同じ語が、種類によって別の意味に読まれないようにするため。

- 各ボタンには、押した後の分担(「あなた」「私」)が付き、項目の枠の中と、ページ上部の「この選択のあとの分担」に出る。移譲(おまかせ)の中身と、本人に残る操作(送信前の確認など)を、決める前に見えるようにする。
- 推奨は `items[].rec_option`(選択肢の id)で指す。推奨のボタンに印が付く。選ぶのは本人で、推奨を既定にはしない。
- `other` は、記入欄が空のあいだ「指示待ち」で、読む側は裁定として読まない。`later` は何も記録しない(7 日で再通知)。
- 「選択を外す」は、`verdict` を空文字に戻す(読む側は裁定として読まない)。

読む側(机の手順 2b-0)は、保存先の文書を `worklog` の出来事に次のように写す(語の意味の正本は `options.json`)。

| `verdict` / `owner` | 出来事 |
|---|---|
| `decide` / `me` | `decide`。反映は本人が行い、エージェントが読み戻して `apply` で閉じる |
| `decide` / `agent` | `decide`。要旨を「おまかせ:」で始める。机の中で実行し、`apply` で閉じる。外部への送信は、送信前の確認(画面・全文・宛先)を本人から取ってから |
| `pass` | `pass` |
| `later` | 書かない |
| `other` | 書かない。`note` の指示を机で読み、`decide`・`pass`・`move`・`ask` のうち指示に合うものを書く。読めなければ、本人に聞く |

この写しは暫定である。`worklog` の語彙には、担い手(`owner`)と「指示待ち」の状態がまだ無い。語彙側の変更は #1312 で扱う。

## ページの並べ方

- 項目は `items[].kind` ごとに 1 つの枠(pane)にまとまる。枠の順と見出しは `options.json` の `kinds` の順と `pane`。
- 下書き(`drafts[]`)は、`drafts[].key` が項目の `key` に一致すれば、その項目の枠の中の、ボタンの上に出る。下書きを読んでから裁定を押すとき、画面を行き来させないため。どの項目にも一致しない下書きだけが、下の節に残る。
- 長い根拠(160 文字超)は、1 行の要約に畳む。推奨とボタンが、根拠の下に押し出されないようにするため。

## 変えるときの決まり

(a) **保存先の形(`verdicts.schema.json`)を変えるときは、読む側を同じ変更で直す。** 読む側は朝の机の手順
(この文書では「読む側の契約」と呼ぶ)で、dotfiles の外にある。読む側の契約は別リポジトリが持ち、保存先
Schema の sha256 を pin して、食い違いを検査する。Schema を変えたら sha256 が変わるので、pin の更新も
同じ変更に含める。`sha256sum verdicts.schema.json` で取る。

(b) **過去の文書を読めること。** `note` の無い古い文書は、これからも読める。`verdict` が空文字の文書は
裁定として読まない。欄を足すときは任意にし、既存の欄の意味と文字種を変えない。文書 id(`key` の `:` と
`/` を `.` に換えたもの)の導出は変えない。変えると、過去の文書と新しい文書が別物になる。

(c) **形を変えたら `format_version` を上げる。** 上げる場所は 3 つ: `items.schema.json` の `const`、
`fixtures/` の入力、公開する Artifact の description(`形式 v<版>` と書く)。生成されたページには、入力の
`format_version` が `<meta name="verdict-page-format">` として入る。版がページと description に残るので、
公開済みのページがどの形式かを、あとから読める。

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
