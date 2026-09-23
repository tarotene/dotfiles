# ADR-0035 — 技術・仕組みの選択を 3 軸の辞書式順序に接地させる(selection-grounding)

- Status: Accepted
- Date: 2026-09-23
- Issue: No-Issue(`/grill-me` セッション中に発見・裁定)

## Context

ユーザーから「よくクリーンかつ先進的な技術選定をするというか、そういうのを
好む傾向にある」を言語化し、ルールとしてエンコードしたいという依頼があり、
自身の別の private リポジトリ(person-state hub、具体名は publish-guard の
denylist 方針により伏せる。以下「件の private リポジトリ」)を人格の
手がかりとして参照するよう指示された。

`/grill-me` セッションで dotfiles(このリポジトリ、32 本の ADR・19 本の
hook・21 本の skill を持つ)と件の private リポジトリ(7 本の ADR-lite、
CV/person-state の 2 ドメイン)の決定履歴を横断調査した結果、自己申告の
「クリーンかつ先進的」は表層で、実際の決定は次の 3 点でもっと強い構造を
持っていた。

1. **「先進的」は独立した基準ではなかった。** `apt→nix`(ADR-0001)・
   `bash→Rust`(ADR-0024)・`SOPS→GPG ホストローカルファイル`(ADR-0022)・
   `esa.io→Obsidian + Self-hosted LiveSync`(件の private リポジトリの
   ADR-0007)は、いずれも「宣言的・単一正本・型付き・pin 固定」で説明
   でき、新しさで説明する必要がない。逆に ADR-0024 は Deno(より新しく
   表現力も高い)を「closure 固定手法(deno2nix)が 2024-06-07 に
   アーカイブ済みで後継がない」という理由で明示的に棄却し、Rust を
   採った — 新しさが執行可能性に負けた実例。「先進性」は、他の軸が
   引き分けのときにだけ効く同順位破りである。
2. **「クリーン」は 2 つの異なる力に分解できた。** 構造面(不正な状態を
   検出でなく表現不可能にする)と、還元面(その仕組みは、より安い手段
   では担えない仕事をしているときだけ残す)。後者は前者に対するブレーキ
   として働く — これが無いと「執行可能性のため」を理由に機構が際限なく
   増える。件の private リポジトリの ADR-0006 は cadence 制約充足ソルバの
   自作を「エンジニアとしては魅力的だが…コア問題を再生産する皮肉な
   リスク」として却下し、同リポジトリの ADR-0007 は新しい `season:`
   フィールドを既存の `status:` と意味が重複するという理由で却下し、
   dotfiles ADR-0010 は消費者が 1 つも生き残っていない SOPS ランタイム
   復号チャネル全体を撤去し、dotfiles ADR-0022 は「実質 1 個の秘密の
   ための入れ物」になっていたリポジトリを丸ごと廃止した。いずれも
   「表現不可能性を上げる」側の理由が無いのに機構だけが残っていた状態
   への反応である。
3. **候補集合の誠実さが未機構化だった。** 件の private リポジトリの
   ADR-0007 ではユーザーの本命が Obsidian(「憧れ駆動」と自認)に
   決まっていたにもかかわらず、ユーザー自身が「HedgeDoc とかと一応
   戦わせてほしい」と手動で指示し、追認調査(本命の結論を後付けする
   調査)にならないよう本気の対抗馬を同じ評価軸で戦わせている。これは
   [ADR-0012](0012-precedent-grounding-over-prompted-adversarial-review.md)
   が指摘した教義(「敵対的に見よ」という都度の指示は、抽象的な標準
   指示にするのではなく検証可能な出力形式に機構化する)の未適用箇所
   であり、本 ADR の対象。

さらに本 ADR を書く直前に `origin/main` が 8 commit 進み、
[ADR-0033](0033-nav-doc-no-materialisation.md)(導線文書は実体を複製
しない)が確定した。これは本 ADR の軸 1 の duality「単一正本 > 複写+
同期」の直近の先行例であり、かつ「新しい判断知識は、既存の器(節・
gate・ドメイン)に載せられるなら新設しない」という設計判断そのものの
先行例でもある(Decision 6「新規ドメインを作らない」)。

## Decision

### D1: 軸は表現不可能性 → 還元性 → 先進性 の辞書式順序

1. **表現不可能性。** 判定質問:「この選択は不正な状態を*表現不可能*に
   するか、*検出可能*にするだけか」。採択側の duality(閉集合):
   宣言 > 手続き(ADR-0001)/ 単一正本 > 複写+同期(ADR-0032、
   ADR-0033)/ 閉語彙 > 自由記述+事後 lint(ADR-0020)/
   型・コンパイル時 > 実行時検査(ADR-0024)/ pin 固定 > 浮動
   (ADR-0029)/ upstream 正本 > fork・vendoring(ADR-0009、件の
   private リポジトリの ADR-0001 の「reuse pattern, not code」)。
2. **還元性。** 判定質問:「その仕組みは、より安い手段では担えない仕事を
   しているか」。死んだ機構・重複した機構は残すより消す
   (件の private リポジトリの ADR-0006/0007、dotfiles ADR-0010/0022)。
3. **先進性。** 1・2 が無差別なときの同順位破りだけ。採用した以上、
   追従した理由は pin 地点自体に残す — 実例(自作の別 public リポジトリ、
   TUI アプリの `Cargo.toml`):
   `rust-version = "1.88" # MSRV: raised from 1.85 (edition 2024 / resolver "3" lower bound) for ratatui 0.30; verified in CI.`

衝突時は必ず上位の軸が勝つ。新しさが軸 1 を弱めるなら、その候補は採らない
(ADR-0024 の Deno 棄却)。

### D2: 候補集合の誠実さを明示ラベルにする

本命(先に決まっていた選択)があるときは「憧れ駆動」と明示ラベルし、
同じ評価軸で本気の対抗馬を立てる(追認調査にしない)。感触で外した候補は
「感触で外した」と正直に書き、分析の結論であるかのように偽装しない
(件の private リポジトリの ADR-0007 の実例をそのまま機構化)。

### D3: 器は新設せず、既存の `## 先行例との対比` 節にトークンを追加する

新しい plan 節・新しい `ExitPlanMode` gate は作らない。ADR-0012 が確立
した節(`config/claude/skills/precedent-grounding/SKILL.md`)の各 `Dn`
行に `軸:`(必須、`表現不可能|還元|検出のみ` のいずれか)を追加し、
外部依存の新設・置換・撤去、または撤収コストが導入コストを上回る選択の
ときだけ `本命:`/`対抗馬:`/`外した候補:` の重い欄を追加で書く。
判定質問 1 つ + 閉じた duality 集合という作動形式は、このリポジトリの
既存の家風(ADR-0013/ADR-0017 の judging question + Accepted/Rejected 例、
件の private リポジトリの CONTRIBUTING.md の同型)をそのまま踏襲する。

新設するのは判断知識(`config/agents/AGENTS.md` の 1 節、
`selection-grounding` skill)と、既存 gate
(`config/claude/hooks/plan-precedent-gate.sh`)への加算的な検査項目
のみ。

### D4: 節名 `## 先行例との対比` は改名しない

`selection-grounding` の判断は必ずしも「先行例と対比」ではなく「複数の
軸で評価する」ことが主眼だが、改名は CLAUDE.md・skill・gate・selftest・
`docs/claude/precedent-grounding.md`・ADR-0012 の Amendment に波及し、
得られるのは名前の正確さだけである。件の private リポジトリの ADR-0004
D8 の "Known weakness, accepted... This paragraph is the record of that
trade-off, so it does not need re-litigating later." にならい、この
ずれを受容した弱点として明記するに留める。

### D5: 遡及的な移行義務は課さない

既存の非適合な選択(自作の PHP サイト、旧世代の JS ツールチェーン等)に
自発的な棚卸し義務を生まない。ADR-0007「No retroactive bulk rename」・
ADR-0020 の `createdAt` grandfathering と同じ扱いとし、新しい監査
ドメインも設けない。作業中に触れて気づいたものは wrap-up inbox → Issue
起票の既存経路に流す。

## Alternatives considered

- **独立した `## 技術選定` 節 + 専用 `plan-tech-gate.sh` を新設する。**
  棄却。`ExitPlanMode` gate が 4 本目になり、軸 2(還元性)に規範自身が
  違反する。ADR-0033 Decision 6 の「新規ドメインを作らない」と同じ理由。
- **先進性を第一軸にする。** 棄却。証拠(D1 の Context)は先進性が軸 1 の
  相関物にすぎないことを示しており、独立の第一基準として扱うと
  ADR-0024 の Deno 棄却を説明できない。
- **`## 先行例との対比` を改名する。** 棄却(D4)。波及コストに対して
  得られるものが名前の正確さだけ。
- **AGENTS.md / docs/README.md の ADR 索引に本 ADR を 1 行追記する。**
  棄却。ADR-0033 Decision 2 が「指す実体が変わるたびに書き換えが要る
  記述は導線文書に書かない」と定め、tarotene/dotfiles#372 がまさにこの
  重複 ADR 索引の削除を追跡している。自分のセッションで同じ領域を
  太らせない。

## Consequences

- `config/agents/AGENTS.md` に 1 節追加(判断知識、後続段)。
- `config/claude/CLAUDE.md` に形式検査の配線を 1 節追加(後続段)。
- `config/claude/skills/selection-grounding/`(新設)+
  `config/claude/skills/precedent-grounding/SKILL.md` §3 への
  `軸:` トークン追記(後続段)。
- `docs/claude/selection-grounding.md`(新設、living design rationale、
  後続段)。
- `config/claude/hooks/plan-precedent-gate.sh` への加算的検査項目 +
  `config/claude/hooks/copilot-plan-review.sh` の lens A 監査項目追加
  (後続段)。
- D4 で受容した弱点(節名の意味的ずれ)は、将来 `## 先行例との対比` 自体を
  改名する契機が別の理由で生じたときにまとめて解消する。個別には
  re-litigate しない。

## Verification

- `config/claude/hooks/plan-precedent-gate.sh --selftest`(後続段で拡張)。
- 本 ADR 自体は docs のみの変更のため `nix flake check` への影響はない
  (回帰確認として実行する)。
