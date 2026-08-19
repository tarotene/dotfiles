# プランレビューは critic を黙らせるのではなく、受入基準で閉じる

Claude Code の plan mode で書いたプランを、Approve 直前に Codex CLI が自動レビューする
仕組みの設計記録。実装は `config/claude/hooks/codex-plan-review.sh` と
`config/claude/hooks/codex-plan-review.schema.json`、デプロイは
`home/modules/claude.nix`。

この文書が説明するのは、**なぜ「Codex が APPROVE と言うまで直す」をやめたか**である。

## 初期実装は収束しなかった

最初の実装（#22）は、Codex に自由作文で `VERDICT: APPROVE` / `VERDICT: REQUEST_CHANGES`
を出させ、hook がその文字列を grep して deny する形だった。つまり critic が
acceptance criterion を兼ねていた。

3 日間 43 レビューの実測:

| 指標 | 実測 |
|---|---|
| `VERDICT: REQUEST_CHANGES` | 42 件 |
| `VERDICT: APPROVE` | 1 件（通過率 2.3%） |
| 2 パス走ったセッションの指摘件数 | 平均 4.0 → 3.7 件（減少 8 例 / 増加 5 例 / 横ばい 6 例） |
| 追跡セッション 22 件のうち上限 2 ラウンドに到達 | 22 件すべて |
| 出力フォーマット | `- [技術]` / `[技術] -` / `- 【高】` が混在 |

2 パス目で指摘が**増えた**セッションが 5 件ある（`3→6`、`0→7`、`5→6`）。
指摘件数は収束しておらず、**実質的な終端条件は「上限 2 ラウンドの打ち切り」だけ**
だった。品質ゲートとしては機能していない。

原因は「モデルが悪い」ではなく、要求の形にある。レビューは「正しいか？」ではなく
「問題を探せ」という open-ended search なので、観点を変えれば新しい指摘はいくらでも
生成できる。初期実装のプロンプトはそれを助長していた:

- レビュー観点に「より単純な代替案」を明示的に含めていた。同等な代替設計が defect と
  して上がる。
- ゼロ件が正当な出力であることを一言も書いていなかった。モデルから見れば
  「問題ありません」は価値の低い応答で、何か指摘を発明するインセンティブがある。
- severity 軸がなかった。`[技術]` / `[要判断]` は「誰が決めるか」の routing 軸で、
  「プランを変更するに足るか」の severity 軸ではない。全指摘が同列にブロックした。
- 各指摘に failure mode / 発生条件 / 根拠を要求していなかった。
- 毎ラウンド同一プロンプトだったので、失敗モードが相関して新規発見に寄与しなかった。

さらに、レビュー対象が「未来の実装についての命題」であるプランの場合、
「この設計で十分か」の評価には不確実性が残る。目的関数も単一スカラーではなく
(simplicity, generality, safety, maintainability, ...) の多目的で、Pareto front 上を
移動するだけの往復が起こりうる（abstraction を足したら次は過剰と言われる類）。
プランの列に全順序が存在しない場合、固定点は原理的に期待できない。

## 停止条件を差し替える

したがって狙うのは **critic convergence ではなく acceptance convergence** である。
「critic が何も言わなくなること」ではなく「実装をブロックする defect がゼロであること」
を停止条件にする。

そのために 3 つの役割を分離する。

```
critic : codex exec --output-schema  … findings と carry-over 判定を JSON で出すだけ
judge  : hook のシェル + jq          … gate 適格性を決定論的に判定し open set を更新
oracle : gate = open set が非空
```

`VERDICT:` 行は廃止した。critic は判定を出さない。verdict は judge が計算し、
レビュー log の 1 行目に `## GATE: PASS` / `## GATE: DENY` として書かれる。

**judge をシェルに置くのが要点**である。acceptance criterion を LLM の自由作文に
任せると、それは critic と同じ確率的写像になってしまう。judge を 2 パス目の
`codex exec` にする案も検討したが、280s × 2 は hook の timeout 300s に収まらず、
毎回 fail-open して実質ゲートが無効になる。

### severity で閉じる

| severity | 定義 | 扱い |
|---|---|---|
| `BLOCKER` | このまま実装すると壊れる / 要件を満たさない | deny |
| `MAJOR` | 実装前に決めないと手戻りが確実 | deny |
| `MINOR` | 直した方がよいが実装をブロックしない | backlog |
| `NIT` | 好み・体裁 | backlog |

deny メッセージには **gate 対象の指摘だけ**を注入する。MINOR/NIT は
`~/.claude/plan-reviews/backlog/<session_id>.md` に退避し、パスだけ伝える。
初期実装はレビュー全文を注入していたので、Claude が nit まで追いかけていた。
ここが往復を止める最大の要因である。

`kind` は severity と直交する routing 軸として維持している
（`TECHNICAL` は Claude が自律検証・反証してよい、`NEEDS_DECISION` は
AskUserQuestion でユーザーに聞く）。**反証できた指摘を、反証の根拠をプランに明記して
却下するのは正当な帰結**であり、deny メッセージにもそう書いてある。

### プランの受入基準は「完璧」ではなく implementation-ready

```
Ready(P) = R ∧ S ∧ I ∧ T
  R (requirements)    要件が特定されている
  S (scope)           スコープ / 非スコープが明確
  I (implementation)  実装手順が具体化されている
  T (verification)    検証方法が定義されている
```

critic はこの 4 boolean を `readiness` として出し、各 finding は自分がどの軸を壊すかを
`readiness_axis` で申告する。「もっと良いプランが存在しないこと」は要求しない。

ただし **`readiness` は deny チャンネルにしていない**。`false` があるのに gate 適格な
finding が 0 件のときは、critic 出力の不備として警告付きで素通しする（ラウンドは消費）。
readiness で deny できてしまうと、critic は `failure_mode` / `trigger` / `evidence` の
要求を回避して boolean を倒すだけで差し戻せる。severity インフレの穴が復活する。

### 観点を変える方が回数を増やすより効く

独立レビューがある重大 defect を確率 p で発見すると仮定すると、k 回とも見逃す確率は
`(1-p)^k`。p = 0.6 なら k=1 で 40%、k=2 で 16%、k=3 で 6.4% まで下がる。

ただし実際の LLM レビューは独立ではない。同じモデル・似たプロンプトなら失敗モードが
相関するので、この見積りは楽観的である。だから同じレビューを回数だけ増やすのではなく、
**観点を変える**。

```
ラウンド 1 … 並列 2 critic（wall-clock は 1 本と同じ 280s 枠に収まる）
  lens A: 要件 R / スコープ S / 前提の誤り
  lens B: 実装可能性 I / 検証戦略 T
  → findings を union（同一ラウンド内の重複だけ summary 正規化で dedup）
  → gate 適格なものに judge が id を振り、open set として永続化

ラウンド 2 … 単発 1 critic
  lens C: adversarial（実装した結果何が壊れるか）
          + open set を id 付きで渡し、各 id を
            RESOLVED / UNRESOLVED / REFUTED_BY_PLAN に判定させる
  → 新 open set = UNRESOLVED な carry-over ∪ 新規の gate 適格 finding
```

子プロセスは `-o` の一時ファイルにしか書かない。log / backlog / state の書き込みは
`wait` 後の親だけが行うので、競合が構造的に起きない。片方だけ失敗したら成功した側で
続行して警告を出す（ラウンドは消費）。両方失敗したら fail-open で、ラウンドは消費しない。

critic が成功したと見なすのは **`wait` の終了コードが 0 かつ `-o` が有効な JSON** の
ときだけである。codex が最終メッセージを書いた後にタイムアウトや後処理エラーで
非ゼロ終了した場合、出力ファイルは有効な JSON に見えるので、rc を捨てると不完全な
結果で gate を張ってしまう。`--selftest` にこの回帰テストがある。

### open set — 「既出だから除外」は穴だった

この設計の初版は「既出 summary はラウンド 2 で gate 対象外」としていた。
これは **未解決の BLOCKER がそのまま再掲された場合に「新規 0 件」と判定されて
素通りする穴**である（この設計プラン自身をこの hook にレビューさせたときに検出された）。

停止条件は「新規 blocker/major = 0」ではなく **`open set = ∅`** である。
既出指摘を open set から外すのは、critic が明示的に closed と判定したときだけ。

| status | 扱い |
|---|---|
| `RESOLVED` | open set から外す |
| `REFUTED_BY_PLAN` | open set から外す（反証が妥当だった） |
| `UNRESOLVED` | open set に残す = gate 対象 |
| 判定が返らない（id 欠落） | **UNRESOLVED 扱い**（保守側）+ 警告 |

判定を落とせば通過できる、という抜け道を作らないためにプロンプトにもそう書いてある。
ラウンド間の summary dedup は廃止した。dedup は同一ラウンド内の lens A / B の重複除去
だけに使う。

### 上限に到達しても未解消が残っていたら、人間に GO/NO-GO を取らせる

bounded passes（既定 2 ラウンド）は守る。だが未解消の BLOCKER/MAJOR を黙って飲み込むと
「実装をブロックする defect がゼロ」という停止条件が嘘になる。

```
count >= MAX かつ open set ≠ ∅ かつ state/<sid>.escalated が未作成
  → codex は呼ばない（トークン消費 0）
  → deny を 1 回だけ返す:
     「残存指摘は以下。AskUserQuestion で GO/NO-GO を取ってから再度 ExitPlanMode を呼べ」
  → touch state/<sid>.escalated
次の ExitPlanMode は escalated 済みなので必ず素通る（ループしない）
```

`systemMessage` ではなく deny を使うのは、**deny 理由は必ず Claude に届く**からである。
systemMessage は無視されうる。deny は最大 1 回なので bounded は保たれる。

hook は GO/NO-GO の**答え**を検証しない。`escalated` は「一度エスカレーションした」
という事実だけを記録し、次回は無条件に素通る。transcript を読んで AskUserQuestion が
実際に呼ばれたかを機械検査する案は、transcript JSONL のフォーマットに依存する脆い検査が
増えるので採らなかった。そもそも **hook は allow を返さない**ので、素通った後も
ユーザーの Approve ダイアログが最終ゲートとして残る。NO-GO は人間がそこで押し切れる。
（この点は本実装のレビューで MAJOR として指摘され、上記の根拠で却下した。
deny の文言側では GO/NO-GO それぞれの次の行動を明示するように直した。）

## 変わっていない原則

- **hook は allow を返さない。** deny か「何も決定しない（exit 0）」の二択で、
  ユーザーの Approve ダイアログは必ず残る。hook がプランを自動承認することはない。
- **fail-open。** codex 不在・タイムアウト・未ログイン・スキーマ不適合・パース不能は
  すべて警告付き素通り。codex が無いマシンでは黙って no-op（ADR-0005 のバイナリ
  存在ゲート）なので、全ホストに無条件でデプロイしている。
- **エスケープハッチ。** `touch ~/.claude/plan-reviews/skip` または
  `SKIP_PLAN_REVIEW=1`。
- **`--advisory` はゲートなし。** deny 経路に到達せず、ラウンドも消費しない。

## 運用

### 環境変数

| 変数 | 既定 | 意味 |
|---|---|---|
| `CODEX_BIN` | `codex` | バイナリ名。不在ならサイレント no-op |
| `MAX_PLAN_REVIEWS` | `2` | セッションあたりのレビューラウンド上限 |
| `CODEX_PLAN_REVIEW_TIMEOUT` | `280` | critic 1 本あたりの timeout（hook 側 300s より短く保つ） |
| `CODEX_PLAN_REVIEW_GATE_SEVERITIES` | `BLOCKER,MAJOR` | deny を張る severity |
| `CODEX_PLAN_REVIEW_PARALLEL` | `1` | `0` でラウンド 1 を単発 critic (lens M) に落とす |
| `CODEX_PLAN_REVIEW_RETENTION_DAYS` | `30` | log / json / backlog / state の保持期限。`0` で無効 |
| `CODEX_PLAN_REVIEW_DIR` | `~/.claude/plan-reviews` | 出力先（selftest が差し替える） |
| `CODEX_PLAN_REVIEW_SCHEMA` | スクリプトと同じディレクトリ | 出力スキーマのパス |
| `SKIP_PLAN_REVIEW` | — | `1` でスキップ |

### 生成物

```
~/.claude/plan-reviews/
├── <日時>-<sid8>.md          # judge がレンダリングしたレビュー（1 行目が ## GATE:）
├── <日時>-<sid8>.json        # judged + critic の生出力（フォレンジック用）
├── debug-last-input.json     # hook の stdin のメタデータのみ
├── skip                      # 存在すればスキップ（保持期限の掃除対象外）
├── backlog/<session_id>.md   # MINOR / NIT
└── state/
    ├── <session_id>.count       # 消費したラウンド数
    ├── <session_id>.open.json   # open set
    └── <session_id>.escalated   # 人間エスカレーションを 1 回に限定するフラグ
```

### ログ衛生

レビュー本文と生 JSON はプラン由来のテキストを含むので、`umask 077` で作成し、
ディレクトリを 700、ファイルを 600 に締める（既存ファイルも毎回 self-heal する）。
保持期限より古いものは hook 起動時に掃除する。`skip` は掃除対象外
——消すとエスケープハッチが黙って外れてゲートが復活してしまう。

`debug-last-input.json` はメタデータのみ（`session_id` / `cwd` / イベント種別 /
プランの文字数 / `planFilePath` の有無）。初期実装はプラン本文 11,887 文字と
`transcript_path` を含む 22 KB を平文で常駐させていた。

redaction は入れていない。プラン本文の任意テキストから秘密を機械判別する信頼できる
手段がなく、誤検出で証拠が壊れる方が有害である。代わりに
**プラン本文に秘密を書かないこと**を運用前提とし、権限と保持期限で残存面積を絞る。
SOPS 管理の秘密は runtime 復号なので（ADR-0003）ここには流れない。

### 自己検査

```bash
bash config/claude/hooks/codex-plan-review.sh --selftest
```

judge を fixture で回し、hook 経路を偽 codex で end-to-end に駆動する。
`CODEX_PLAN_REVIEW_DIR` を `mktemp -d` に差し替えるので実運用のログ・state・`skip` は
触らない。CI（`.github/workflows/ci.yml` の dry-run job）でも同じものが走る。

回帰テストとして特に重要なのは「carry-over `UNRESOLVED` + 新規 0 件 → deny」で、
これが上で述べた初版の穴に対応する。

### 効果測定

```bash
cd ~/.claude/plan-reviews
grep -c '^## GATE: PASS' *.md | grep -c ':1$'          # gate 通過したラウンド数
jq -r '.judged.open[].severity' *.json | sort | uniq -c
ls state/*.escalated 2>/dev/null | wc -l               # 人間エスカレーション発生数
```

ベースラインは APPROVE 1 / 43（2.3%）。人間エスカレーションが常態化するようなら、
severity の定義か `CODEX_PLAN_REVIEW_GATE_SEVERITIES` を見直す。

## 確定した技術事実

これを「修正」しないための記録。

- **`--search` は root 専用フラグ。** `codex --search exec ...` が正しく、
  `codex exec --search` は**パースエラーで即死する**（codex-cli 0.147.0 で実測）。
  hook はすべての出力を捨てているので、間違えても表面化しない。
- **`codex exec --output-schema <FILE>`** で最終メッセージの形を強制できる。
  OpenAI strict structured outputs の制約により、schema の全 object に
  `"additionalProperties": false` と全プロパティの `required` 列挙が必要で、
  `minLength` は使えない。だからフィールドの非空検査は judge 側が持つ。
- `codex exec` は非 TTY で stdin を待ち続けるので `</dev/null` が必須
  （openai/codex#20919）。
- ExitPlanMode の hook は cwd が `~` になるので、stdin JSON の `.cwd` へ明示 `cd` する
  （anthropics/claude-code#22343）。
- プラン本文は `tool_input.plan` → `tool_input.planFilePath` → `~/.claude/plans/` の
  最新 `.md` の順で取る。`planFilePath` を挟むのは、最新 mtime へのフォールバックが
  並行セッションで別プランを拾いうるため。
- `~/.claude/settings.json` は Claude Code 自身が実行時に書き換えるので store symlink に
  できない。hook 登録は activation 時の冪等な jq マージで行う（`home/modules/claude.nix`）。
