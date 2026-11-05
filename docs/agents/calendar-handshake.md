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

遷移は 6 行(`from` × `to` に展開すると 15 本)。表にない遷移は、script は起こさない。

| 遷移元 | 遷移先 | 遷移させる主体 |
|---|---|---|
| `candidate` | `ok`・`resched`・`ng` のいずれか | workplace-script |
| `ok`・`resched` | `go` | person |
| `go` | `blocked` | workplace-script |
| `blocked` | `secured` | workplace-agent |
| `blocked`・`secured` | `go` | workplace-script(不在と食い違った確保を取り直す) |
| `ok`・`resched`・`ng`・`go`・`blocked`・`secured` | `candidate` | person(取り下げ・やり直し) |

`ng` は、本人が `candidate` に戻すまで動かない。`secured` から出るのは、不在との食い違いを script が見つけたときの `go` だけである。
語彙の版 2 で、下 2 行を足した(版 1 は上 4 行)。足した理由と検査の結果は、仕様の節と ADR にある。

版 3 で、語彙 JSON を対象の語彙の共通の形(`docs/agents/state-vocabulary.md`、ADR-1293)に移した。
`transitions` を `events` に改め、事象に id を付けた(上の 6 行は `judge`・`confirm`・`reserve`・`secure`・`reopen`・`withdraw`)。
`owner`・`by` の `workplace-script` は `machine`、`workplace-agent` は `agent` にし、元の名前は `owner_detail`・`by_detail` に残した。
予定を作る事象 `propose`(`none` → `candidate`、person。担い手は `slot-availability`)を足した。`propose` は Quint の遷移の表に含めない
(仕様の `personCreate` は遷移の記録を更新しないため)。`token`・`hold`・`group_key` は変えていない。

版 4 で、予定の種類(`kinds`)を足した(`docs/adr/1371-calendar-handshake-remote-kind.md`)。状態の印と遷移の表は変えていない。

## 種類(kinds)

予定は、勤務先の不在を入れるかどうかで 2 つの種類に分かれる。種類は、題名の状態の印の直後の tag で表す。

| 種類 | tag | 題名の例 | 意味 |
|---|---|---|---|
| `leave` | なし(既定) | `[hold] 〇〇` | 仕事を離れる予定。`blocked` で、勤務先のカレンダーに不在が入る |
| `remote-trip` | `[remote] ` | `[hold] [remote] 〇〇` | 別の場所での勤務。仕事は続くので、不在は入らない |

tag が無ければ `leave` である。状態の印は、種類によらず同じ 7 つで、印だけを付け替える(`[hold] [remote] 〇〇` が判定で `[ok] [remote] 〇〇` になる)。
組の id に使う題名の残りは、状態の印を除いたもの(tag を含む)で、変えない。

種類ごとに、使える事象が決まる(事象の `kinds`)。`remote-trip` は、予定を作る `propose`、判定の `judge`、取り下げの `withdraw` だけを持つ。
`confirm`・`reserve`・`secure`・`reopen` は `leave` だけである。そのため `remote-trip` は、`candidate`・`ok`・`resched`・`ng` にしか至らず
(語彙の `reaches`。`scripts/vocab-graph-check` の規則「種類」が、事象を足してこの上限を超えれば落とす)、`go`・`blocked`・`secured` には至らない。

`remote-trip` に `[go] ` を書くことは、語彙に無い書き換えである。会社側は、規則 6(S6)のとおり、最後に受理した印へ差し戻し、不在を入れない。
利用側が `remote-trip` に `[go] ` へ進める手順を出さないことを、仕様は前提にする(`personGo` は `leave` だけ)。
`remote-trip` の予定は、判定の結果(`ok`・`resched`・`ng`)が出た時点で、会社側の仕事が終わる。

種類を知らない会社側(版 3 までの実装)は、tag を題名の残りとして読み、すべて `leave` として扱う。そこでは `remote-trip` の `[go] ` が
語彙の遷移として受理され、不在が入る(仕様の `s7_unaware`)。`remote-trip` の予定は、会社側が版 4 の規則に配備し直したあとに置く。

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

同じ組の予定は、組の id が一致するものとする(`group_key` = `stamped-id-else-title-after-token`)。
組の id は、script が予定を最初に判定するとき、題名から先頭の印を除いた残りを、予定の拡張プロパティに刻んだものである。
刻む前の予定は、題名の残りを組の id の代わりにする。刻んだあとに題名の残りを直しても、組は割れない。

組の id を刻むのは、版 1 の「題名の残りが一致するもの」では、本人が片方の題名を直すと組が割れ、
`go` のときに他の候補が消えなかったためである(仕様の検査で反例が出た)。残る穴は、
最初の判定の前(10 分以内)に題名を直す場合で、このときは別の組の id が刻まれる。これは仕様では防げず、受容する。

## 仕様と検査

握手のプロトコルは `spec/handshake.qnt`(Quint)に書き、TLC で検査する。遷移の可否の正本はこの語彙 JSON で、
仕様は検査のたびに、`spec/vocab.jq` で JSON から生成したモジュールを読む(写しをコミットしない)。

| ファイル | 内容 |
|---|---|
| `spec/handshake.qnt` | 参加者(本人・会社側 script・会社 PC のエージェント・朝の机)、共有記憶(専用カレンダー・会社カレンダーの不在)、環境の動作(本人の任意の書き換え・削除、script の途中の失敗と重複実行)、性質 |
| `spec/scenarios.json` | 検査の表。規則(ADR-550 のまま / 改訂後)と環境の組と、期待(通る / 反例が出る) |
| `spec/vocab.jq` | 語彙 JSON から、仕様が import する `vocab.qnt` を作る |

実行は `nix build .#checks.x86_64-linux.calendar-handshake-spec -L`(`home/tests/calendar-handshake-spec.nix`)。
表の 1 行が 1 つの derivation で、CI の `calendar-handshake-spec` ジョブ(`.github/workflows/ci.yml`。
握手の語彙・仕様・検査の定義が変わった PR と main への push で回す)が回す。`flake-check` は `--no-build` なので、評価しか行わない。期待が「反例が出る」の行は、版 1 の規則の穴を回帰テストにしたもので、
改訂を戻して反例が出なくなれば、検査が赤になる。語彙の遷移ごとに、仕様がその遷移を起こせる(到達可能)ことも確かめ、
語彙に死んだ遷移があれば赤になる(遷移は、種類 × `from` × `to` に展開する。種類の事象だけでは到達できない `from` の組は展開しない。`spec/vocab.jq`)。
逆に、仕様が語彙に無い遷移を起こせば、不変条件 `s6` が破れる。

性質と、検査の対応:

| 性質 | 内容 | 検査 |
|---|---|---|
| S1 | 同じ会議の候補で、`blocked`・`secured` は高々 1 件 | 不変条件 `s1` |
| S2 | 不在は二重に入らない。`blocked`・`secured` には、日の合った不在がちょうど 1 件ある | 不変条件 `s2dup`・`s2blk`、活性 `s2heal` |
| S3 | `blocked` のあと、同じ会議の他の候補は、判定待ちの `hold` と `ng` の他に残らない | 不変条件 `s3` |
| S4 | 予定が消えた・印が `hold` などに戻った不在は、いずれ消える | 活性 `s4` |
| S5 | 会社側が読めるのは専用カレンダーだけ | 仕様の変数に主カレンダーが無い(検査ではなく構造。専用カレンダーだけを会社に共有する設計で担保) |
| S6 | 仕様が起こす遷移はすべて語彙にある(種類つき)。許されない書き換えは、最後に受理した印へ差し戻される | 不変条件 `s6`、活性 `s6revert` |
| S7 | `remote-trip` の予定には、不在が入らない(本人が手書きで `go` などを書いても、script は受理せず差し戻す) | 不変条件 `s7`。対照 `s7_unaware`(種類を知らない会社側の規則では破れる) |
| L1 | `go` は、いずれ `go` でなくなる | 活性 `l1` |
| L2 | 本人が送った `blocked` は、いずれ `blocked` でなくなる | 活性 `l2` |
| L3 | 停滞し続ける予定は、いずれ朝の机の警告に達する | 活性 `l3` |

活性は、script が動き続け、落ちる回数・本人の書き換えの回数が有限である、という公平性の仮定のもとで検査する。

## 会社側の実装が守る規則

検査を通った規則である。会社側の実装は、これを守る。

1. 予定の書き込みは、読んだときの ETag を `If-Match` に付ける。変更されていれば 412 で失敗し、何も書かずに終える。
2. 1 回の実行全体を、`LockService.getScriptLock()` で囲む。ロックはスクリプトの終了で解放される。
   冪等(作る前に探す)だけでは、2 つの実行が同時に探して同時に作る場合に不在が二重に入る。
3. 不在は、作る前に、印の予定 id の拡張プロパティで探す。あれば作らない。
4. `go` の処理は、不在を入れる → 同じ組の他の候補を削除する → 印を `blocked` に直す、の順。確定の点は印の書き換えである。
5. 最初の判定で、組の id を刻む。閉じた組(同じ組に `blocked`・`secured` がある)への新しい候補は、判定で `ng` にする。
6. 印を書くたびに、最後に受理した印も拡張プロパティに書く。読んだ印が最後に受理した印からの語彙の遷移でなければ、最後に受理した印へ差し戻す。
7. `blocked`・`secured` の予定は、不在がちょうど 1 件あり、不在の日と予定の日が一致するかを確かめる。食い違えば、不在を消して `go` に戻す。
8. 予定が消えた、または印が `hold`・`ok`・`resched`・`ng` に戻った予定の不在は消す。
9. 題名の状態の印の直後の tag で、予定の種類を読む(tag が無ければ `leave`)。遷移の可否は種類ごとの集合で判定する。
   `remote-trip` に `go` が書かれたら、規則 6 のとおり最後に受理した印へ差し戻し、不在を入れない。

拡張プロパティの種類(`private` か `shared` か)は、共有カレンダーで会社側と個人側が同じ値を読み書きできるか、
会社 PC での実装時に実測して決める(未確認)。

## 仕様の限界

- 規模は、予定 2 件・題名 2 種・開始日 2 種・実行 2 本・本人の書き込み 3〜5 回・落ちる回数 1 回に絞った有限モデルである。
- 不在の掃除(規則 8)は、仕様では 1 ステップで終わると仮定した。実際は複数の API 呼び出しである。
- 種類は予定ごとに固定した。本人が題名の tag を書き換えて種類を変えること(`[remote] ` を消して `leave` にするなど)は、
  モデルに入れていない。書き換えれば別の種類の予定として読まれ、`leave` の遷移集合に従う(本人の意図した変更である)。
  題名の残りが同じでも tag が違えば別の組になる(組の id が tag を含むため)。仕様の題名は抽象値なので、この区別は仕様に出ない。
- 時刻と日は抽象値である。10 分ごとのトリガの実行時間と、開始の 3 日前の境は、整数の大小で表した。
- 本人が自分の予定の印を書き換える(`secured` と書くなど)ことは、カレンダーが書き手を区別できないので防げない。
  会社側が保証するのは、`blocked`・`secured` に不在が付いていることだけである。

## 勤務先の機体の起動時 hook(`calendar-handshake-reminder`)

勤務先の機体のエージェントが、下書き待ちの印(`blocked`、`[blocked] `)の予定を放置しないための、
SessionStart(matcher: `startup`)の hook(#1239)。`crates/calendar-handshake-reminder` が、
次の手順を `additionalContext` に注入する。

- 起動時の hook からはカレンダーのコネクタを読めない(Claude Code の hooks の文書、取得 2026-11-05)ので、
  hook は件数を数えない。握手の専用カレンダーで題名が `[blocked] ` で始まる予定を探すのは、エージェントが行う。
  読めない、または 0 件なら、本人に何も報告せず、依頼を進める。
- 1 件以上あれば、本人に件数を伝え、予定ごとに休みの連絡・リスケの依頼・半休の申請の下書きを作る。
  下書きは Slack・メールの下書きまでで止め、送信と申請の確定は本人がする(外部発信は下書き止まり)。
- 本人が送った(申請した)ことを、送信済みの状態などで読み戻して確かめたら、題名の印を `[secured] ` に直す
  (`secure` の事象。`blocked` → `secured`)。確かめる手段が無い申請は、本人に済んだかを聞き、
  「本人申告」と明記して記録してから印を直す。確かめていないものを `[secured] ` にしない。

印のトークンは、このファイルの語彙 JSON をコンパイル時に読んで取る(コードに写さない)。

登録は option `dotfiles.calendarHandshakeReminder.enable`(既定 false、`home/modules/agents.nix`)で行い、
勤務先の機体(`home/hosts/arcturus.nix`)でだけ true にする。他の機体では登録されず、無音である。
完了条件の「勤務先の PC で `[blocked] ` の合成の予定を 1 件置き、手順が実行されて下書きが作られる」確認は、
その機体の人手の作業である(未実施)。

## 実装との関係

このリポジトリが持つのは語彙の JSON と、この文書と、仕様と検査と、勤務先の機体の起動時 hook である。
`slot-availability` のコードは変えない。印を読み書きする側の実装(会社側の script)は、利用側が持つ。
