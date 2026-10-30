# task-slot — 作業の枠の「済んだ・やめた」を、作業ログに完了記録として残す

決定の記録: `docs/adr/1295-slot-completion-record.md`(ADR-1291 の slot kind の仕様を置き換える。作りは ADR-1259 の `mail` と同じ)
語彙の正本: `config/agents/task-slot/vocabulary.json`(版 3。共通の形は `docs/agents/state-vocabulary.md`)
実体: `crates/worklog/src/slot.rs`(文法・畳み込み・書き込みの検査)、`crates/worklog/src/main.rs`(`worklog add slot`・`worklog slot`)

## 何を解くか

自分一人でやる作業の枠(時間枠つき)の**時間の定義は予定データ(Git)に残る**。カレンダーへ射影され、busy と通知に使われる。
作業ログに移すのは、本人が「済んだ」「やめた」と裁いた完了記録だけで、vault の当日ノートの 1 行で残す。時間を作業ログに
持たないので、「未了」「終了を過ぎた」は作業ログでは導かない。読む側が、予定データの終了時刻と完了記録を突き合わせて導く。

## 状態と出来事

状態は保存しない。vault の当日ノートの `[slot]` 行を出来事とみなし、同じ鍵の行を日付・時刻の順にたどって決める。
表の正本は `vocabulary.json` 1 枚。

| 状態 | 種別 | 意味 |
|---|---|---|
| `done` | 終端 | 済んだ |
| `dropped` | 終端 | やめた |

| 出来事 | 遷移 | by |
|---|---|---|
| `done` | (記録なし) → done | person |
| `drop` | (記録なし) → dropped | person |

済んだかどうかは本人にしか分からないので、本人に確かめて書く(`by` は書く主体の記録で、コードは強制しない)。
担い手は `cli:worklog` と、朝の机が本人に確かめて書く `skill:daily-desk`。機械が書く出来事は無い(語彙の読み込みが
`machine` を拒否する)。

## 行の形と鍵

```text
- 12:30 [slot] done: example-slot-20261101
- 12:31 [slot] drop: other-slot-20261102
```

- 鍵は予定データの予定 id(kebab-case、128 文字まで。数字を含んでよい)。時間枠・Issue・本文・個人名を持つ欄は無い。
  `owner/repo` の参照(`--ref`・`--repo`)は付けない。
- 同じ鍵に `done` と `drop` の両方は書けない。先に書かれた終端が勝ち、あとから別の終端を書こうとすると拒否する
  (exit 2)。同じ事象の再実行は「記録済み」(exit 0)で、行は増えない。

## 使い方

```text
worklog add slot done <鍵> | drop <鍵>
worklog slot closed [--json]    # 記録済みの鍵を、書かれた順に
worklog slot check              # 食い違いの一覧。あれば exit 1
```

```text
$ worklog slot closed
example-slot-20261101	done	2026-11-01
$ worklog slot closed --json
[{"day":"2026-11-01","key":"example-slot-20261101","state":"done"}]
```

出力は 1 行 1 件のタブ区切り `鍵<TAB>状態(done|dropped)<TAB>日付`、または `--json`(`{"key","state","day"}` の配列)。
読む側(予定データを持つ側の、過ぎた予定を拾うスクリプト)は `--json` を呼び、予定 id が載る予定を済み扱いにする。

`check` は、同じ鍵に別々の終端が書かれている行(合流した別の機体や手書きで入る)と、解析できない行を出す。
旧い形の行(`plan:`・`resched:`・時間枠つき)も解析できない行になる。

## 保証しないこと(限界を認める)

- 終端の行を手で消すと、その枠は記録の無い状態に戻る。`mail` のような控えは持たない: 無人で書く `sweep` が無く、
  読む側から未記録に見えれば、本人が裁き直せる。
- 切り替え日(`2026-10-30`)より前のノートは畳み込まない。読む範囲は `list` の上限(400 日)に収まる分まで。
- 並行する 2 機体が同じ枠に別の終端を書いて合流すると、時刻の早い方が勝ち、遅い方は `check` が食い違いに出す。

## 検査

Quint の仕様は無い。`crates/worklog/tests/slot.rs` が、語彙 JSON から読んだ遷移表の全マス、終端から出る出来事が無いこと、
同じ鍵の二重終端の扱い、畳み込みが行の読み込み順に依らないことを確かめる。`crates/worklog/tests/cli.rs` が、CLI を通しで
確かめる。
