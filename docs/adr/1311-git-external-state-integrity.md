# ADR-1311 — Git の外に置いた状態の整合を、日次の timer で読む

- Status: Accepted
- Date: 2026-10-31
- Issue: No-Issue(記録を変化の速さで層に分け、日〜都度で入れ替わる状態を Git から vault へ移す計画の机上の整理で決めた。計画は非公開のリポジトリにある)
- Amends: ADR-543(日次の `promotion-detect` に、昇格・降格の候補に加えて、Git の外の状態の整合の検査を載せる)

## Context

状態遷移を形式で守るとき、守りは 2 か所に分かれる。

- Git に置いた状態は、PR の差分で「語彙に許された遷移だけか」を検査できる(状態遷移を持つ個人用のリポジトリの検査)。
- Git の外(vault)に置いた状態は、PR の差分が無いので、その検査が効かない。

先行例は `worklog` のメール裁定の状態機械(ADR-1259)。語彙と Quint のモデルは CI で検査され、書き込み口 `worklog add mail` は語彙に無い遷移を拒む
(`crates/worklog/src/mail.rs` の `add_mail`)。しかし、**実データ(vault の当日ノートの `[mail]` 行)の整合を全件読む `worklog mail check` を、
呼ぶものが無かった**(2026-10-31 に、このリポジトリを `mail check` で grep して確かめた。呼び出し元は無い)。vault は Obsidian の画面から手で直せる
ので、書き込み口の検査を通らない編集が起きうる。

上の計画は、状態を Git から vault へ移すもので、移す対象が増えるほどこの穴が増える。CI は vault を読めない(クラウドから vault を読む検証は未了)。

## Decision

### D1: vault を宣言している機体で、毎日 `worklog mail check` を回し、結果を wrap-up inbox に流す

日次で動いている `promotion-detect`(ADR-543)に載せる。終了コード 1(食い違いあり)は「食い違いが N 件」の候補、終了コード 2 以上・道具が無い・
食い違いの行を読めないときは「整合を読めませんでした」の候補にする。検査が回っていないことを、黙って飛ばさない。
vault を宣言していない機体では何もしない。同じ題・同じ本文の候補は翌日に入れ直さない(既存の `emitted.json`)。

- 軸: 検出のみ — 書き込み口で拒めない手の編集を、事後に検出して人に渡す。

### D2: Git の外へ移す状態は、同じ形の check を移す PR に同梱して、ここに足す

状態を Git から出す変更は、移した先の書き込み口の検査と、この日次の check への載せ替えを同梱する。

## Consequences

- 画面での手の編集による食い違いは、翌日までに wrap-up inbox に出る。リアルタイムでは止められない(検出のみ)。
- `promotion-detect` は「昇格・降格の候補」に加えて、整合の検査も持つ。名前と用途がずれるが、timer と出口(wrap-up inbox)を共有するほうが、
  同じ形の timer をもう 1 本足すより小さい(dotfiles#1310 が層のずれの検出を同じ timer に載せる予定と同じ向き)。
- クラウドの定期実行は vault を読めないので、この検査は vault のある機体(vega など)だけで効く。

## 執行点

- `crates/promotion-detect/src/main.rs`
- `crates/promotion-detect/src/lib.rs`
- `crates/promotion-detect/tests/cli.rs`
- `docs/agents/promotion-detect.md`

## 先行例との対比

- D1: 事後の整合検査を、既存の日次 timer に載せる。
  先行例: tarotene/dotfiles `home/modules/promotion-detect.nix`(日次 `OnCalendar = "daily"` の user timer で、出口は wrap-up inbox) (取得 2026-10-31)
  差分: 異なる — 既存は昇格・降格の候補の検出器。整合の検査を載せるのは用途の拡張
- D1(書き込み口で拒む型): 語彙に無い遷移を書き込み口が拒む。
  先行例: tarotene/dotfiles `crates/worklog/src/mail.rs` の `add_mail`(ADR-1259) (取得 2026-10-31)
  差分: 一致 — 書き込み口の検査はそのまま残し、通らない編集を事後に拾う検査を足す
