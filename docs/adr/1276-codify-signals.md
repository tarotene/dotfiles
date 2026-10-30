# ADR-1276 — 振り返りで as-code 化の候補を閉じた語彙で記録し、採用・却下の実績で語彙を直す

- Status: Accepted
- Date: 2026-10-30
- Issue: No-Issue(セッションの終わりの振り返りで、その場で書いたコードや手順を「決定論化できないか」という目で見て Wrap-up Inbox に入れたい、という依頼から始まった)
- Amends: ADR-697(retro の行の語彙 `mechanism` を 5 段に引き直し、`signal`・`existing`・`searched` を足す)、ADR-543(Q2 の「安定の兆候」を、振り返りが記録する閉じた語彙にする)

## Context

retro の行には、次から防ぐ手段 `mechanism: prose | script | gate` を書く。ADR-543 は、散文で始めて安定の兆候が出たら決定論に昇格させる、
と定めた(Q2)。実装済みの昇格の検出は日次の `promotion-detect` だけで、見るのは「同じ `Target:` の feedback Issue の重なり」と
「skill のコードブロックと一致するコマンドの逐語反復」である。**セッションの中で、その場で書いたコードや手順を拾う経路は無い**。

しかも、記録の形が粗い。

- 段が 3 つで足りない。このリポジトリの過去の決定論化 30 件を洗うと、多くは `gate` より上位の手段(共有の採番を無くす ADR-380、
  閉じた状態機械と形式検査にする ADR-1259、写しをやめて単一の宣言にする ADR-872・ADR-503)か、`gate` と `prose` の間の
  「正しいやり方を 1 本の道具にする」だった。
- 分類が Inbox に渡るとき、`detail` の文の一部に溶ける(Inbox の項目は `ts`・`title`・`detail`・`repo` の 4 欄)。後から
  「どの兆候の候補が採用・却下されたか」を数えられず、何を決定論化すべきかの嗅覚を実績で磨けない。

外部の先行例も、同じ線を示している。Anthropic は、強調を足し続けても守られない行は hook に属し、好みでしかない hook は消す、と書く
("Best practices for Claude Code", https://code.claude.com/docs/en/best-practices、取得 2026-10-30)。労働安全の対策の優先順位
(除去 > 置換 > 工学的対策 > 管理的対策。CDC/NIOSH, "Hierarchy of Controls", https://www.cdc.gov/niosh/hierarchy-of-controls/about/index.html、
取得 2026-10-30)は、手段の順位付けに対応させられる(この対応付けは類推で、出典の主張ではない)。

## Decision

### D1: 手段を 5 段の閉語彙に引き直す

`mechanism` は `eliminate > unrepresentable > gate > tool > prose`(上位ほど優先)に、`existing:<名前>`(すでにある仕組みが漏らした)と
`none:<理由>`(決定論化しない)を足したもの。旧 `script` は `tool` と読み替える。書き込み時は `script` を拒否し(`tool` を使う)、
過去の台帳と vault のノートは書き換えず、表示・集計で `tool` と読む。

- 軸: 表現不可能 — 閉語彙 > 自由記述。順位は語彙ファイルに固定する。

### D2: 兆候(signal)を閉語彙にし、段を持つ行の必須欄にする

段を持つ行は、なぜ決定論化できそうだと思ったかを `signal` に書く。兆候ごとに既定の段を持ち、既定と違う段を選ぶなら `level_reason` を
必須にする(既定は「ここまで上を目指せ」という値で、届かなかった行は理由を書かせる)。初期の兆候は 15 個(`vocabulary.json`)。
決定論化に向かない側(意味の判断・観測できない・好み・一度きり)は、`none:<理由>` で明示する。

「どの兆候にも入らない」を明示する兆候 `unclassified`(既定 `prose`)を持つ。これで語彙の穴の割合を数え続けられる(D6)。

- 軸: 表現不可能 — 閉語彙。単一正本 `config/agents/codify-signals/vocabulary.json` から、retro の手順の文・行の検証・
  `docs/agents/codify-signals.md` の事例集の表を生成する。

### D3: 既存手段の探索を、段とは別の必須欄にする

`prose` 以外の段の行は `existing: 採用:<名前> | 拡張:<名前> | 自前` と `searched`(探した範囲)を必須にする。ADR-543 の `既存手段:` と
同じ 3 値。「既存に載せる」と「どの段か」は別の軸である(例: pr-gate に検査を 1 つ足すのは、既存への拡張で、段は gate)。
リポジトリ内(`pipeline-map` / `crates` / `skills`)は必ず探し、外部(`nixpkgs` / `upstream:<製品>`)は `自前` のときに 1 回探す。
`prose` の行は作る物が無いので、`existing`・`searched` は求めない(上位の段を選ばなかった理由は `level_reason` が残す)。

- 軸: 表現不可能 — 探索を飛ばした行を書けなくする。

### D4: 痕跡で retro を発火させる(段 4)

その場で書いたコードの痕跡(セッション専用の scratchpad のコード、形を持つコマンドのセッション内の反復、その場のスクリプトの形)が
しきい値を超えたら、PR が無いセッションでも retro を求める。しきい値は語彙ファイルに置く。閉じた後に新しい痕跡が出たら、
未処理の分だけ開き直す(1 セッション 2 回まで)。「作業の終わり」を機械で推測する案は #854 で却下済みなので、推測せず、開き直しで後続を拾う。

- 軸: 検出のみ — 未処理の痕跡イベントを検出して差し戻す。

### D5: 候補の構造を Inbox に渡し、却下の理由を閉語彙にする(段 5)

Inbox のノートの frontmatter に `signal`・`mechanism`・`existing` を載せる。`/backlog-cleanup` で却下するときは、閉じた理由
(`semantic-judgment` / `unobservable` / `preference` / `one-off` / `existing-suffices` / `misread-signal`)を必須にする。

- 軸: 表現不可能 — 却下理由を閉語彙にし、自由記述では数えられない実績を数えられる形にする。

### D6: 兆候ごとの採用・却下を日次に集計し、語彙の定義を直す候補を Inbox に戻す(段 6)

`promotion-detect` が兆候ごとの採用(fixed・issued)・却下(dropped)を数える。却下が続く兆候(`dropped ≥ 3` かつ却下率 ≥ 2/3、または
`misread-signal ≥ 2`)は「語彙の定義を直す候補」として、`unclassified` が `5` 件を超えたら「語彙に足す候補」として、Inbox に戻す。
兆候の `since` 以降のノートだけを数える(定義を直したら数え直す)。ADR-543 の ADR-1136 Amendment の「日次 timer で自動検出する仕組みは
持たない」は、Inbox の再発(同じ題の繰り返し)の検出の話で、これは別の対象(兆候ごとの却下率)である。

- 軸: 検出のみ — 集計と所見。しきい値は語彙の `calibration` に置き、直すのは語彙の変更として扱う。

### D7: 公開する事例は、このリポジトリの ADR・PR の番号だけにする

語彙の `examples` は `ADR-<数字>` か `#<数字>` だけで、ADR は実在を検査する(`crates/issue-inbox/tests/vocab.rs`)。他のリポジトリや業務の行は
件数にだけ使う(ADR-0034)。

- 軸: 表現不可能 — 形式をテストで固定し、外部の事例を書けなくする。

## Alternatives considered

- 語彙の読み込みを新しい crate `codify-vocab` に置く: 却下。`issue-inbox` は Inbox の項目の形をすでに持ち、`session-retrospective` も依存している。
  新しい単位を増やす理由が無い(還元性)。
- retro の `kind` に `codify` を足す: 段 4 で足す(痕跡のイベントを消し込む行の出所が、既存の kind と違うため)。
- 兆候を持たず、段だけを 5 段にする: 却下。後で「どの兆候から出た候補が採用・却下されたか」を数えられず、嗅覚を実績で直せない。
- 試し当ての付け直しを、続けて回すスクリプトとして残す: 却下。付け直しは LLM の判断で再現できない。続けて回すのは D6 の較正が担う。

## 根拠(back-test)

語彙を確定する前に、過去の retro の台帳(`~/.local/state/claude/wrapup/retro/*.jsonl` の 646 行)から、旧 `mechanism` が `prose`・`script`・`gate` の
204 行と、`none:` の行から固定の種(42)で抜いた 40 行、計 244 行を、サブエージェントが当時の 10 兆候と 5 段で付け直した(2026-10-30)。
抽出は次の 2 本(ledger の各行にリポジトリは無いので、`<session>.closed` の PR の URL か vault のノートの `repo` から引いた)。

```bash
jq -r 'select(.mechanism | test("^(prose|script|gate)$"))' ~/.local/state/claude/wrapup/retro/*.jsonl
jq -c 'select(.mechanism | startswith("none:"))' ~/.local/state/claude/wrapup/retro/*.jsonl | awk 'BEGIN{srand(42)} {print rand() "\t" $0}' | sort -n | head -40
```

結果(全リポジトリの件数。他のリポジトリの行は引用しない):

- 当時の 10 兆候のどれにも入らない行が 93 行(38%)。決定論化に向かない行(`NOCODIFY`)は別に 64 行(26%)。
- 穴の中身は、sandbox の宣言・除外・許可の不足 34 行、gate や道具の誤検知・拒否文の案内不足 21 行、新しい worktree の前提の欠け 7 行、
  CI でしか分からない失敗が手元で出ない 6 行で合計 68 行。残りは 5 行以下の塊 8 種類 25 行。
- 旧 `prose` で、決定論化できると判断された 88 行の 86%(76 行)は、付け直しでは `tool` 以上(`gate` 31、`tool` 30、`unrepresentable` 15、
  `eliminate` 1)になった。文章で済ませすぎていた傾向(付け直しは LLM の判断で、傾向の根拠にとどまる)。
- 既定の段がずれた兆候: `copy-drift` は 23 行中 15 行が既定の `unrepresentable` より下(上流が外にあり、宣言では防げず検査まで)、`shared-counter` は
  4 行中 3 行が `eliminate` ではなく `unrepresentable`。

この結果を受けて 2026-10-30 に本人が裁定した改訂: 兆候を 4 つ足す(`sandbox-env-gap`・`guard-misfire`・`local-ci-gap`・`worktree-bootstrap`)、
`unclassified` を足して穴の割合を数える、`copy-drift` は既定を変えず「上流が外にあるものは gate まで」を足す、`shared-counter` の既定は変えない。
5 行の塊(シェルの書き方の落とし穴・成果物の散らかり)は足さず、`unclassified` で数える。

## 執行点

この ADR を含む stacked PR の連鎖が、段ごとに執行点を足す(各段の PR がこの節に自分の段のパスを足す)。段 2 の時点:

- `config/agents/codify-signals/vocabulary.json`
- `crates/issue-inbox/src/codify.rs`
- `crates/session-retrospective/src/lib.rs`
- `crates/session-retrospective/tests/casebook.rs`
- `crates/issue-inbox/tests/vocab.rs`
- `flake.nix`

段 4(痕跡による発火・`codify` kind・開き直し):

- `crates/session-retrospective/src/traces.rs`
- `crates/wrapup-stop-gate/tests/retro_adhoc.rs`
- `pipeline-map.toml`

段 3(コマンドの形の記録):

- `crates/command-inspection/src/cmd_shape.rs`
- `crates/cmd-hash-log/src/lib.rs`

## Verification

- `cargo test -p issue-inbox -p session-retrospective -p wrapup-stop-gate`(語彙の読み込み・行の検証・手順書の生成・事例集の鮮度・公開する事例の形式)。
- `session-retrospective --retro-casebook` の出力が `docs/agents/codify-signals.md` の生成部と一致する。
- `nix flake check`(`flake.nix` が語彙 JSON を crane の入力に通す)。
