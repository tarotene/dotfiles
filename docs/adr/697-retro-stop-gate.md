# ADR-697 — 作業終了時のレトロスペクティブを Stop hook で強制する

- Status: Accepted
- Date: 2026-10-02
- Issue: No-Issue(`/grill-me` セッション中の依頼から始まった。「作業の終わりに強めの
  レトロスペクティブをフックで強制し、指摘・気付きを仕組み化の検討につなげたい。
  Wrap-up Inbox のスーパーセットにできないか」)

## Context

wrap-up inbox(`docs/claude/wrapup-inbox.md`)は「作業中にモデルが自発的に `--add` した
項目を、Stop 時に Issue 化させる」仕組みで、項目の中身は追記した時点で決まっている。
作業を終えた時点でセッション全体を見直させる機構は無く、ユーザーの指摘・詰まり・
hook に当たった事実は、モデルが気付いて `--add` したものしか残らない。

また inbox は `--check-dup` が重複を返すと行を黙って捨てていた。ADR-543 は「同じ規範違反が
繰り返し起きる」ことを散文 → script → gate への昇格の合図にしているのに、その再発回数が
どこにも残らなかった。

「スーパーセットにできるか」への答え: **出口(inbox → Issue 化 → wrapup-chores)は共有でき、
入口(終了時の網羅的な振り返り)と強制の仕方は別物として新設する。** inbox は項目を
追記した時点で判断が済んでいる。レトロは終わった時点で全体を見直すもので、gate が検査
できるのは「実施したか」「決定論的に分かる出来事を拾ったか」までで、「漏れなく振り返ったか」
自体は検査できない。

## Decision

- **D1 発火**: このセッションで PR を作成した後の最初の Stop で、1 セッション 1 回。
  PR 作成の判定は gh-edit-allow の台帳(`pr ` 行)、台帳が無い環境(Codex)では現ブランチの
  PR のうちセッション開始 stamp より後に作られたもの。PR の無いセッションは手動の
  `/retro` で補う。SessionEnd は block できないので使わない。
- **D2 記録**: `~/.local/state/claude/wrapup/retro/<session_id>.jsonl` に、閉じた語彙の行を
  `--retro-add` で書く。`kind`(`user-correction` / `insight` / `friction` / `gate-hit` /
  `none` / `skipped`)、`disposition`(`inbox` / `issue:#N` / `none:<理由>`)、
  `mechanism`(`prose` / `script` / `gate` / `existing:<名前>` / `none:<理由>`)。語彙外は
  exit 64 で書けない。「何も無かった」も `none` の 1 行で表し、黙った省略を表現不可能にする。
  `disposition: inbox` の行は同じ呼び出しで既存の inbox に追記する。
- **D3 網羅性**: 決定論的に分かる出来事(判定レッジャーの deny/ask、pr-gate の block、
  今セッションで更新した feedback memory)は、いずれかの行の `evidence` に id が引かれて
  いなければ完了にならない。ユーザーの発言は transcript から抽出して手順書に**並べるだけ**で、
  指摘かどうかはモデルが判断する(gate は判定しない)。
- **D4 仕組み化への接続**: 指摘・詰まり・gate 発火の行には `mechanism` を必須にする。
  inbox の重複は捨てず、既存 Issue に再発コメントを積む(コメント数 = 再発回数)。
- **D5 実装位置**: `crates/wrapup-stop-gate` の `retro` モジュール。Stop hook は 1 本のまま、
  同一プロセス内で「レトロ → inbox」の順に処理する(Claude Code は Stop hook を並列実行する
  ので、別 hook にすると順序が保証できず、block も二重になる)。
- **D6 可視化**: 全行を kind ごとの表にして、セッションの PR(stacked なら最上段)に 1 件
  コメントする。`--retro-close` が `gh api` でコメントを読み戻し、先頭の目印
  `<!-- wrapup-retro -->` を確かめてから完了にする。
- **D7 ループ防止と抜け道**: `stop_hook_active` では抜けず、セッション単位のカウンタ
  (上限 3)で数える。上限に達したら警告を 1 回だけ出して通す。省略できるのは
  ユーザーの発言を逐語で引用した `skipped` 行だけ。
- **D8 Codex**: 同じバイナリなので強制と突き合わせは両方に効く。transcript の形式が
  Claude と異なる(`transcript_path` が渡る保証が無い)ので、発言一覧の提示だけは Claude のみ。

## Alternatives considered

- **別の Stop hook(新 crate)にする**: 責務は分かれるが、並列実行で順序が保証できない。
- **pr-gate に同居させる**: 「PR 作成後」の条件は近いが、pr-gate の責務は PR 本文の検査で凝集が
  崩れる。感触で外した。
- **毎ターンの Stop で、ユーザーの指摘を検出したときに発火する**: 頻度が高すぎ、「作業の
  終わり」でもない。
- **transcript を解析して指摘を機械判定する**: ADR-478 D1 が棄却した理由(Claude 固有・
  非公開の形式)に当たる。ここでは判定ではなく抽出・提示のみに限った。
- **マーカーファイルだけで実施を検査する**: 中身を検査できず形骸化する。
- **PR 本文に「振り返り」節を足す**: PR 本文の 5 節スケルトンと pr-description スキルの変更を
  伴う。

## Consequences

- 既知の限界: `gate-events.jsonl` は session_id を持たないので、突き合わせの対象にできない。
  指摘の見落とし防止は、発言一覧の提示とモデルの判断に頼る(検出のみ)。
- 上限に達すると警告だけで通る。レトロを完了させるのはユーザーの GO であり、gate は
  放置を防ぐだけで強制し続けはしない。
- inbox の重複は、再発コメントを積む手順に変わる(`--check-dup` は重複時に Issue 番号を
  stdout に出す)。
- 先行例: Google SRE Workbook ch.10 "Postmortem Culture"
  (https://sre.google/workbook/postmortem-culture/、取得 2026-10-02)は action item に
  追跡番号を持たせ、繰り返しを傾向として見る。ただし事故の大きさで開く点と、LLM 自身の
  振り返りを決定論的な出来事と突き合わせる点は先行例が見つからなかった。

## 執行点

- `crates/wrapup-stop-gate/src/retro.rs`
- `crates/wrapup-stop-gate/tests/retro.rs`
- `config/claude/skills/retro/SKILL.md`

## Verification

- `cargo test -p wrapup-stop-gate` — 発火条件・語彙・突き合わせ・上限・読み戻しの検査。
- `nix build .#homeConfigurations.vega.activationPackage --no-link` — skill の配備が通る。
