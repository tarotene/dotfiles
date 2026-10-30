# task-slot — 作業の枠が、状態をたどって本人の裁定で終わる

決定の記録: `docs/adr/1291-task-slot-worklog.md`(ADR-998 の追記。作りは ADR-1259 の `mail` と同じ)
語彙の正本: `config/agents/task-slot/vocabulary.json`(版 2。共通の形は `docs/agents/state-vocabulary.md`)
実体: `crates/worklog/src/slot.rs`(文法・畳み込み・書き込みの検査)、`crates/worklog/src/main.rs`(`worklog add slot`・`worklog slot`)

## 何を解くか

自分一人でやる作業の枠(出向く予定ではないもの。時間枠つき)の状態を、作る・済んだ・組み直す・やめる、の
たびに Git の PR にせず、vault の当日ノートの 1 行で追う。`mail` と違い、タイマーは無い。再通知も失効もせず、
枠は本人が裁くまで未了のまま残る。「終了を過ぎた」は終了時刻から導く値で、状態としては保存しない。

## 状態と出来事

状態は保存しない。vault の当日ノートの `[slot]` 行を出来事とみなし、同じ鍵の行を順にたどって決める。
表の正本は `vocabulary.json` 1 枚。

| 状態 | 種別 | 意味 |
|---|---|---|
| `scheduled` | 未了 | 時間枠を取った。済んだか・やめたかは、まだ裁かれていない |
| `done` | 終端 | 済んだ |
| `dropped` | 終端 | やめた |

| 出来事 | 遷移 | by |
|---|---|---|
| `plan` | (新規) → scheduled(時間枠が要る) | agent |
| `resched` | scheduled → scheduled(時間枠を書き換える) | person |
| `done` | scheduled → done | person |
| `drop` | scheduled → dropped | person |

済んだかどうかは本人にしか分からないので、`resched`・`done`・`drop` は本人の裁定を受けて書く(`by` は
書く主体の記録で、コードは強制しない)。機械が書く出来事は無い(語彙の読み込みが `machine` を拒否する)。

## 行の形と鍵

```text
- 09:12 [slot] plan: reader-bot-resume [start:2026-11-02T10:00+09:00 end:2026-11-02T12:00+09:00 issue:example/repo#12]
- 09:15 [slot] resched: reader-bot-resume [start:2026-11-03T10:00+09:00 end:2026-11-03T12:00+09:00]
- 12:30 [slot] done: reader-bot-resume
- 12:31 [slot] drop: other-slot
```

- 鍵は枠の短い id(kebab-case、64 文字まで)。同じ鍵は二度 `plan` できず、終端の後も使い回せない。
  同じ内容の `plan` の再実行だけは「記録済み」になる。
- `start:`・`end:` は ISO 8601(`YYYY-MM-DDTHH:MM[:SS]` に `Z` か `±HH:MM`)。オフセットが無い時刻と、
  `end` が `start` 以前の組は書けない。`done`・`drop` に印は付けない。
- `issue:OWNER/REPO#N` は `plan` の行だけが、複数持てる。`resched` は参照 Issue を変えない。
- 本文・要旨・個人名を持つ欄は無い。`owner/repo` の参照(`--ref`・`--repo`)は付けない。
- 書き込み口は本文を正規の形(`start`・`end`・`issue` の順)にそろえる。手で書いた順違いの行も読める。

## 使い方

```text
worklog add slot plan <鍵> --start <ISO 時刻> --end <ISO 時刻> [--issue owner/repo#N …]
worklog add slot resched <鍵> --start <ISO 時刻> --end <ISO 時刻>
worklog add slot done <鍵> | drop <鍵>
worklog slot open [--json]                 # 未了の一覧。開始の早い順
worklog slot overdue [--now <ISO 時刻>] [--json]   # 未了のうち終了を過ぎたもの。終了の古い順
worklog slot check                         # 食い違いの一覧。あれば exit 1
```

出力は 1 行 1 件のタブ区切り `鍵<TAB>開始<TAB>終了<TAB>Issue(`,` 区切り。無ければ `-`)`、または `--json`
(`{"key","state","start","end","issues"}` の配列)。

```text
$ worklog slot open
reader-bot-resume	2026-11-02T10:00+09:00	2026-11-02T12:00+09:00	example/repo#12
$ worklog slot overdue --now 2026-11-02T13:00+09:00 --json
[{"end":"2026-11-02T12:00+09:00","issues":["example/repo#12"],"key":"reader-bot-resume","start":"2026-11-02T10:00+09:00","state":"scheduled"}]
```

読む側が使う順: 朝に `worklog slot overdue` を引き、出たものを本人に「済んだか・組み直すか・やめるか」と
聞いて、答えを `done`・`resched`・`drop` で書く。次に `worklog slot open` で、これからの枠を見る。

- 「過ぎた」は `終了 < 今`(終了ちょうどは、まだ過ぎていない)。`--now` が無ければ今の時刻。`--now` を渡すと、
  読むノートもその日(JST)までになる。
- 許されない出来事(開いていない枠への `done`、終端の後の `drop`、使い回した鍵の `plan`)は、書かずに止まる
  (exit 2)。今の状態と同じになる再実行(同じ `plan`、今の時間枠と同じ `resched`、既にその終端にある
  `done`・`drop`)は「記録済み」。

## 保証しないこと(限界を認める)

- 終端の行を手で消すと、枠は `scheduled` に戻る。`mail` のような控えは持たない: 無人で書く `sweep` が無く、
  戻った枠は `overdue` か `open` に出て、本人が裁き直せる。`plan` の行が消えて後続の行が残ると、`check` が
  「開いていない枠への …」を出す。
- 同じ日のノートに同じ `resched` の行は 2 度書けない(同じ日に A → B → A と組み直すと止まる。翌日なら書ける)。
- 切り替え日(`2026-10-30`)より前のノートは畳み込まない。読む範囲は `list` の上限(400 日)に収まる分まで。
- 並行する 2 機体が同じ枠に別の終端を書いて合流すると、時刻の早い方が勝ち、遅い方は `check` が食い違いに出す。

## 検査

Quint の仕様は無い(ADR の D5)。`crates/worklog/tests/slot.rs` が、語彙 JSON から読んだ遷移表の全マス、
終端から出る出来事が無いこと、畳み込みが行の読み込み順に依らないこと、再 `plan` の拒否、`overdue` の境界と
順序を確かめる。`crates/worklog/tests/cli.rs` が、CLI を通しで確かめる。
