# ADR-0000 — 相手のいる会話への返信は、会話の全文を読み、相手の未決の依頼すべてに答えた証拠を残してから作る

- Status: Accepted
- Date: 2026-11-15
- Issue: #1577(段1〜段4 の stacked PR で実装する)

## Context

紹介会社の担当者から LinkedIn で声をかけられ、返信を下書きしたとき、相手が最初のメッセージで出していた電話の日程候補に触れず、質問にだけ答えた。相手は「ヒアリングのあとに求人を紹介する内容だった」と訂正し、話の流れが変わった。原因は、通知メールの最新の 1 通や一覧の抜粋だけで下書きを組む経路があり、相手がこれまでに依頼・提示したことすべてに答えたかを確かめる手順も検査も無かったことである。

本人の指示(2026-11-15)で、範囲を「LinkedIn の下書き」から広げた。特定の道具や特定の作業に限らず、考え方を転換する。**相手のいる会話に返す文は、最新の 1 通への応答ではなく、会話の始めからの、相手の未決の依頼・提示すべてへの応答として書く。** 本人の言葉は次のとおり: 質問に答えるのはよいが、ずれた返事をしてしまったことを軽く謝るくらいはしてもよい。出口は、会話のコンテキストをきちんと貯めること(全文を保存してもよい)。

調べて分かったこと(取得 2026-11-15、このリポジトリの読み取り):

- 答え漏れを検査する仕組みは、どの媒体にも無い。近い規則は、宛先は台帳でなくスレッドを読むこと(共有 AGENTS.md)と、状態をスレッド全体で確かめること(inbox-adjudication の SKILL)だけである。
- `external-send-guard` は下書きを素通しにして本文を読まない。`ja-polish-gate` は日本語の段落が記録済みかだけを見る。ブラウザ経由の入力(`browser_type`)はどの関所も見ない。
- `resolve-pr-threads` は `comments(first: 1)` で、スレッドの 2 通目以降を読めない。
- `claude -p --bare` は購読(OAuth)の認証を読まず、`Not logged in` で失敗する。設定・skill・MCP を切らずに呼ぶと、自明な 1 回でも `total_cost_usd` が約 0.106 になり、`--setting-sources project`・`--disable-slash-commands`・`--strict-mcp-config`・空の作業ディレクトリ・`--system-prompt` で約 0.00024 になる(2026-11-15 に実測)。

## Decision

### D1: 原則は共有の AGENTS.md に置き、媒体ごとの手順書と hook は、その原則を実装する搬送路にする

原則: 会話の全文を読む → 相手の未決の依頼を逐語で列挙する → 各項目に「答える / 断る / 保留(理由と時期)/ 依頼でない」を付ける → 書く。本人とのチャットへの返答は `requirements-inventory` が担うので、対象に含めない。

### D2: 会話の全文は、hook が MCP の応答から機械的に保存する

エージェントに書き写させると、書き写しの脱落が今回の失敗そのものを再生産する。Gmail の `get_thread` の応答を PostToolUse の hook が保存する。ブラウザの媒体は、`browser_snapshot` の `filename` でツール自身にファイルへ書かせ、`reply-coverage import` で取り込む。

### D3: 新しい crate `reply-coverage` が、検出(`check`)・照会(`verify`)・計測(`bench`)を持つ

`ja-polish` の関所と記録の型は流用するが、責務が違う(段落を書き直したかと、依頼の一覧の全項目に処分があるか)ので、同居させない。

### D4: 検出は、文脈を切った新しい Claude(`claude -p`)に任せ、引用の逐語照合で検証する

会話の全文と下書きを渡し、相手の依頼を逐語の引用つきで列挙させる。Rust 側で、依頼の引用が会話の全文に、「答えた」の根拠が下書きに逐語であることを照合する。存在しない依頼は捨て、根拠の無い「答えた」は未回答に戻す。道具を渡さない(`--tools ""`)ので、会話に紛れた指示に乗っ取られても、読み書きも実行もできない。

### D5: 下書きの作成口で、検査済みでない返信を拒否する(段2)

PreToolUse の規則が、本文の sha・会話の同一性・保存済みの全文の新しさ(24 時間)を、記録だけで判定する。LLM は呼ばない。

### D6: hook で止められない媒体は、裁定ページの生成で止める(段3)

`drafts[]` に検査の記録を必須にする。代行送信の入力そのもの(`browser_type`)は、どの hook も見ないので、止められない。送信直前の確認(#1261)に、依頼の一覧と処分の表を出して、本人が見る。

### D7: 会話の全文は、機体ローカルの状態に置き、有効期限(60 日)で掃く

`~/.local/state/claude/reply-coverage/`(Git の外・vault の外。権限は 0700 / 0600)。vault は LiveSync でスマホに同期されるので、第三者のメッセージの全文を置かない(ADR-1256 D4)。正本は媒体のままで、保存は複製にすぎない。別の機体では、媒体から取り直す。

### D8: テストフィクスチャは 2 層にする

架空の見本(公開リポジトリ)で、自動テストと計測の回帰を回す。本物の見本は、非公開の person-state リポジトリに置き、`reply-coverage bench --cases <dir>` で手で回す計測にだけ使う。公開側のコード・文書・出力に、本物の本文と相手の名前を出さない(表に出すのは、見本の id と判定だけ)。

### D9: 前の返信が落としていた依頼に今回初めて答えるときは、軽い詫びを求める

検出の出力の `late`(こちらの先の送信より前に出ていた依頼か)で、`late` の依頼に今回答える下書きに、詫びの文(下書きに逐語である根拠つき)が無ければ、不合格にする。

### D10: `resolve-pr-threads` は、スレッドの全コメントを読む(段3)

## Alternatives considered

- 検出に Gemini(`ja-polish` と同じ経路)を使う: 無料枠のキーでは、送った文面が Google の製品改善に使われうるので、第三者のメッセージを新しく Google へ送ることになる。`claude -p` の送り先は、今のセッションと同じ Anthropic だけで、データの流れが増えない。
- エージェント自身が作る依頼の一覧の形式検査だけ: LLM を呼ばないが、抜けた当人が一覧を作るので、今回の型の抜けが一覧の段で再発しうる。
- `--bare` で起動する: 購読の認証を読まないので、動かない(実測)。
- 会話の全文を vault に置く: LiveSync でスマホに同期され、第三者の個人情報の複製が増える。
- 全文の保存を、エージェントが書き写す: 脱落が、検査したい失敗そのものを生む。

## Consequences

- 返信の下書きは、検査(約 5 秒、検出 1 回)を 1 回挟む。検出の失敗(通信・認証)は「合格」にならない。止めるときの逃げ道は、段2 で escape の登録に足す。
- 検出が相手の依頼を挙げ損ねる(見落とし)は、照合では防げない。精度は `bench` で測り、実物の見本の計測の結果で既定のモデルを決める(段4)。残りは、裁定ページで本人が見る。
- 媒体のうち、カレンダー招待への返答・日程調整の返答・esa・Claude Docs のコメントは、機械で止める口が無く、原則と手順書だけで担う(検出のみ)。

## 執行点

- `crates/reply-coverage/src/detect.rs`
- `crates/reply-coverage/src/verify.rs`
- `crates/reply-coverage/src/backend.rs`
- `crates/reply-coverage/src/store.rs`
- `crates/reply-coverage/tests/fixtures.rs`
- `crates/reply-coverage/tests/cli.rs`
- `pipeline-map.toml`

## 先行例との対比

- D1: 先行例: dotfiles の `config/agents/skills/requirements-inventory/SKILL.md` §1・§2(依頼を逐語で列挙し、全項目に処分を付ける。取得 2026-11-15)。差分: 異なる — 先行例は本人から計画への依頼の落とし防止。ここは相手から本人への依頼の落とし防止。軸: 検出のみ。
- D2: 先行例: dotfiles の `crates/hook-router/src/rules.rs`(PostToolUse の記録役)、Anthropic, "Hooks reference", https://code.claude.com/docs/en/hooks(PostToolUse の入力に `tool_response` を含み、MCP ツールにも発火する。取得 2026-11-15)。差分: 異なる — 先行例は Bash の実行記録。ここは MCP の応答の本文。軸: 表現不可能。
- D3: 先行例: dotfiles の `docs/agents/ja-polish.md`(記録・関所・bench。取得 2026-11-15)。差分: 異なる — 責務が違うので同居させない。軸: 表現不可能。
- D4: 先行例: dotfiles の `config/agents/skills/precedent-grounding/SKILL.md` §4(文脈を切った critic。取得 2026-11-15)、`docs/operations.md`(`claude -p` を無人で呼ぶ study-intake)。差分: 異なる — 監査の対象が下書きで、戻りの照合が引用の逐語一致。軸: 検出のみ。
- D5: 先行例: dotfiles の `docs/agents/ja-polish.md`「投稿の関所」(取得 2026-11-15)。差分: 異なる — 下書き全体の sha と会話の同一性まで見る。軸: 表現不可能。
- D6: 先行例: dotfiles の `config/agents/skills/inbox-adjudication/reference/verdict-page/build.sh`(入力の検証が不適合なら生成しない。取得 2026-11-15)。差分: 異なる — 別の記録の有無を引く。軸: 表現不可能。
- D7: 先行例: dotfiles の `docs/adr/1256-worklog-mail-kind.md` D4、`docs/agents/record-layers.md`(L4 は Git の外・vault の外。取得 2026-11-15)。差分: 異なる — 本人の指示で全文を残すが、置き場を vault の外にする。軸: 表現不可能。
- D8: 先行例: dotfiles の `docs/agents/ja-polish.md`「測定」、`config/agents/skills/test-grounding/SKILL.md` §5(取得 2026-11-15)。差分: 異なる — 見本の置き場が別の非公開リポジトリで、負例を必ず持つ。軸: 表現不可能。
- D9: 先行例なし: dotfiles の `docs/adr/`・`docs/agents/`・`config/agents/skills/*/SKILL.md` を「詫び」「抜け」「答え漏れ」で検索(取得 2026-11-15)。軸: 検出のみ。
- D10: 先行例: dotfiles の `config/claude/commands/resolve-pr-threads.md`(`comments(first: 1)`。取得 2026-11-15)。差分: 異なる — 取得件数と表示を改める。軸: 表現不可能。
