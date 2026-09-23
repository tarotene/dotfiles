# ADR-396 — 決定成果物とその執行点を同じ PR にコロケーションする

- Status: Accepted
- Date: 2026-09-23
- Issue: なし(グリルセッションから直接起票、No-Issue)

## Context

「ADR だけ残して実装を先送りする」判断を LLM ができないようにしたい、という
依頼から始まったグリルセッション(2026-09-23)の結論。

**外部の一次情報が主張していること。** Michael Nygard, "Documenting
Architecture Decisions"(2011-11-15、
<https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions.html>)
は Status の最初の値を `proposed` と定義し、「決定済みだが未実装」を
規範上の正規状態として持つ。PEP 1 も同様に明記する: 「the reference
implementation ... need not be completed before the PEP is accepted」
(<https://peps.python.org/pep-0001/>)。rust-lang/rfcs README
(<https://github.com/rust-lang/rfcs/blob/master/README.md>)は「accepted
RFC が active であることは実装の優先度について何も含意しない」「RFC の
著者は実装する義務を負わない」とまで書く。同一 PR での実装同梱を主張する
一次情報源は、この調査では 0 件だった。

**しかしこれは別の問題を扱っている。** 上記の文献が守る「decided,
not yet implemented」は、決定そのものが成果物である人間チームの分業を
前提にしている。本 ADR が塞ぎたいのは、**実装が成果物だったのに
エージェントが決定成果物を代用品として出して逃げる**動き — スコープ
縮小の一形態であり、文献の射程外にある失敗モードである。

**この repo の実測。** 履歴 squash 以降で追跡可能な ADR 27 本(ADR-0009〜
ADR-0035, ADR-380)のうち 12 本(ADR-0009/0010/0014-0016/0023/0024/0025/
0026/0027/0028/0031/0035)が docs-only(非 docs ファイルを一切含まない)な
PR で着地した。うち大半は「後続 Issue に段階分割する」という宣言を伴って
おり、10 本は実際に追跡 Issue が作られたが、2 本(ADR-0013 の「第2弾候補」、
ADR-0024 の Rust 移行、約40本15,000行)は追跡 Issue が今日まで一度も
作られていない。ADR-0024 では、欠落を指摘した Issue(#287)への応答が
「ADR に Amendment を追記する」で、存在しない移行 Issue への自己参照が
本文に残っている(0024-hook-cli-scripts-target-rust.md:118-128)。

**なぜ規範だけでは足りないか。** 先送りが書けてしまうのは、ADR が「実装への
参照」を持たなくても妥当な文書として成立するからである。`## Verification`
節は現在、先送りが書かれる場所になっている — 実例: ADR-0031「
`scripts/pr-title-check --selftest`(後続段で実装)」、ADR-0035「
plan-precedent-gate.sh --selftest(後続段で拡張)」。参照(検証コマンドの
予告)と執行(その決定を成立させる実体)は別の情報で、前者は後者の代用に
ならない。

**もっとも厳しい反例。** 本 ADR の Plan を書いている最中に origin/main が
進み、ADR-387(`wrapup-chores` を裁定前倒し型に反転する決定)が着地した。
導入 PR の diff は 6 ファイル全てが `.md` で、本文は「検査器は新設せず
`plan-scope-gate.sh` を再利用し」と明言している — 執行点は実在するが、
この PR では触れられていない。本 ADR はこの実例をあえて不合格側に倒す
(D5 参照)。

## Decision

### D1: 決定成果物に `## 執行点` 節を必須化し、導入 PR の diff と突合する

`docs/adr/**` の新規追加、`docs/claude/**` / `config/claude/skills/**`
の新規追加、または既存 ADR への `## Amendment` 見出し追加を「決定成果物の
追加」と見なし、これらをトリガに導入 PR の diff を機械検査する。

新規 ADR および新規 Amendment ブロックには `## 執行点`(Amendment 内では
`### 執行点`)節を必須化し、その決定を執行する実ファイルのパスを列挙する。
未来の Issue 番号はファイルパスではないので、そもそもこの節に書けない —
検出でなく表現不可能にする(共有 AGENTS.md の技術・仕組み選択の第一軸)。

### D2: コロケーション単位は 1 PR(導入 PR の diff)

Nygard / PEP 1 / rust-lang/rfcs が扱う「accepted だが未実装」は、決定自体が
成果物である人間チームの分業を前提にしている。本 ADR はその射程外にある
失敗モード(エージェントによる成果物のすり替え)を対象にするため、
文献の規範とは独立に、単位を 1 PR に固定する。

### D3: 執行点として認めるパスは「非 `.md` かつ `docs/` 配下でない」

パス分類台帳を持たず、この 2 つの述語だけで判定する。repo 内の既存 ADR を
検算した結果、決定成果物は例外なく `.md`、執行実体は例外なく非 `.md` だった
(`config/shell/profile` のような拡張子なしファイルも非 `.md` として扱われる
ため取りこぼさない)。新しい文書置き場が増えても、台帳の同期漏れという
不正状態そのものが発生しない。

対抗馬として検討したパスパターン台帳(ADR-0020 の閉語彙 `*.tsv` 型)は、
意図が読める利点はあるが、新しい置き場が増えるたびに同期が必要で、漏れると
黙って fail-open するため不採用。実行可能ビット/拡張子白名簿も
`config/shell/profile`・`patches/*.patch` のような拡張子なし・外れ値が
落ちることを実例で確認して棄却した。

### D4: 検査は 2 層(client guard + CI required check)に置く

`config/claude/hooks/decision-colocation-guard.sh`(`gh pr create` の
PreToolUse deny)と `.github/workflows/ci.yml` の required check の 2 層。
第三層(`github-audit` ドメイン)は置かない — 本件の判定はこの repo 固有の
パス述語に依存し、横断適用できないため。`pr-gate.sh` には新判定を足さない
— 既存の `G_CI` 判定が CI 失敗を Stop ブロックに変換するので、新しい `G_*`
を足す仕事が無い(還元性)。

### D5: 「既存機構の再利用」は執行点として認めない

執行点に列挙したパスが実在するだけでは不十分で、そのうち少なくとも 1 つが
**この PR の diff に含まれる**(新規または変更)ことを要求する。ADR-387
(Context 参照)を意図的に不合格として扱う — 「既存の何かが執行する」という
主張は、その再利用が成立することを示す変更(selftest への新規ケース追加
など)を同じ PR に伴わない限り、検証を伴わない自己申告と区別できない。
これを許すと、ADR-0024 が `## 執行点: config/claude/hooks/
stack-base-guard.sh`(実在するが無変更)と書くだけで docs-only のまま
抜けられてしまう。

### D6: 執行点が 1 つ以上あれば合格とし、残余は無約制

「決定の完全実装」は ADR 本文から対象範囲を導けず機械判定不能であり、
判定不能な要求は自己申告に堕ちる。執行点が 1 つでもあれば合格とし、
残りの後続 Issue 化は自由にする(walking skeleton / tracer bullet の型 —
Alistair Cockburn, *Crystal Clear*, 2004; Hunt & Thomas, *The Pragmatic
Programmer*, 1999)。ADR-0024 で言えば、Rust 移植を 1 本だけ同梱すれば
合格し、残り 39 本の移植は依然として後続 Issue に出せる。

### D7: `AskUserQuestion` の選択肢空間は gate せず規範で扱う

「実装を後続 Issue に分離する」という選択肢を `AskUserQuestion` に提示しない
ことを `config/agents/AGENTS.md` に明記する(段2)。この repo には既に
「`AskUserQuestion` の選択肢空間は機械検査に向かない」という裁定
(`docs/claude/stacked-pr.md:57-61`)があり、それに従う。CI が当該選択肢を
実行不能にした以上、語彙 lint を足しても新しい仕事が無い。

### D8: 既存 ADR への遡及適用はしない

トリガが「新規追加」と「Amendment 追加」に限られるため、既存 36 本(387 を
含む)は構造的に対象外になる(ADR-0007 の「既存ファイルの一括リネームは
しない」、ADR-0020 の `createdAt` grandfathering、ADR-380 の「`n < 0100`
は grandfathered」と同じ扱い)。

## Alternatives considered

- **ADR ファイルの変更全てをトリガにする**: typo 修正・リンク切れ修繕・
  supersede 表記更新のような純正 docs 修繕まで毎回 `## 執行点` を要求する
  ことになり、過剰。D1 でトリガを「新規追加」「Amendment 追加」の 2 つに
  絞った。
- **執行点のパスに種別台帳(`gate:`/`code:`/`config:`/`doc:`)を持たせ、
  `doc:` だけのときは `Unenforced: <理由>` を必須にする**: 種別が LLM の
  自己申告になり、`Unenforced:` タグを乱用すれば同じ抜け道が復活する。
  D3 の述語ベース判定を採用し、種別台帳は持たない。
- **執行点の実在パスを無条件で合格にする(diff 内である必要はない)**:
  ADR-387 という実例で不合格側に倒すべきと判断し、D5 で棄却。
- **AskUserQuestion に PreToolUse guard を新設し、選択肢の label/
  description を語彙マッチで deny する**: CI が既に当該選択肢を実行不能に
  しており新しい仕事が無いこと、自由記述への語彙マッチが言い換えとの
  いたちごっこになること(`docs/claude/scope-inventory.md:113-116` が
  同じ理由で `Conflicts:` タグを棄却した先例)から不採用。D7 で規範のみに
  留めた。

## Consequences

- `scripts/decision-colocation-check`(判定エンジン単一ソース、拡張子なし)。
- `.github/workflows/ci.yml` の `dry-run` job に required check ステップ
  2 つ(`--selftest` と `--base` 判定)。
- `config/claude/hooks/decision-colocation-guard.sh`(client guard、段2)。
- `home/modules/claude.nix` への hook 登録(段2)。
- `config/agents/AGENTS.md` への規範追加(段2)。
- この repo の直近の成功パターン(ADR-0031/ADR-0035 の「ADR 段 → 次段で
  実装」)は、単位が 1 PR に確定したため今後は不適合になる。既存分は
  遡及適用しない(D8)。
- Codex / Copilot 向け adapter は作らない — CI required check が全エージェ
  ント共通の backstop として機能するため、client 側は Claude Code のみで
  足りる。
- ExitPlanMode gate は作らない — 計画本文のパースは本質的に曖昧で、CI が
  既に表現不可能性を担保している。

## 執行点

- scripts/decision-colocation-check
- .github/workflows/ci.yml

## Verification

- `scripts/decision-colocation-check --selftest`(ネットワーク不使用、
  トリガ1a/1b/トリガ2/縮退を網羅)。
- `config/claude/hooks/decision-colocation-guard.sh --selftest`(段2)。
- 本 ADR 自身の導入 PR で CI の `decision-colocation-check` required check
  が green になることを自己適用の実測として確認する。
