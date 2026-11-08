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
```

投稿の本文・下書き・コミットメッセージは、本文を scratchpad のファイルに書き、`ja-polish <そのファイル>`
で書き直し、直したファイルの内容を `--body-file` や `-F` に渡す(または下書きのツールに貼る)。
scratchpad は git の外なので基点が無く、本文の全体が対象になる。`ja-polish` は**単体で呼ぶ**: `<`・`>`
のリダイレクトや `$(…)` を足すと、呼び出し全体が sandbox の中に残り、キーに届かず失敗する
(`sandbox-compound-guard`、#1164)。`--stdin` は人が端末で使う形。**出力の差分は必ず読む**。意味・数値・主張の強さがずれて
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
  環境変数 `JA_POLISH_GATE_ALLOW=1`(ja-polish が壊れたときの逃げ道)、本文が取れないもの(`--body-file` が
  読めない、知らない形の MCP 入力)。判定不能を拒否に変えない。
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
  が素通しとして記録するので、直せない状態でも止まり続けない。逃げ道は `JA_POLISH_GATE_ALLOW=1`。
- **基点のスナップショット**(PreToolUse の規則 `ja-polish-snapshot`): git の外の文書(vault のノートなど)には
  merge-base が無い。`Write` / `Edit` / `MultiEdit` の前に、そのファイルの内容を
  `~/.local/state/claude/ja-polish/base/<パスのハッシュ>.snap` に残し(12 時間以内の新しいものがあれば
  上書きしない)、触れた文書の一覧を `touched/<session_id>.txt` に足す。作業ツリーには何も書かない。
  基点の優先順位は、git の merge-base、スナップショット、なし(文書全体が対象)。セッション ID を使わない
  のは `ja-polish` の CLI が ID を知らないため。基点が古い側に寄っても、対象が増えるだけで、通した段落は
  記録で飛ばすので書き換えは増えない。
- 登録: Stop hook は `home/modules/agents.nix` の宣言(`dotfiles.jaPolish.enable` の機体だけ)。Codex には
  Claude の宣言から導く規則でそのまま登録される。

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
  唯一の印**。無ければ関所は何も検査しない(置いていない機体では、関所も LLM への送信も起きない)。環境変数 `JA_POLISH_MODEL` で
  その場だけ上書きできる。
- API キー: 各機体で一度 `llm keys set gemini` を手で実行する。キーは `llm keys path` が指す
  `keys.json`(Linux は `~/.config/io.datasette.llm/`、macOS は `~/Library/Application Support/
  io.datasette.llm/`)に入る。そのディレクトリは `agentPermissions.secret_paths` に入っていて、Claude の
  sandbox の中のコマンドと Read ツールから読めない。
- キーは Google AI Studio で発行する。**無料枠は、送った文面が Google の製品の改善に使われ、人が読み
  うる契約**(Google, Gemini API Additional Terms of Service、更新 2026-03-23、
  https://aistudio.google.com/docs/terms 、取得 2026-11-08)。個人の側は、それでよいと本人が裁定した。
  この経路(個人の無料キー)で、勤務先の文書は通さない。会社契約の送り先は、別の経路として要件を
  確かめてから足す(「横展開」)。

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
3. 端末で `echo '日本語を直すことができる。' > /tmp/t.md && ja-polish /tmp/t.md && ja-polish --check /tmp/t.md`
   を実行する(人の端末なので連結してよい)。確認する値: 書き直した差分が出て、最後に「未通過の日本語の
   段落はありません」と出る。

## 後続(意図は決まっていて、未着手)

- 会社の機体(arcturus)への横展開(#1385)。下の「横展開」の確認事項を調べるところから。
- Codex の sandbox の外で `ja-polish` を動かす(#1386)。

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
