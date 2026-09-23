# ADR-0000 — wrapup-chores を裁定前倒し型に反転する(adjudication-first)

- Status: Accepted
- Date: 2026-09-23
- Issue: No-Issue(ユーザー依頼の grill-me セッションで確定)
- Supersedes: `config/claude/skills/wrapup-chores/SKILL.md` の 2026-09-21 版
  (「判断を要さないものだけを対処し、要判断は一切手を付けず放置する」設計)
  を全面的に置き換える。`docs/claude/github-audit-triage.md` L76-79 の
  「`wrapup-chores.md` と同じ型を踏襲しているため確認の往復は増えない」という
  相互参照の前提を部分的に更新する(本 ADR の射程外、参照修正のみ)。

## Context

`wrapup-chores` スキルは、wrap-up inbox の未起票項目と起票済み wrapup 由来
open Issue をまとめて 1 つの chores PR で一括対処する器として導入された
(`docs/claude/wrapup-chores.md`)。運用実績を踏まえたユーザーからのグリル
(2026-09-23)で、次の実害が特定された:

1. **母集団から落ちる件数が多い**: open Issue 28 件のうち、実際に chores PR の
   対象になったのは数件だけだった。
2. **裁定の機会が一度も来ない**: ユーザーが triage 結果に返せるのは
   `GO / 一部除外して GO / 中止` の 3 択のみで、個々の項目に判断を下す場面が
   存在しなかった。

原因は 2026-09-21 版 SKILL.md §2 の設計そのものにあった:

- 「即対処」の判定条件は 5 つの**連言**(すべて満たす場合のみ)で、条件の 1 つに
  「ユーザーに聞きたくなった時点で要判断」が含まれていた。つまり設計そのものが
  「ユーザーに裁定を委ねる」ことを明示的に避け、判断が要る項目を無条件で
  「要判断」に回していた。
- 「要判断」に振り分けた項目には**一切手を付けない**(コメントすら付けない)。
  inbox 行も Issue もそのまま残るため、次回の棚卸しでも同じ理由で同じ場所に
  戻ってくるだけだった。
- 除外理由は自由文だったため、「規模が大きい」「設計判断が要る」のような、
  `scope-inventory` スキルが本来禁じている棄却理由を、agent が実質的に
  いくらでも書けた。

3 つの経路に共通するのは、**いずれも「ユーザーに聞く」を一度も経由しない**
ことだった。ユーザーの意図は逆で、「ユーザーに積極的に裁定を委ねてでも多くの
Issue を刈り取る」設計への転換を求めていた。

## Decision

### D1: 確認の総数ではなく位置を変える

triage フェーズ(SKILL.md §2〜§4)に確認を全部前倒しし、`AskUserQuestion` で
その場に決着させる。決着した triage 結果を Plan に書き、`ExitPlanMode` が GO を
兼ねる。**`ExitPlanMode` 以降は項目ごとの確認を一切挟まない** — 止まらない
という 2026-09-21 版の美点はそのまま残し、止まる位置だけを実行フェーズから
triage フェーズへ移す。

### D2: 除外は閉じたタグ 3 種のみ。規模・工数は棄却理由にならない

除外の語彙を `scope-inventory` スキルの閉じたタグ(`Blocked-Upstream:` /
`Obsolete:` / `User-Excluded:`)に固定する。`Blocked-Upstream:` は
**リポジトリ外**の事情に限り、同一母集団内の Issue 依存(先行 Issue 待ち)は
このタグの対象にしない — 先行を今回刈れば後続も着手可能になるため、後続は
裁定(AskUserQuestion)に載せる。規模・工数・セッション長・コンテキスト残量を
理由にした除外は誤用であり、書いてはいけない。

### D3: 検査器を新設せず `plan-scope-gate.sh` を再利用する

triage 結果を `## 要求インベントリ` 形式で Plan に書き、既存
`config/claude/hooks/plan-scope-gate.sh` に節内整合性(全項目が段または
閉じたタグを 1 つ持つこと)を検査させる。この hook は本来「依頼の要求項目」を
入れる器だが、母集団を「open Issue 全件 + inbox 行」に広げても検査ロジック
(閉語彙・1 項目 1 処分)は変わらないため、そのまま再利用する。hook 自体は
無改造。

### D4: 大物も裁定対象にし、選ばれたものは ADR-0027 の段として受ける

chores PR に物理的に入らない規模の項目(設計構想・大型機能)も、triage で
黙って除外一覧に流すのではなく `AskUserQuestion`(multiSelect)でユーザーに
「今セッションで着手するか」を問う。選ばれたものは ADR-0027 の単一チェーンの
段として受ける。出力は 1 つの chores PR ではなく stacked PR になる — 分割の軸は
「レビューが 1 本で成立するか」。選ばれなかったものは `User-Excluded:` になる。

### D5: PR で閉じない項目は「宣言化して正本に取り込む案」を第一候補にする

ホストローカルファイル・GitHub 設定のように PR そのものでは閉じられない項目は、
§3 の起草フェーズで「宣言化して正本(home-manager / private wrapper flake)に
取り込む案」を必ず 1 案含めさせる(ADR-0001, ADR-0034)。宣言化できないもの
(実値を含む private wrapper flake 行きのもの等)は、その旨を選択肢として
ユーザーに提示し、無裁定では除外しない。

## Alternatives considered

- **現状維持(2026-09-21 版のまま)**: 「聞きたくなったら要判断」という
  設計そのものがユーザーの意図(積極的に裁定を委ねる)と正反対であり、
  今回のグリルが直接それを否定した。棄却。
- **triage 専用検査器の新設**(`wrapup-triage-check.sh` 等、独自語彙
  `Blocked-Dependency: #N` / `Tracking-Parent:` を持たせる): 語彙の精度は
  上がるが、閉語彙の正本が 2 つに割れる。還元性の軸で、既存
  `plan-scope-gate.sh` が同じ仕事(全項目に処分があることの機械検査)を
  既に担っているため、対抗馬として検討したが採らない(D3)。
- **裁定を会話上の表 + 自由回答で行う**(Plan mode を経由しない): 往復は
  最小になるが、日和りを拘束する機械検査(`plan-scope-gate.sh` /
  `plan-precedent-gate.sh`)が一切効かず、2026-09-21 版と同じ「自己申告の
  規律だけに頼る」構造に戻る。棄却。

## Consequences

- `config/claude/skills/wrapup-chores/SKILL.md` を全面改訂する(9 節構成)。
- `docs/claude/wrapup-chores.md` に日和りの構造・検査器を新設しない理由・
  stacked PR への出力変更を追記する。
- `docs/claude/github-audit-triage.md` の相互参照 1 行(L76-79)が陳腐化するため
  修正する。`github-audit-triage` 自体の型は変えない(本 ADR の射程外)。
- `github-audit-triage` にも同じ裁定フェーズを入れるべきかは未検証のため、
  wrap-up inbox に検討項目として追記し、この ADR のスコープには含めない。
- 射程は wrapup-chores 1 スキルに限る。ADR-0015 Decision 1(github-audit-triage
  の判断ループ)は amend しない。

## Verification

- `config/claude/hooks/plan-scope-gate.sh --selftest` と
  `config/claude/hooks/plan-precedent-gate.sh --selftest`(hook 無改造の回帰確認)。
- 現在の open Issue 全件を `R1..Rn` の要求インベントリとして書いたサンプルで
  `plan-scope-gate.sh --check-plan` が節内整合性を通過することを実地確認する
  (D3 の再利用が成立する根拠)。
- `config/claude/hooks/wrapup-stop-gate.sh --selftest`(inbox 契約は無改造)。
- 次回の `/wrapup-chores` 実行(ドッグフーディング)で、粗振り分けが母集団を
  L1〜L4 に配り、裁定(AskUserQuestion)が実際に発火し、要求インベントリが
  母集団全件をちょうど 1 回ずつ含むことを確認する。
