# calendar-handshake — 個人と勤務先のカレンダーの握手に使う印の語彙

語彙の正本: `config/agents/calendar-handshake/vocabulary.json`
関連: `docs/agents/slot-availability.md`(仮押さえの印 `marker_prefix` を読む側)

## 目的

個人カレンダー上の専用カレンダーに置く予定の題名は、先頭に印(トークン)を 1 つ付ける。
個人側と勤務先側のスクリプト・エージェントは、この印だけで状態を受け渡す。
状態が変わるときは、題名の印を付け替える。印の語彙は閉じており、表にない印は使わない。

語彙はコードに埋め込まず、JSON 1 つを正本にする。両側の実装が同じ表を読むことで、
片側だけが新しい状態を足して握手が食い違う事態を、表現できなくする。

## 状態と遷移

状態は 7 つ。`hold` は、予定がほかの予定との重なりを避ける強さを表す。
`soft` は仮押さえで、ほかの予定を入れてよい。`hard` は確定した拘束で、通常の予定と同じに扱う。

| id | 印 | 置く主体 | hold |
|---|---|---|---|
| `candidate` | `[hold] ` | person | soft |
| `ok` | `[ok] ` | workplace-script | soft |
| `resched` | `[resched] ` | workplace-script | soft |
| `ng` | `[ng] ` | workplace-script | soft |
| `go` | `[go] ` | person | soft |
| `blocked` | `[blocked] ` | workplace-script | hard |
| `secured` | `[secured] ` | workplace-agent | hard |

印は末尾の空白まで含めて 1 つのトークンとする。

遷移は 4 本。表にない遷移は行わない。

| 遷移元 | 遷移先 | 遷移させる主体 |
|---|---|---|
| `candidate` | `ok`・`resched`・`ng` のいずれか | workplace-script |
| `ok`・`resched` | `go` | person |
| `go` | `blocked` | workplace-script |
| `blocked` | `secured` | workplace-agent |

`ng` と `secured` から出る遷移は無い。この 2 つが終端である。

## 版の固定

利用側は、`vocabulary.json` の内容の sha256 を自分の設定に pin し、読み込み時に照合する。
一致しなければ失敗させ、読み替えて続行しない。浮動(最新を読む)にはしない。

語彙を変えるときは、このリポジトリに PR を 1 本出す。
利用側は、その PR のマージ後に、自分の設定の pin を新しい hash へ上げる。
JSON の `version` は人が読む世代番号であり、照合の根拠には使わない。根拠は内容の hash だけである。

## slot-availability との関係

`slot-availability` は、題名が設定の `marker_prefix` で始まる予定を、仮押さえとして読む。
`candidate` の印 `[hold] ` は、この `marker_prefix` の既定の形と同じトークンである。
そのため `candidate` は、追加の設定なしに仮押さえとして読まれる。

`soft` の状態(`ok`・`resched`・`ng`・`go`)も仮押さえとして読ませたい利用者は、
それらの印を設定の `marker_prefix_aliases` に並べる。別名は読み取り専用で、
`slot-availability` が新しく作る印には使われない。

`hard` の状態(`blocked`・`secured`)は、`marker_prefix_aliases` に並べない。
並べなければ、通常の予定として扱われ、空き時間の判定で busy に数えられる。
確定した拘束を仮押さえとして読み飛ばす誤りを、設定の書き方で避ける。

## 同じ組の判定

同じ組の予定は、題名から先頭の印を除いた残りが一致するものとする(`group_key` = `title-after-token`)。
状態が変わっても題名の残りは変わらないので、印を付け替えた予定が同じ組のまま追跡できる。
題名の残りを編集した予定は、別の組になる。

## 実装との関係

このリポジトリが持つのは語彙の JSON と、この文書だけである。
`slot-availability` のコードは変えない。印を読み書きする側の実装は、利用側が持つ。
