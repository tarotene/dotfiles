# ja-polish — 日本語の文書を Gemini に書き直させ、通した証拠を残す

設計の判断: `docs/adr/1387-ja-polish.md`。実装: `crates/ja-polish`。有効化: `home/modules/ja-polish.nix`
(`dotfiles.jaPolish.enable`、個人の identity だけ)。

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
```

投稿の本文・下書き・コミットメッセージは、本文を scratchpad のファイルに書き、`ja-polish --stdin`
に通した出力を `--body-file` などに渡す。**出力の差分は必ず読む**。意味・数値・主張の強さがずれて
いたら、出力を手で直さず、下書きを直して通し直す(記録は段落の中身で照合するため、出力を手で
直すと「通していない段落」に戻る)。

`ja-polish` は `llm` を呼び、キーと Google への通信に届く必要があるので、Claude の sandbox の外で
動かす(`home/modules/claude.nix` の `excludedCommands` に `ja-polish *`)。Codex の sandbox では
通信できず、段落は「素通し」になる(下の「失敗したとき」)。Codex 側の対応は未実装(意図あり)。

## 何が起きるか

1. 文書を段落(空行で区切られた塊)に分ける。フェンス(```)と front matter は 1 つの段落として、
   書き直しの対象から外す。
2. 対象の段落は、次をすべて満たすもの: 伏せ字を除いて日本語の字(かな・漢字)が 4 字以上ある、
   記録に無い、基点に同じ段落が無い(追加・変更した段落である)。
   - 基点: git の作業ツリーの中なら、origin の既定ブランチとの merge-base にある同じパスの内容。
     git の外(vault)・新規のファイル・origin が無いときは基点なしで、文書全体が対象。
     一度通した段落は記録で分かるので、基点が無くても通し直しで書き換えは起きない。
3. 記法を**伏せ字**(`⟦P0⟧`)に置き換える: 見出しの行、`Closes #N` などの参照の行、末尾の
   `🤖 Generated with …` の行、`No-Issue:` などのタグの接頭辞、インラインコード、HTML タグ、
   リンクの宛先、URL、Issue/PR の参照。見出しを丸ごと伏せるのは、PR 本文の `## 課題` や
   `## 要確認` のように、別の検査が見出しの語で節を探すものがあるため。
4. `writing-style-hub` が指す `docs/style/writing.md`(private の文体規約)をその場で読み、指示文に
   載せる。写しは持たない。ハブが解決できないときは、基準なしでは送らず、段落を素通しにする。
5. 対象の段落と、その前後の段落(参考。書き換えない)を、`llm prompt -m <model> -s <指示文>
   --no-stream -n` に標準入力で渡す。`-n` で llm の履歴 DB に残さない。
6. 戻ってきた段落を照合する。外れたら、その段落だけ 1 回再依頼する:
   - 伏せ字がすべて同数で残っている
   - 数値の集合(全角は半角にそろえる)が同じ
   - 行頭の記法(見出し・箇条書き・番号付き・引用・表)の並びとインデント幅が同じ
   - 分量が元の 4 割以上 2 倍以下
   - 依頼の書式(`<para>` など)が混ざっていない
7. 通った段落は伏せ字を戻して差し替え、`records.jsonl` に書く。

## 記録

`~/.local/state/claude/ja-polish/records.jsonl`(`$XDG_STATE_HOME` があればその下)。1 行 1 段落:

```json
{"ts_unix":1793000000,"sha":"<段落の正規化した本文の SHA-256 の先頭 32 桁>","state":"polished","model":"gemini-3.8-flash"}
```

- `sha`: 段落の**最終の**本文(空白をそろえてから)。書き直した段落は書き直した後の本文、素通しの段落は元の本文。
- `state`: `polished`(書き直した、または直す必要が無いと確かめた)か `skipped`(素通し。`reason` つき)。
- 関所(投稿の PreToolUse、文書ファイルの Stop。後続の段で入る)は state を区別せず、`sha` があれば通したとみなす。
  素通しで止めないための設計で、素通しは次の gate イベントが知らせる。

## 失敗したとき

止めない(fail-open)。失敗した段落は**元の本文のまま**残り、`skipped(<理由>)` で記録される。理由の例:
`llm が見つかりません`、`llm が失敗しました: Error: No key found …`、`120 秒以内に応答がありません`、
`スタイルガイドのハブを解決できません`、伏せ字・数値・記法・分量の照合に再依頼でも外れた。

`~/.local/state/claude/gate-events.jsonl` に、書き直した回ごとに `ja-polish` の `fail-open`(1 段落でも
素通しがあった)か `pass` を残す。`gate-failopen-advisory` が、`pass` を挟まずに 3 回続けて `fail-open` の
とき、次のセッションの冒頭で警告する(`GATE_FAILOPEN_THRESHOLD` で変えられる)。

## 設定とキー

- 設定ファイル: `~/.config/dotfiles/ja-polish.toml`(home-manager が置く)。`model`(既定
  `gemini-3.8-flash`)と `timeout_secs`(既定 120)。**このファイルの有無が、その機体で有効かどうかの
  唯一の印**。無ければ関所は何も検査しない。会社の機体には置かれない。環境変数 `JA_POLISH_MODEL` で
  その場だけ上書きできる。
- API キー: 各機体で一度 `llm keys set gemini` を手で実行する。キーは `llm keys path` が指す
  `keys.json`(Linux は `~/.config/io.datasette.llm/`、macOS は `~/Library/Application Support/
  io.datasette.llm/`)に入る。そのディレクトリは `agentPermissions.secret_paths` に入っていて、Claude の
  sandbox の中のコマンドと Read ツールから読めない。
- キーは Google AI Studio で発行する。**無料枠は、送った文面が Google の製品の改善に使われ、人が読み
  うる契約**(Google, Gemini API Additional Terms of Service、更新 2026-03-23、
  https://aistudio.google.com/docs/terms 、取得 2026-11-08)。そのため個人の機体だけで有効にし、
  勤務先の文書は個人の機体でも通さない。

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

1. Google AI Studio で API キーを作る。URL・画面名・ボタン名は **未確認**(作業時に画面を読んで
   確かめ、その場でこの手順を直す)。
2. `llm keys set gemini` を実行し、プロンプトに貼る。
3. `echo '日本語を直すことができる。' | ja-polish --stdin` で、書き直した文が返り、`ja-polish --check`
   が「未通過の日本語の段落はありません」と答えることを確かめる。

## 後続(意図は決まっていて、未着手)

- 会社の機体(arcturus): 会社の Gemini 契約(Vertex AI や Workspace など)の接続方法を本人に確かめ、
  `dotfiles.jaPolish` に送り先を足す。無料枠のキーは配らない。
- Codex の sandbox の外で `ja-polish` を動かす。

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
