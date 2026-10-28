# ADR-0000 — 個人と勤務先のカレンダーの握手を Quint の仕様に書き、TLC で検査する。語彙は版 2 に上げる

- Status: Accepted
- Date: 2026-10-28
- Issue: No-Issue(利用側の設計の依頼。握手の語彙 ADR を入れた PR #1238 の続き。利用側のリポジトリ名と番号は、この公開リポジトリには書かない)

## Context

`config/agents/calendar-handshake/vocabulary.json`(#1238)は、個人の専用カレンダーの題名の先頭の印で、個人と勤務先の予定を受け渡す握手の状態と遷移を定めた。
参加者は 4 者で、本人、勤務先の Apps Script(10 分ごと)、勤務先 PC のエージェント(起動時だけ)、個人側の朝の机である。
共有記憶は、専用カレンダー上の予定の題名と、勤務先カレンダー上の不在で、メッセージでなく、書き換えられる共有の状態で受け渡す。

この構造には、遷移の表だけでは書けない失敗がある。

- 本人は印を任意の値に書き換えられる(スマホから)。表の外の書き込みも起きる。
- script は、複数の書き込みの途中で落ち、トリガは重複して動く。
- 本人と script が、同時に同じ予定を書く。

勤務先側の実装(Apps Script)に入る前に、この握手をプロトコルとして書き、安全性と活性を検査して、設計の穴を実装の前に見つける。

## Decision

### D1. 仕様は Quint で書き、TLC で検査する

候補を比べ、同じ縮小モデル(H2: `[go]` の処理中に、不在を入れたあと印を直す前に script が落ちる)を Quint と TLA+ の両方で書いて動かした(2026-10-28、nixpkgs の `quint` 0.33.0・`tlaplus` 1.7.4)。

| 観点 | Quint(`--backend tlc`) | TLA+(TLC) |
|---|---|---|
| 反例 | 出る(不在が 2 件) | 同じ反例 |
| 弱公平性つきの活性 | 検査できる。公平性を外した対照では、`[go]` のまま止まる反例(Stuttering)が出る | 検査できる |
| nix のビルド sandbox(ネットワークなし、HOME が書けない) | 通る。Apalache が Quint の package に同梱される | 未実測(TLC は JVM だけなので通る見込み) |
| 語彙 JSON の取り込み | `jq` で生成したモジュールを import できる。被覆の反例も出る | `tla2tools.jar` に Json モジュールが無い。CommunityModules を足すか、同じく `jq` で生成する |
| 型 | 静的な型検査が、検査の前に記述の誤りを落とす(公平性の引数の個数の誤りが、型検査で落ちた) | `TypeOK` は検査時の不変条件 |

軸 1(表現不可能性: 型・コンパイル時 > 実行時検査)で Quint が勝つ。活性の公平性は TLC backend で検査する。Quint の既定の backend である Apalache は、
公平性(WF/SF)を持たない(Apalache の `features.md`: "`WF_e(A)` ✖ … Has to be specified manually")ので、使わない。
憧れ駆動(TLA+ が先に思い浮かんだ)で始めた選定で、Alloy 6・SPIN・P は感触で外した(共有記憶と環境の任意の書き込みを書く形が遠い。分析ではない)。

Quint は Node と JVM を要し、仕様は Apalache を経由して TLC に渡る。層が 1 つ多い。TLA+ を直接書く場合に比べ、この層を許すのは、型検査が効くためである。

### D2. 語彙 JSON を遷移の単一正本とし、仕様は検査のたびに語彙から検査用モジュールを生成して読む

`spec/vocab.jq` が `vocabulary.json` から `vocab.qnt`(`from` × `to` に展開した遷移の一覧と状態の集合)を作る。生成物はコミットしない。依頼の 2 案(仕様から語彙を生成する、語彙が仕様の部分集合であることを検査する)は、
どちらも写しを作るので採らない。一致は、次の 2 つの検査で担う(双方向)。

- 仕様が起こす遷移はすべて語彙にある(不変条件 `s6`)。仕様に書いた改訂の規則を版 1 の語彙で回すと、本人の取り下げで `s6` が破れた(語彙の穴の検出)。
- 語彙の遷移はすべて、仕様が起こせる(到達可能)。`spec/scenarios.json` の `cover` が、遷移ごとに到達を確かめ、死んだ遷移があれば赤になる。

### D3. 仕様・検査・語彙は同じ公開リポジトリに置く

`config/agents/calendar-handshake/` の下に置く。勤務先側の実装が同じ場所から読め、語彙と仕様を 1 つの PR で一緒に変えられる。個人や勤務先の情報は入れない。

### D4. 検査は nix の check にし、表の 1 行を 1 つの derivation にする

`home/tests/calendar-handshake-spec.nix` が `spec/scenarios.json` を読み、`flake.nix` の `checks.x86_64-linux.calendar-handshake-spec` に登録する。
Apalache のサーバーは 8822 番ポートを使うので、同じ環境では 2 つ同時に回せない。derivation ごとの sandbox に分けると、この衝突が無い。
darwin では回さない(sandbox の分離が前提)。expect が `fail` の行は、ADR-550 のままの規則の穴を回帰テストにしたもので、反例が出なくなれば赤になる。

### D5. 検査で見つけた穴の処分(改訂した規則)

| 穴 | 反例 | 処分 | 改訂した規則 |
|---|---|---|---|
| H1 組のキーが題名の一致 | 本人が片方の題名を直すと組が割れ、`[go]` のときに他の候補が消えない | 規則を改訂 | script が最初の判定で組の id を拡張プロパティに刻む。刻んだあとの題名の編集では組が割れない |
| H1 の残余 | 最初の判定の前(10 分以内)に題名を直すと、別の id が刻まれる | 受容 | 仕様では防げない。環境の仮定(判定のあとに編集する)を外した行 `h1_residual` が、反例を残す |
| H2 落ちたあとの再実行 | 不在が二重に入る | 規則を改訂 | 不在を作る前に、拡張プロパティで探す(冪等) |
| H4 トリガの重複実行 | 冪等でも、2 つの実行が同時に探して同時に作ると、不在が二重に入る | 規則を改訂 | 1 回の実行全体を script lock で囲む。冪等だけでは足りない |
| H3 `[blocked]` を `[hold]` に戻す | 不在が残り続ける | 規則を改訂 | 印が `blocked`・`secured` でなくなったら、不在を消す。語彙に、本人の取り下げ(`ok`/`resched`/`ng`/`go`/`blocked`/`secured` → `candidate`)を足す |
| H5 `[secured]` のあとに日を変える | 不在の日が食い違ったまま残る | 規則を改訂 | 不在の日と印の日が食い違えば、不在を消して `go` に戻す(取り直す)。語彙に `blocked`/`secured` → `go`(script)を足す |
| H6 同じ組の別の候補が既に `[blocked]` | 後から来た候補の `[go]` も `[blocked]` になり、確保が 2 件になる | 規則を改訂 | 閉じた組への新しい候補は、判定の時点で `[ng]` にする |
| S6 許されない書き換え | 語彙に無い書き換え(例: `[ng]` を `[go]` に)が残る | 規則を改訂 | 最後に受理した印を拡張プロパティに持ち、語彙の遷移でない書き換えは、その印へ差し戻す |

見つけた死んだ機構: 閉じた組への `[go]` を `[ng]` にする遷移(`go → ng`)を、最初の改訂案に入れていた。被覆の検査で到達不能と出た。
落ちる実行・2 つの実行・題名の編集・削除・取り下げ・日の編集を全部入れた環境(書き込み 5 回、335 万状態)でも到達しなかった。判定の時点の閉じ込めと、`[go]` の処理の候補削除が先に効き、
`[go]` の時点で組が閉じていることが無いためである。還元性(死んだ機構は消す)により、規則と語彙から外した。

計画の段階で名前を付けた H7(別の会議が同じ題名だと、同じ組になる)は、検査の対象にしていない(仕様は、会議の同一性を題名と組の id でしか見ない)。受容し、題名を分ける運用で避ける。

### D6. 実装の適合は、仕様のトレースを実装に流して確かめる

勤務先側の実装の純粋な関数(判定・不在の要否・組の削除・差し戻し)に、仕様が出すトレース(Quint の ITF: `quint run --out-itf`)を `node:test` で再生する。
先行例(Cirstea, Kuppe, Loillier, Merz, "Validating Traces of Distributed Programs Against TLA+ Specifications", arXiv:2404.16075v2, 2024-09-17、取得 2026-10-28)は、
実装のログを仕様に照らす向きで、トレースは JSON で出る。ここでは逆に、仕様のトレースを実装に流す。勤務先のログを公開リポジトリへ持ち出さずに済むためである。
本番のログの検証は、記録の置き場が決まってから検討する。トレースを実装に流す再生の部品は、実装(利用側)で作る。この PR は仕様側の義務(トレースが出せること)までを担う。

## 前提(外部の挙動。一次情報、取得 2026-10-28)

- A1: Calendar API は、更新・削除に `If-Match`(前回の取得の ETag)を付けると、変更されていれば 412 を返す。Google, "Calendar API: Version resources and conditional modification"
  (最終更新 2026-09-11) https://developers.google.com/workspace/calendar/api/guides/version-resources — "Otherwise, you will get a 412 (Precondition failed) response code."
  仕様は、script の書き込みを「読んだときから予定が変わっていなければ成功」として扱う。
- A2: LockService のロックは、スクリプトの終了で解放される。Google, "Class Lock"(最終更新 2026-04-13) https://developers.google.com/apps-script/reference/lock/lock —
  "The lock is automatically released when the script terminates, but for efficiency it is best to release it as soon as you no longer need exclusive access to a section of code."
  同じく "Class LockService": "A code section guarded by a script lock cannot be executed simultaneously regardless of the identity of the user."(最終更新 2026-04-13)
- A3: 不在の掃除(`orphanScan`)は 1 ステップで終わると仮定した。実際は複数の API 呼び出しで、この仮定は楽観的である。
- 拡張プロパティ: 検索は `privateExtendedProperty`/`sharedExtendedProperty` の `propertyName=value`(Google, "Extended properties"、最終更新 2026-09-11)。
  共有カレンダー上の予定の private プロパティを、勤務先側と個人側が同じ値として読み書きできるかは未確認で、勤務先 PC での実装時に実測して決める。

## 検査の結果(2026-10-28、flake が固定する nixpkgs の Quint 0.32.0・Apalache 0.56.1、nix の sandbox の中。33 行すべてが期待どおり)

| 行 | 性質 | 期待 | 結果 | 状態数 |
|---|---|---|---|---|
| `h1_asis` | S3 | 反例 | 反例(題名の編集で組が割れる) | 15,815 |
| `h1_revised` | S1・S3・S6 | 通る | 通った | 40,993 |
| `h1_residual` | S3 | 反例 | 反例(判定前の編集。受容) | 39,587 |
| `h2_asis` | S2(重複) | 反例 | 反例(落ちて再実行) | 2,563 |
| `h2_revised` | S2 | 通る | 通った | 2,753 |
| `h4_asis` | S2(重複) | 反例 | 反例(冪等でも同時に作る) | 7,721 |
| `h4_revised` | S2 | 通る | 通った | 2,225 |
| `h3_asis` | S4 | 反例 | 反例(取り下げで不在が残る) | 11,081 |
| `h3_revised` | S4 | 通る | 通った | 13,761 |
| `h5_asis` | S2(食い違いは直る) | 反例 | 反例(日の編集) | 9,473 |
| `h5_revised` | S2 | 通る | 通った | 10,697 |
| `h6_asis` | S1 | 反例 | 反例(確保が 2 件) | 12,899 |
| `h6_revised` | S1・S3 | 通る | 通った | 15,785 |
| `s6_asis` | S6(応答) | 反例 | 反例(許されない書き換えが残る) | 2,297 |
| `s6_revised` | S6・S2 | 通る | 通った | 53,185 |
| `l1_revised` | L1 | 通る | 通った | 18,817 |
| `l2_revised` | L2 | 通る | 通った | 7,945 |
| `l3_revised` | L3 | 通る | 通った | 55,664 |
| `cover_0`〜`cover_14` | 語彙の遷移 15 本の到達可能性 | 反例(到達する) | 全部到達 | 96〜67,659 |

L3 の最初の定式化(「停滞した瞬間があれば、いずれ警告に達する」)は、反例が出た(一瞬だけ停滞して、机が動く前に `[secured]` になる予定)。
停滞し続ける予定に直した。机の警告は、解消した一瞬の停滞には出せず、出す必要もない。

S5(会社側が読めるのは専用カレンダーだけ)は、検査でなく構造で担う。仕様の変数に主カレンダーが無い。専用カレンダーだけを会社に共有する設計(ADR-550 D1)が担保で、この仕様がそれを検査しているわけではない。

## 先行例との対比

- D1: Newcombe et al., "Use of Formal Methods at Amazon Web Services", 2014-09-29 https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf (取得 2026-10-28)。差分: 異なる — 先行例は TLA+ を直接書く。Quint は同じ TLC を使う型つきの表記。
- D6: Cirstea et al., arXiv:2404.16075v2(上記)。差分: 異なる — 向きが逆(仕様のトレースを実装に流す)。

## 却下した案

- Alloy 6・SPIN・P — 感触で外した(共有記憶と環境の任意の書き込みを書く形が遠い)。分析ではない。
- 仕様から語彙を生成する案 — 写しを作る。語彙 JSON が正本である。
- Apalache を直接の検査器にする案 — 公平性(WF/SF)を持たず、活性を検査できない。

## 執行点

- `config/agents/calendar-handshake/spec/handshake.qnt`
- `config/agents/calendar-handshake/spec/scenarios.json`
- `config/agents/calendar-handshake/spec/vocab.jq`
- `config/agents/calendar-handshake/vocabulary.json`
- `home/tests/calendar-handshake-spec.nix`
- `flake.nix`

## Consequences

- 語彙は版 2 に上がる(遷移を 2 行足す。`group_key` を改める)。利用側は、このリポジトリのマージ後に、自分の pin を新しい hash へ上げる。
- 勤務先側の実装は、`docs/agents/calendar-handshake.md` の「会社側の実装が守る規則」を守る。
- 語彙を変える PR は、`checks.x86_64-linux.calendar-handshake-spec` を通す必要がある。語彙に死んだ遷移を足せば被覆で、仕様に無い遷移を許せば不変条件で落ちる。
- 仕様は有限のモデルで、規模・不在の掃除の原子性・時刻の抽象は `docs/agents/calendar-handshake.md` の「仕様の限界」にある。
