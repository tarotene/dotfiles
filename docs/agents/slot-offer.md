# slot-offer — 日程の提示と仮押さえを、1 提案 1 ジャーナルで食い違わせない

決定の記録: Issue #1319(本人の指示 2026-10-31「先方に日程を提示する操作と Google カレンダーを押さえる操作は、そのスロットごとに内部的にアトミックに」)。
手順の文面の元: Issue #1315(仮押さえを先に作り、読み戻してから送る)
語彙の正本: `config/agents/slot-offer/vocabulary.json`(共通の形は `docs/agents/state-vocabulary.md`)
実体: `crates/worklog/src/offer.rs`(状態機械・ジャーナル・回復の計画・コマンド行)、`crates/worklog/src/main.rs`(`worklog offer`)
試験: `crates/worklog/tests/offer.rs`

## 何を解くか

日程の候補を相手に出す返信(LinkedIn・メール)は、提示(送信)と、カレンダーの仮押さえ(`[hold] `)が別のシステムの操作で、
1 つのトランザクションにはできない。手順の文面だけだと、途中で止まったときに次のどちらかが残る。

- 押さえだけが残る(提示は出ていない)。安全側だが、空いている枠を塞ぐ。
- 提示だけが出て、押さえが無い。相手が選んだ枠が、別の予定で埋まりうる。

`worklog offer` は、**意図の記録(ジャーナル)と回復**で、中断のあとに押さえと提示が食い違わない状態へ戻す。
1 提案 1 ファイルのジャーナルに、押さえを読み戻して揃えるまで提示に進めない関門と、中断後の回復の計画を持たせる。

**カレンダーの操作と送信は、スクリプトから行わない。** コネクタ(カレンダー・Gmail・ブラウザ)を呼べるのはエージェントだけで、
gcalcli・Calendar API の呼び出しはこのリポジトリに無い。スクリプトは「次に何をすべきか」の計画(作る押さえ・消す押さえ・
本人に聞くこと)を JSON で出し、エージェントが実行して、結果を `worklog offer record` で返す。送信前の確認(画面・全文・宛先。
`inbox-adjudication` の「代行送信の前に、本人の確認を取る」)は、スクリプトの外(エージェントと本人)に残る。

## ジャーナル

置き場は `~/.local/state/claude/slot-offer/<提案 id>.json`(環境変数 `WORKLOG_OFFER_DIR` で置き場を上書きできる)。機体ごとのローカルな状態で、
Git にも vault にも入れない。書き込みは追記でなく、一時ファイルに書いて `rename` で置き換える原子的な置換で、提案ごとのロック
(`<提案 id>.lock`)の下で読み書きする。読み込みは未知の項目・未知の状態・壊れた JSON・不変条件の違反(`held` なのに予定 id が無い、など)を
すべてエラーにする(`deny_unknown_fields`)。壊れたジャーナルに対する `record`・`plan` は、黙って上書きせず読み込みで止まる。

```json
{
  "version": 1,
  "id": "offer-20261105-a",
  "state": "held",
  "thread": "thread-1",
  "slots": [
    { "n": 1, "start": "2026-11-10T10:00", "end": "2026-11-10T11:00", "event_id": "ev1" }
  ]
}
```

- 提案 id: 小文字・数字・ハイフンの 64 文字まで(ファイル名になるので、それ以外は受けない)。
- `thread`: 宛先の所在(スレッドの識別子。空白を含まない 1 語)。**メール本文・個人名・宛先の氏名は書かない。**
- `slots[].n`: 指定順に 1 から。押さえの説明欄の印 `[offer:<提案 id>#<n>]` に入る。日時は JST の `YYYY-MM-DDTHH:MM`。
- `slots[].event_id`: 最後の読み戻しで見つかった押さえの予定 id。見つからなければ `null`。

## 状態と事象

表の正本は `vocabulary.json` 1 枚。コードの遷移の可否はその表を引き、`State`・`Event` は閉じた enum(`match` は網羅的)で、
試験が語彙との一致と全マスを確かめる。

| 状態 | 種別 | 意味 |
|---|---|---|
| `intended` | | 意図を記録した。押さえはまだ全部は揃っていない |
| `held` | | 全スロットの押さえを読み戻して確かめた。まだ提示していない |
| `offered` | | 提示した(送信済みの画面を読み戻した) |
| `chosen` | 終端 | 相手が日を選んだ |
| `cancelled` | 終端 | 取り消した。押さえは残っていない |

| 事象 | 遷移 | 関門 |
|---|---|---|
| `intend` | (提案なし) → intended | `worklog offer begin` |
| `observe` | intended・held・offered → 同じ | 押さえの読み戻しの記録。状態は変えない |
| `confirm_holds` | intended → held | 全スロットの押さえが読み戻せたとき |
| `lose_hold` | held → intended | 提示の前に、読み戻した押さえが欠けたとき |
| `present` | held → offered | 全スロットの押さえが揃っているときだけ(**押さえの無い提示を記録できない**) |
| `abort` | intended・held → cancelled | 押さえが 1 件も残っていないと読み戻したとき |
| `retract` | offered → cancelled | 出た提示の取り下げ(本人の判断)。同じく残りが無いと読み戻したとき |
| `choose` | offered → chosen | 選ばれたスロットを記録する |

出た提示を `abort` で取り消すことはできない(`retract` だけ)。出てしまった提示は、スクリプトでは引っ込められないので、相手への連絡を本人が
決めてから押さえを消す。

握手の語彙(`calendar-handshake/vocabulary.json`)の `candidate`(`[hold] `)・`propose` と、この語彙の `held`・`intend` は重なって見えるが、
状態としては重ねない。握手は別リポジトリが内容の sha256 を固定して読むので触らず、押さえの題名の印(`[hold] `)だけを、`include_str!` で
握手の語彙から読む(単一正本。値を複写しない)。語彙の具体化の対応表(`config/agents/vocabulary/refinements/`)は、具体の語彙を
「裁定待ちの項目」(`awaiting-item`)へ写す形で、P1・P2(放置された項目への手段と、本人に見せる担い手)が前提になる。この提案は本人の裁定を
待つ項目でなく、写すと意味を偽るので、対応表は置かない。

## コマンド

```text
worklog offer begin <提案 id> --thread <識別子> --slot <開始>/<終了> [--slot ...]   意図の記録(同じ内容の再実行は「既に記録済み」)
worklog offer plan <提案 id>                                                         通常の流れの次の手
worklog offer record <提案 id> holds --holds <n=予定id,...|none>                     押さえの読み戻しの結果
worklog offer record <提案 id> offered                                               提示した(送信済みの画面の読み戻しのあと)
worklog offer record <提案 id> chosen --slots <n,...>                                相手が選んだ
worklog offer record <提案 id> cancelled [--retracted] --holds none                  取消(押さえが残っていないと読み戻したあと)
worklog offer recover <提案 id> --sent yes|no|unknown --holds <n=予定id,...|none>   中断後の回復の計画(ジャーナルは書き換えない)
worklog offer abort <提案 id> [--retract]                                            巻き戻しの計画(送信に失敗した・本人が送らないと決めた)
worklog offer show <提案 id>
worklog offer list [--all]                                                           終端でない提案(--all で全部)
```

`plan`・`recover`・`abort` は計画を JSON で出す。手(`action`)は閉じた集合で、エージェントが順に実行する。

| action | すること |
|---|---|
| `create_hold` | 押さえを作る。題名は `title`(`[hold] `)、説明欄は `description`(`[offer:<提案 id>#<n>]`)。**同じ印の予定が既にあれば作らない**(カレンダーの `create_event` は予定 id を指定できないので、印で冪等にする) |
| `delete_hold` | 押さえを消す。`event_id` が分からなければ、`marker` を説明欄に持つ予定を探して消す |
| `read_back_holds` | 押さえを読み戻す(`search` を説明欄に持つ予定)。`expect` のスロットが揃い、それ以外が無いこと(空なら 1 件も無いこと) |
| `read_back_sent` | 送信済みの画面を読み戻して、提示の文面が出ているかを確かめる |
| `present` | 本人の送信前の確認を経て、提示を送る。スクリプトの外の手順 |
| `ask_person` | 本人に状態を見せて聞く。**勝手に消したり送ったりしない** |
| `finalize` | 選ばれたスロットを確定し、残りを解放する(`slot-hit.sh finalize`) |
| `record` | 結果を `worklog offer record …` で返す |
| `nothing` | することが無い |

読み戻しの結果は、見つかった押さえを `n=予定id` で渡す(見つからなければ `none`)。`recover` の `--sent` は、送信済みの画面を読めたら
`yes`(文面あり)・`no`(文面なし)、読めなかったら(ログインの壁・画面が開かない)`unknown`。**読めなかったことを `no` と書かない**
(`no` は押さえを消す計画を出す)。

### 通常の流れ(中断なし)

1. `begin`(意図の記録)。
2. `plan` が出す `create_hold` を実行し、`read_back_holds` で読み戻して、`record holds`。全スロットが揃えば `held`。揃わなければ作り直す。
3. 本人の送信前の確認(押さえの一覧を同じ確認に載せる)を経て送り、送信済みの画面を読み戻して、`record offered`。
4. 送信に失敗した・本人が送らないと決めたら、`abort` の計画で押さえをすべて消し、読み戻して、`record cancelled --holds none`。
5. 相手が日を選んだら、`record chosen --slots <n>` と、`slot-hit.sh finalize`(選ばれた枠だけを確定にし、残りを消す)。

## 回復の表

中断のあと(セッションの終了・ツールの失敗・タイムアウト)は、ジャーナルと、実際に確かめた 2 つ(押さえの読み戻しと、送信済みの画面)を
`worklog offer recover` に渡す。計画は次の表で決まる(試験: `crates/worklog/tests/offer.rs` の `the_recovery_table_gives_the_safe_plan_for_each_state`)。

| ジャーナル | 送信済みの画面 | 計画 | 状態の例 |
|---|---|---|---|
| `intended` | 文面なし | 欠けた押さえを作り、読み戻して記録する。**提示はしない**。本人に聞く(送るか、取り消すか) | 意図だけ・押さえが途中まで |
| `intended` | 文面あり | 押さえを揃えて記録し、そのあと提示済みを記録する | 提示だけが出て、押さえが無い |
| `held` | 文面なし | **abort**(押さえをすべて消し、読み戻して、取消を記録する) | 押さえだけが残る・提示が失敗 |
| `held` | 文面あり | 欠けた押さえがあれば作り直し、提示済みを記録する | 両方あり(記録が遅れていた) |
| `offered` | 文面あり | 欠けた押さえだけを作り直す。揃っていれば何もしない | 提示のあとに押さえが消えた |
| `offered` | 文面なし | 欠けた押さえは作り直す。取り下げるか送り直すかを本人に聞く | 提示済みと記録したが、画面に無い |
| `intended`・`held` | 読めない | 押さえは作るが、消さず、送らない。状態を見せて本人に聞く | ログインの壁・画面が開かない |
| `chosen` | (問わない) | 選ばれなかった押さえが残っていれば `finalize` | |
| `cancelled` | (問わない) | 押さえが残っていれば消す | |

`held` で文面が無いときの回復は abort である(Issue #1319 の表のとおり)。本人が送ると決め直したら、回復でなく `plan` の提示の手順に進む。
回復の計画は読み取りだけで、ジャーナルを書き換えない。計画の手を実行し、`record` で結果を返したときに、状態が動く。

## SKILL への反映案

`slot-availability/SKILL.md` と `inbox-adjudication/SKILL.md` は、この文書を書いた段では書き換えていない(Issue #1315 の 5 段の散文が先に入るため、
同じ行を 2 つの段が書き換えない)。散文を `worklog offer` の呼び出しに置き換えるときの文面案を次に示す。

### `slot-availability/SKILL.md` の「発行側の打診」の、仮押さえの 5 段の散文と置き換える

> **候補日を相手に出す返信は、`worklog offer` で押さえと提示を切り離せなくする**(#1319。手順の詳細と回復の表は `docs/agents/slot-offer.md`)。
> 候補一覧型(調整さん等)だけが対象で、予約型は押さえを作らない(上の 2)。
>
> 1. 意図を記録する: `worklog offer begin <提案 id> --thread <スレッドの識別子> --slot <開始>/<終了> ...`(メール本文・氏名は渡さない)。
> 2. `worklog offer plan <提案 id>` の `create_hold` を実行する。題名は `title`、説明欄は `description` をそのまま使い、同じ印の予定が既にあれば作らない。
> 3. 説明欄の `search` で押さえを読み戻し、`worklog offer record <提案 id> holds --holds <n=予定id,...|none>` で返す。全スロットが揃うまで 2 に戻る。
>    **1 件でも作れなければ送らない**(`record offered` が拒否する)。作れなかった提案は `worklog offer abort` の計画で押さえを消す。
> 4. 本人の送信前の確認(`inbox-adjudication`)を経て送る。送信済みの画面を読み戻したら `worklog offer record <提案 id> offered`。
>    送信に失敗した・本人が送らないと決めたら `worklog offer abort <提案 id>` の計画で押さえをすべて消し、読み戻して `record cancelled --holds none`。
> 5. 相手が日を選んだら `worklog offer record <提案 id> chosen --slots <n>` を返し、`slot-hit.sh finalize` で確定する(既存の手順)。
>
> セッションの開始時と、途中で止まった提案があるときは、`worklog offer list` で終端でない提案を見て、`worklog offer recover <提案 id> --sent yes|no|unknown --holds ...`
> の計画に従う。送信済みの画面が読めなかったときは `--sent unknown`(`no` と書かない)。`ask_person` の手は、本人に状態を見せて聞き、勝手に消したり送ったりしない。

### `inbox-adjudication/SKILL.md` の「代行送信の前に、本人の確認を取る」への追記

> 候補日つきの返信を代行で送るときは、確認のページ(画面・全文・宛先)に、`worklog offer plan` が出した押さえ(作る予定の一覧と、読み戻した結果)を
> 載せる。本人が OK と答えたあとに、`worklog offer record <提案 id> offered` が通る状態(押さえが全スロット揃っている)で送る。
> 送信の確認はこのスクリプトの外(エージェントと本人)に残り、スクリプトは確認のあとに呼ばれる前提である。

## 保証しないこと(限界を認める)

- ジャーナルは機体ごとのローカルな状態で、2 機体が同じ提案を別々に進める調整はしない(提案 id は 1 機体で作る)。
- 計画の手を実行したか、読み戻しの結果が正しいかは、エージェントが担う。スクリプトは「結果として返された状態が、表の遷移として許されるか」と
  「押さえが揃うまで提示を記録しない」ことを守る。読み戻さずに `record holds` へ嘘の予定 id を渡す誤りは検出できない。
- 説明欄の印で冪等にするので、人が押さえの説明欄を書き換えると、読み戻しに載らず、作り直される(重複する側に倒れる)。
- Google Calendar の API を直接使えれば、予定 id を指定して冪等にできる(未確認)。リモート MCP の `create_event` は予定 id を指定できない。
- Quint の仕様は書いていない。試験が、語彙の遷移表の全マスと、中断の各段(意図の記録後・押さえ作成後・提示後)を表で確かめる。

## 検査

- `scripts/vocab-graph-check --all`(語彙の形と、到達・出口・担い手の解決)。
- `cargo test -p worklog`: `tests/offer.rs` が、語彙と enum の一致、遷移表の全マス、押さえが揃うまで提示を記録しないこと、
  回復の表、中断の各段を偽のカレンダーと偽の送信で通すこと、壊れたジャーナル・未知の状態が読み込みで落ちること、
  CLI の通しを確かめる。
