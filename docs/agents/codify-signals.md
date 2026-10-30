# as-code 化の候補を振り返りで記録する(段・兆候・既存手段)

作業の終わりの振り返り(retro。[retro](retro.md))は、「この手順は決定論化(as-code 化)できないか」を、
次の 3 つの閉じた語で記録する。何をどの手段で決定論化するかの嗅覚は、この語彙を実績で直して磨く。
語彙の正本は [`config/agents/codify-signals/vocabulary.json`](../../config/agents/codify-signals/vocabulary.json)
1 つで、retro の手順の文・行の検証・下の表・日次の較正は、すべてそこから出る(複写しない)。
決定の理由と代替案は ADR-0000(仮番号。PR で改番する)。

## 3 つの語

| 欄 | 何を答えるか | 値 |
|----|----|----|
| `mechanism` | 次から防ぐのに、実際に効く最も上位の手段 | 5 段(下の表)/ `existing:<名前>`(すでにある仕組みが漏らした)/ `none:<理由>`(決定論化しない) |
| `signal` | なぜ決定論化できそうだと思ったか(兆候) | 下の表の id。段を持つ行で必須 |
| `existing` | 作る前に、採用・拡張できる既存手段は無いか | `採用:<名前>` / `拡張:<名前>` / `自前`。`prose` 以外の段で必須 |

加えて次の欄がある。

- `searched`: 探した範囲(カンマ区切り)。リポジトリ内(`pipeline-map` / `crates` / `skills`)は必ず探す。外部(`nixpkgs` /
  `upstream:<製品>`)は `existing` が `自前` のとき 1 回探して書く。
- `level_reason`: 兆候の既定の段と違う段を選んだときの理由。既定は「ここまで上を目指せ」という値なので、届かなかった
  行は理由を書く。理由が溜まると、既定の段を直す根拠になる。

`prose`(文章)の行には作る物が無いので、`existing` と `searched` は書かない。上位の段を選ばなかった理由は
`level_reason` が残す。旧い `script` は `tool` と読み替える(過去の台帳は書き換えない)。

## 決定論化しないもの

意味の判断が要るもの、機械から観測できないもの、好みの問題、一度きりの探索は、決定論化しない
(`mechanism: none:<理由>`)。「例外ゼロの規則だけを hook にし、好みの hook は消す」という考え方と同じ線引き
(Anthropic, "Best practices for Claude Code", https://code.claude.com/docs/en/best-practices、取得 2026-10-30)。
過去の振り返り 244 行の試し当てでは、26% がこれに当たった。

## 事例集(語彙から生成)

<!-- BEGIN GENERATED: session-retrospective --retro-casebook -->
### 段(上位ほど優先)

| 段 | 意味 |
|---|---|
| `eliminate` | 手順や共有の状態そのものを無くす。誤りが起きる場が存在しなくなる |
| `unrepresentable` | 型・閉じた語彙・単一の宣言・状態機械で、誤りを書けなくする |
| `gate` | 実行前の拒否・Stop での差し戻し・CI で、誤りを止める |
| `tool` | 正しいやり方を 1 本の CLI・スクリプトにする |
| `prose` | skill・AGENTS.md の文章。最後の手段 |

### 兆候

| 兆候 | 既定の段 | こういうことがあった | 事例 |
|---|---|---|---|
| `rebuilt-rule` | `tool` | 決められる規則を、毎回その場の会話で組み立て直した | — |
| `verbatim-repeat` | `tool` | 同じコマンド・手順を、そのまま繰り返し実行した | ADR-543 |
| `adhoc-transform` | `tool` | その場でデータの変換・集計のスクリプト(jq・awk・python)を書いた | — |
| `repeated-violation` | `gate` | 同じ規範違反が、文章で書いてあるのに繰り返された | ADR-396、ADR-598 |
| `form-check` | `gate` | 書式・必須要素を目で確かめた。機械で検査できる形だった | ADR-0031 |
| `irreversible` | `gate` | 取り消せない・外向きの操作の手前で迷った、または踏み外した | ADR-705、ADR-544 |
| `copy-drift` | `unrepresentable` | 同じ事実の写しと実体がずれた。手で転記した。上流がこのリポジトリの外にあるものは、宣言では防げないので gate(検査)まで | ADR-0033、ADR-503、ADR-872 |
| `hidden-state` | `unrepresentable` | 状態を頭・ブラウザ・会話の中だけで持ち、記録に戻らなかった | ADR-1259 |
| `race-or-crash` | `unrepresentable`(形式仕様も添える) | 並行・中断で壊れうる手順。順序や途中失敗で状態が食い違う | ADR-1242、ADR-1259 |
| `shared-counter` | `eliminate` | 共有の採番・資源を並行する担い手が取り合った | ADR-380 |
| `sandbox-env-gap` | `unrepresentable` | sandbox の宣言・除外・許可の不足で、書き込みや署名や認証が失敗し、外での再実行や往復が要った | #1164 |
| `guard-misfire` | `gate` | gate や道具が誤検知した、過剰に再通知した、拒否文が行き止まりだった。直す先は既存の gate(拡張) | #898 |
| `local-ci-gap` | `gate` | CI でしか分からない失敗が、手元の検査では出なかった | #1092 |
| `worktree-bootstrap` | `tool` | 新しい worktree に、依存・認証・検証の前提が無かった。作成直後の準備を毎回手で整えた | — |
| `unclassified` | `prose` | 上のどれにも入らないが、決定論化できそうだと思った。上位の段を選ぶなら level_reason が必須。数が溜まると語彙に足す候補になる | — |

### 却下の理由(`/backlog-cleanup` で却下するとき)

- `semantic-judgment`
- `unobservable`
- `preference`
- `one-off`
- `existing-suffices`
- `misread-signal`
<!-- END GENERATED -->

表を直すときは、語彙 JSON を直して `session-retrospective --retro-casebook` の出力を生成部に貼る
(`crates/session-retrospective/tests/casebook.rs` が、文書と語彙のずれを落とす)。事例には、このリポジトリの
ADR・PR の番号だけを載せる(ADR-0034。`crates/issue-inbox/tests/vocab.rs` が形式と実在を検査する)。

## 限界

- 段の付け方は LLM の判断で、機械は「語彙の内か」「必須欄が揃っているか」しか見ない。当たっているかは、
  `/backlog-cleanup` での採用・却下の実績で測る(却下には閉じた理由を書く)。
- 兆候の `since` より前のノートは、その兆候の較正に数えない(定義を直したら数え直す)。
- user-correction の行は Inbox を通らず Issue になるので、較正の集計には入らない。
