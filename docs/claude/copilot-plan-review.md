# プランレビューは critic を黙らせるのではなく、受入基準で閉じる

Claude Code の plan mode で書いたプランを、Approve 直前に GitHub Copilot CLI が
自動レビューする仕組みの設計記録。実装は `config/claude/hooks/copilot-plan-review.sh` と
`config/claude/assets/copilot-plan-review.schema.json`、read-only custom agent は
`config/copilot/agents/plan-reviewer.agent.md`、デプロイは `home/modules/claude.nix`。

> **註（履歴と現行の区別）**: 以下の「初期実装は収束しなかった」「第二次の非収束」
> 「停止条件を差し替える」の各節が語る設計動機と実測データは、**Codex CLI を critic に
> 使っていた初期実装(#22, #23)**によるものである。critic / judge / oracle の分離、
> severity によるゲート、closer ラウンド、carry-over 判定といった不変条件はそのまま
> 現行実装に引き継いでいるが、**critic の実行エンジンは GitHub Copilot CLI に置き換わった
> （#80。Closes #56, #58）**。Codex 固有の技術事実（`--search` のフラグ位置、
> `--output-schema` の制約等）は「確定した技術事実」節の末尾に**過去の事実として**
> 残してあるが、現在のスクリプトはそれらを使っていない。現行の運用（環境変数・生成物・
> 自己検査・Copilot 固有の技術事実）は「Copilot への移行」節を参照すること。

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

## 第二次の非収束: closer なき最終ラウンド

停止条件を `open set = ∅` に差し替えた後（#23）も、gate は収束しなかった。
現行ログフォーマットの全 9 セッションの実測:

| 指標 | 実測 |
|---|---|
| ラウンド 1 の GATE | 9/9 DENY |
| ラウンド 2 の GATE | 9/9 DENY |
| `## GATE: PASS` | **0 件** |
| 人間エスカレーション発火 | 9/9（100%） |
| `state/*.count` が上限未満で終わったセッション | 0/32 |
| 残存 21 finding の出自 | ラウンド 2 で新規に生えた `R2-C-*` が **18 件** / R1 の carry-over が 3 件 |
| 残存 21 件の severity | MAJOR 20 / BLOCKER 1 |
| carry-over 判定 | closed 22（RESOLVED 21 / REFUTED_BY_PLAN 1）対 UNRESOLVED 5 = **閉率 81%** |
| carry-over 未応答・不適合破棄・重複除去 | 全ラウンドで 0 |

critic の出力品質は問題ではない。carry-over の閉率は 81% で、critic はプランの修正を
ちゃんと認めている。壊れていたのは**不変条件**である。

```
gate 適格な finding は、修正後の再判定を最低 1 回受ける
```

ラウンド 1 の指摘はラウンド 2 の carry-over 判定が見るので、この機会を得る。だが
ラウンド 2 は **最終ラウンドかつ adversarial な発見観点 (lens C)** だった。そこで
生えた指摘を判定するラウンドが存在しないので、`open set = ∅` は原理的に到達不能に
なる。実測で lens C は 8/9 のラウンドで新規 gate 適格 finding を 1〜3 件生やしていた。

帰結として、エスカレーションのメッセージは必ず「最後の修正を反映していない古い判定」を
貼り出す。Claude 側は「その指摘の根拠行はもう存在しない」と反論し、噛み合わないまま
人間に GO/NO-GO を求める出力が定常状態になっていた。**機構は入れ替わったが、実質的な
終端条件が「打ち切り」であることは初期実装から変わっていなかった。**

対策は上限を増やすことではなく、**最終ラウンドの職責を変えること**である
（後述の lens Z = closer）。回数を増やしても、最後のラウンドが発見観点である限り
同じ穴が一段深いところで再発するだけである。

## 停止条件を差し替える

したがって狙うのは **critic convergence ではなく acceptance convergence** である。
「critic が何も言わなくなること」ではなく「実装をブロックする defect がゼロであること」
を停止条件にする。

そのために 3 つの役割を分離する（この分離自体は現行の Copilot 実装でも維持している。
critic の呼び出し方だけが後述の「Copilot への移行」節の内容に置き換わった）。

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

ラウンド 3 (= MAX_PLAN_REVIEWS) … 単発 1 critic
  lens Z: closer。carry-over 判定が唯一の職責。新規欠陥は探させない
  → 新規 finding は severity に関わらず gate 対象外にして backlog へ退避
  → 新 open set = UNRESOLVED な carry-over のみ（新規流入ゼロ = 単調非増加）
```

closer は `judge` に gate 適格 severity として**空文字**を渡すことで実現している。
jq 側の `$gates` が空になれば全 finding が backlog に落ちるので、判定ロジックに
「closer モード」の分岐を持ち込む必要がない。ただし `judge()` の既定値展開は
`${3-...}`（コロンなし）でなければならない。`${3:-...}` は空文字も既定値
`BLOCKER,MAJOR` に置換してしまい、closer が新規流入を止められなくなる
（この穴は本改訂のプランをこの hook にレビューさせたときに BLOCKER として検出された）。

closer は最後の言葉なので、そこで carry-over が残ったら通常 deny（「直して再度
ExitPlanMode を呼べ」）には落とさず、直接エスカレーションに行く。通常 deny を返すと
その修正を判定するラウンドがまた無くなり、同じ非収束が一段深いところで再発する。

`MAX_PLAN_REVIEWS=1` に絞った退化ケースではラウンド 1 を closer 化しない。発見
ラウンドが 1 本も無くなり、carry-over 判定だけの空回りになるからである。

子プロセスは自身の標準出力を一時ファイルにリダイレクトされるだけで、log / backlog /
state には一切書かない。書き込みは `wait` 後の親だけが行うので、競合が構造的に起きない。
片方だけ失敗したら成功した側で続行して警告を出す（ラウンドは消費）。両方失敗したら
fail-open で、ラウンドは消費しない。

critic が成功したと見なすのは **`wait` の終了コードが 0 かつ出力先に有効な JSON**
（かつ後述の jq schema validator を通過）のときだけである。critic プロセスが最終
メッセージを書いた後にタイムアウトや後処理エラーで非ゼロ終了した場合、出力ファイルは
有効な JSON に見えるので、rc を捨てると不完全な結果で gate を張ってしまう。
`--selftest` にこの回帰テストがある。

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

### closer 後も未解消が残っていたら、人間に GO/NO-GO を取らせる

bounded passes（既定 3 ラウンド）は守る。だが未解消の BLOCKER/MAJOR を黙って飲み込むと
「実装をブロックする defect がゼロ」という停止条件が嘘になる。

```
closer ラウンド (round == MAX) の judge 結果が gate=true
  → 通常 deny には落とさない
  → deny を 1 回だけ返す:
     「残存指摘は以下。AskUserQuestion で GO/NO-GO を取ってから再度 ExitPlanMode を呼べ」
  → touch state/<sid>.escalated
次の ExitPlanMode は escalated 済みなので必ず素通る（ループしない）
```

`escalated` のチェックは上限判定より**前**に置く。エスカレーションは closer の直後に
起きるので、`count >= MAX` を待たずに立つフラグである。上限判定側の同じ分岐は、
`MAX_PLAN_REVIEWS` を途中で下げた等で古い state が上限を超えている場合の安全網として
残してある（この経路だけは copilot を呼ばない = premium request 消費 0）。

貼り出す open set が **closer が現行リビジョンに対して下した判定**であることが、
この設計の要点である。旧実装では最後の修正を反映していない判定を貼っていたので、
Claude は「もう直した」と反論するしかなく、人間は判断材料を得られなかった。

closer が新たに報告した BLOCKER/MAJOR は gate 対象外だが、黙って捨てない。PASS 時の
`systemMessage` に件数と BLOCKER 数を載せ、backlog のパスを示す。**hook は allow を
返さない**ので、最終ゲートは人間の Approve ダイアログである。

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
- **fail-open。** copilot 不在・タイムアウト・未ログイン・モデル利用不可・
  スキーマ不適合・パース不能はすべて警告付き素通り。copilot が無いマシンでは黙って
  no-op（ADR-0005 のバイナリ存在ゲート）なので、全ホストに無条件でデプロイしている。
- **エスケープハッチ。** `touch ~/.claude/plan-reviews/skip` または
  `SKIP_PLAN_REVIEW=1`。
- **`--advisory` はゲートなし。** deny 経路に到達せず、ラウンドも消費しない。

## Copilot への移行

Codex CLI から GitHub Copilot CLI への移行(#80)で変わったのは **critic の呼び出し方と
出力検証だけ**である。judge / oracle / severity ゲート / closer / carry-over / state
の各不変条件は上記の設計のまま維持している。

### なぜ移行したか

Codex CLI 側の事情ではなく、モデル選択の主導権を取り戻すためである。GitHub Copilot CLI
は `--model` で具体的な GPT モデル ID を明示指定でき、`--agent` で read-only の
custom agent を強制できる。critic に要求しているのは「決定論的な judge が食える
JSON を、read-only の探索から返す」ことだけなので、Codex 固有の機能（sandbox mode や
`--output-schema`）に依存する理由はない。

### read-only custom agent

`config/copilot/agents/plan-reviewer.agent.md`(`~/.copilot/agents/` に配備)が
critic の権限境界そのものである。フロントマターの `tools` を
`["view", "grep", "glob"]` に絞り、write / execute / web / GitHub MCP を
一切含めない。実機で `tools` を絞った custom agent にファイル作成・シェル実行を
指示すると、モデル自身が「このセッションには書き込み/実行ツールが無い」と応答して
拒否することを確認済み(1.0.82 実測)。`--allow-all-tools` / `--yolo` /
`--allow-all-paths` は critic に変更権限や無制限ファイルアクセスを与えるため
使わない。plan file がリポジトリ外にある場合は、その直接の親ディレクトリだけを
`--add-dir` で追加読み取り許可する(cwd 自体は既定で読める)。

read-only は「書き込まない」だけでは不十分である。リポジトリ内の未信頼テキストを
読ませながら `web_fetch` と `--allow-all-urls` を与えると、prompt injection により
読んだ内容を外部 URL へ送る経路になり得る。そのため critic は web tool を持たず、
`--allow-url` / `--allow-all-urls` も渡さない。外部仕様はプランまたはリポジトリに
記録済みの根拠だけで判定する。

### 非対話実行のフラグ

```
copilot -p "<prompt>" \
  --agent plan-reviewer --model gpt-5.6-sol \
  --silent --no-custom-instructions --disable-builtin-mcps --no-ask-user \
  [--add-dir <plan file の親ディレクトリ>]
```

- `-p` : 非対話でプロンプトを実行して終了する。
- `--silent` : 統計行を出さず、critic の最終応答だけを stdout に出す
  （hook はこれをそのままファイルに捕捉する。`--output-format json` は CLI の
  イベント JSONL であり critic の応答そのものではないので使わない）。
- `--no-custom-instructions` : リポジトリの `AGENTS.md` 等を読み込ませない。
  custom agent 自身のフロントマター本文はこのフラグの影響を受けず、常に system
  prompt として効く(実機で確認済み)。
- `--disable-builtin-mcps` : GitHub MCP 等の組み込み MCP サーバを無効化する。
- `--no-ask-user` : `ask_user` tool を無効化し、質問で停止させず自律的に完走させる。
- `--add-dir` : plan file がこの hook の workdir(対象リポジトリ)の外にあるときだけ
  その親ディレクトリを追加する。workdir 自体は既定で読める。

`</dev/null` は Codex 時代からの既知バグ対策をそのまま維持している。TTY が無い
プロセスは何かを待ち続ける可能性があるため、必ず標準入力を切る。

### モデル固定と premium request

`COPILOT_PLAN_REVIEW_MODEL`(既定 `gpt-5.6-sol`)で固定する。調査時点で利用可能な
最新 GPT の具体 ID であり、model catalog の変更で利用不可になった場合は fail-open
するが、warn として critic failure が記録されるのでログで気づける。

Copilot CLI の呼び出し 1 回が premium request を 1 回消費する。ラウンド 1 は
lens A / B の並列 2 critic なので、その回だけ 2 回分を消費する。それ以外の
ラウンド(単発 critic、closer)は 1 回。`--advisory` コマンドも並列時は 2 回消費する。
セッションあたりの上限は `MAX_PLAN_REVIEWS`(既定 3、closer を含む)で抑えてある。

### 認証

`copilot login` 済みであることが前提。未ログインの場合、CLI 呼び出しは失敗し
fail-open(素通し)になる — ゲートが止まるのではなく黙って通過するので、
未ログインだからといってプランが誤ってブロックされることはない。

### `--output-schema` が無いので hook 自身が厳格検証する

GitHub Copilot CLI には Codex の `exec --output-schema` に相当する、モデルの最終応答を
特定の JSON スキーマに強制する機構が無い。そこで:

- `copilot-plan-review.schema.json` は**契約文書**として残す。プロンプト
  (`config/claude/hooks/copilot-plan-review.sh` の `prompt_preamble`)にも同じ形を
  インラインで明記し、モデルに直接守らせる。
- 実際の検証は hook 内の `CRITIC_SCHEMA_JQ`(jq)が行う。top-level keys の完全一致
  (`readiness` / `findings` / `carryover` 以外を許さない)、`readiness` の 4 boolean、
  `findings` / `carryover` の各要素の必須キー・型・enum・想定外キー禁止を検証する。
- コードフェンス付き応答(`` ```json ... ``` ``)や末尾に説明文が付いた応答は、
  そもそも `jq -e '.'` で有効な JSON としてパースできないため、この時点で弾かれる。
- 不適合は他の critic failure(タイムアウト・パースエラー等)と同じ扱いで fail-open
  する。judge 側の「必須フィールドが空の BLOCKER/MAJOR は機械的に破棄」という
  既存の非空検査はそのまま残っている — jq validator は「JSON として型・形が正しいか」
  を見るだけで、「内容が空文字でないか」は judge の役割のままである。

### 導入は含めない

pinned nixpkgs の `github-copilot-cli` は 1.0.61 だが、実機で `gpt-5.6-sol` と
read-only custom agent の非対話実行を確認したのは native install の 1.0.82 である。
新しい GPT モデルとの互換性を優先し、パッケージ管理(`home/modules/packages.nix` への
追加や `nixpkgs-unstable` オーバーレイの要否検討)は本 PR の範囲に含めない。
`copilot` バイナリが無いマシンでは ADR-0005 のバイナリ存在ゲートにより黙って
no-op になるので、導入前のホストにも安全にデプロイできる。

## 運用


### 環境変数

| 変数 | 既定 | 意味 |
|---|---|---|
| `COPILOT_BIN` | `copilot` | バイナリ名。不在ならサイレント no-op |
| `MAX_PLAN_REVIEWS` | `3` | セッションあたりのレビューラウンド上限。**最終ラウンドは closer（lens Z）になる**ので、発見ラウンドは `MAX-1` 本。`1` に絞った場合だけ closer 化せず従来の単発 critic に落ちる |
| `COPILOT_PLAN_REVIEW_MODEL` | `gpt-5.6-sol` | critic に固定する具体モデル ID |
| `COPILOT_PLAN_REVIEW_AGENT` | `plan-reviewer` | `--agent` に渡す read-only custom agent 名(`~/.copilot/agents/<name>.agent.md`) |
| `COPILOT_PLAN_REVIEW_TIMEOUT` | `280` | critic 1 本あたりの timeout（hook 側 300s より短く保つ） |
| `COPILOT_PLAN_REVIEW_GATE_SEVERITIES` | `BLOCKER,MAJOR` | deny を張る severity |
| `COPILOT_PLAN_REVIEW_PARALLEL` | `1` | `0` でラウンド 1 を単発 critic (lens M) に落とす |
| `COPILOT_PLAN_REVIEW_RETENTION_DAYS` | `30` | log / json / backlog / state の保持期限。`0` で無効 |
| `COPILOT_PLAN_REVIEW_DIR` | `~/.claude/plan-reviews` | 出力先（selftest が差し替える） |
| `COPILOT_PLAN_REVIEW_SCHEMA` | スクリプトと同じディレクトリ | 契約文書として読む schema.json のパス（jq validator 自体はスクリプトにハードコードしてあり、このファイルを実行時に読み込むわけではない） |
| `SKIP_PLAN_REVIEW` | — | `1` でスキップ |

`COPILOT_PLAN_REVIEW_GATE_SEVERITIES` と `MAX_PLAN_REVIEWS` は `home/modules/claude.nix` の
`home.sessionVariables` で `BLOCKER` / `2` に再校正している。実測（`~/.claude/plan-reviews/`
322 セッション分、Codex 版時点のデータ）で deny 率 69%、3 ラウンド到達が中位という結果が
出ており、review の価値より摩擦が勝っていたための調整。MAJOR は backlog へ落として報告のみ
になる。`home.sessionVariables` は次回ログインから効く（同一シェルでは反映されない）。

### 生成物

```
~/.claude/plan-reviews/
├── <日時>-<sid8>.md          # judge がレンダリングしたレビュー（1 行目が ## GATE:）
├── <日時>-<sid8>.json        # judged + critic の生出力（フォレンジック用）
├── debug-last-input.json     # hook の stdin のメタデータのみ
├── skip                      # 存在すればスキップ（保持期限の掃除対象外）
├── backlog/<session_id>.md   # MINOR / NIT（closer の新規は severity 問わずここ）
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
bash config/claude/hooks/copilot-plan-review.sh --selftest
```

judge を fixture で回し、hook 経路を偽 copilot(実機の 1.0.82 の引数形 — `-p`,
`--agent`, `--model`, `--silent`, `--no-custom-instructions`,
`--disable-builtin-mcps`, `--no-ask-user`, `--add-dir` — を
受け、固定契約や危険フラグの不在を自ら検証する)で end-to-end に駆動する。
`COPILOT_PLAN_REVIEW_DIR` を `mktemp -d` に差し替えるので実運用のログ・state・`skip` は
触らない。selftest 冒頭でホストの `home.sessionVariables`(`COPILOT_PLAN_REVIEW_*` /
`MAX_PLAN_REVIEWS`)が既に export されていてもテストの前提を汚染しないよう、判定に使う
グローバルを既知値へ固定し直し、hook 経路のサブプロセスにも `env -u` で同じ変数群を
明示的に消してから渡す(#56, #58 の再発防止)。CI（`.github/workflows/ci.yml` の
dry-run job）でも同じものが走る。

回帰テストとして特に重要なのは:

- 「carry-over `UNRESOLVED` + 新規 0 件 → deny」— 初版の穴に対応。
- 「closer で空 gate を明示 → `gate=false` / `new_eligible=0`」— `judge()` の既定値展開を
  `${3:-...}` に戻すと落ちる。closer の新規流入停止が生きているかの検査。
- 「closer で carry-over 全閉 → 素通り」— **PASS 経路が到達可能であること**の検査。
  第二次の非収束ではここが 0/9 だった。
- 「--agent / --model 固定、read-only 境界、危険フラグ不在」— 偽 copilot が
  `--allow-all-tools` / `--yolo` / `--allow-all` / `--allow-all-paths` /
  `--allow-all-urls` / `--allow-url` を検出したら
  即座に非ゼロ終了する。呼び出し contract が壊れたら他の全テストも道連れで落ちる設計。
- 「コードフェンス付き JSON / top-level キー欠落・余剰 / enum・型不正」→ 全て
  critic failure として fail-open する — `--output-schema` が無い分、この jq validator
  が唯一の構造ゲートであることの検査。

### 効果測定

```bash
cd ~/.claude/plan-reviews
# セッションごとの最終 GATE。PASS で終わったセッションが定常的に出ているか
for f in state/*.count; do s="$(basename "$f" .count)"
  printf '%s %s %s\n' "${s:0:8}" "$(cat "$f")" \
    "$([ -e "state/$s.escalated" ] && echo ESCALATED || echo closed)"
done | awk '{print $3}' | sort | uniq -c

# closer ラウンドが PASS を出しているか（lens Z のログだけ見る）
grep -l 'lens: Z' *.md | xargs grep -h '^## GATE' | sort | uniq -c

# carry-over の閉率。分母が小さいまま UNRESOLVED が積むなら critic 側の問題
jq -r '.judged.closed[]._carry, (.judged.carried[] | "UNRESOLVED")' *.json |
  sort | uniq -c
```

ベースライン（Codex 版時点の実測。Copilot 版移行後の再測定はまだ無い）:

| 指標 | 初期実装 (#22) | closer 導入前 (#23) | 目標 |
|---|---|---|---|
| セッションが PASS で終わる率 | 1/43 (2.3%) | **0/9 (0%)** | 定常的に PASS が出ること |
| 人間エスカレーション | — | 9/9 (100%) | 例外的な事象であること |
| carry-over 閉率 | — | 22/27 (81%) | 維持 |

エスカレーションがまた常態化するなら、次に疑う順序は
(1) closer の carry-over 判定精度、(2) `MAX_PLAN_REVIEWS` を増やす前に発見ラウンドの
severity 定義、(3) `COPILOT_PLAN_REVIEW_GATE_SEVERITIES` を `BLOCKER` に絞る、
(4) `COPILOT_PLAN_REVIEW_MODEL` の model catalog 上の可用性。
**ラウンド数を増やす対策は採らない** — 最終ラウンドが closer である限り収束は
保証されており、非収束が再発したならそれは別の不変条件が破れている。

## 確定した技術事実（現行 = Copilot CLI）

これを「修正」しないための記録。

- **read-only と network-safe は別の境界である。** `web_fetch` はファイルを書かなくても、
  読み取った内容を URL へ載せて外部送信できる。critic は web tool を持たず、URL allow
  フラグも渡さない。
- **custom agent のフロントマター本文(system prompt)は `--no-custom-instructions`
  の影響を受けない。** このフラグが抑制するのはリポジトリの `AGENTS.md` 等の
  自動読み込みだけで、`--agent` で明示指定したファイルの内容は常に効く
  （1.0.82 実測）。
- **`--output-format json` は critic の応答ではない。** CLI 内部のイベント JSONL
  (`assistant.message` 等)であり、最終応答テキストそのものではない。critic の
  最終応答は `--silent` 付きの通常出力(stdout)から取る。
- **Copilot CLI には `--output-schema` に相当する機構が無い。** 最終応答の形式は
  プロンプトで指示するだけで、CLI 側の強制力は無い。だから
  `copilot-plan-review.sh` の `CRITIC_SCHEMA_JQ` が hook 側の唯一の構造ゲートに
  なっている(「Copilot への移行」節)。
- `copilot -p` は非 TTY で待ち続ける可能性があるため `</dev/null` を維持する
  （Codex 時代からの既知バグ対策を踏襲）。
- ExitPlanMode の hook は cwd が `~` になるので、stdin JSON の `.cwd` へ明示 `cd` する
  （anthropics/claude-code#22343）。
- プラン本文は `tool_input.plan` → `tool_input.planFilePath` → `~/.claude/plans/` の
  最新 `.md` の順で取る。`planFilePath` を挟むのは、最新 mtime へのフォールバックが
  並行セッションで別プランを拾いうるため。
- `~/.claude/settings.json` は Claude Code 自身が実行時に書き換えるので store symlink に
  できない。hook 登録は activation 時の冪等な jq マージで行う（`home/modules/claude.nix`）。

## 確定した技術事実（履歴 = Codex CLI 版。現在は非該当）

Codex 版時代に確定していた事実。現行の Copilot 実装はこれらの機構を使っていないが、
過去にこの設計へ到達した経緯として残す。

- `--search` は root 専用フラグ。`codex --search exec ...` が正しく、
  `codex exec --search` はパースエラーで即死する（codex-cli 0.147.0 で実測）。
- `codex exec --output-schema <FILE>` で最終メッセージの形を強制できた。
  OpenAI strict structured outputs の制約により、schema の全 object に
  `"additionalProperties": false` と全プロパティの `required` 列挙が必要で、
  `minLength` は使えなかった。
- `codex exec` は非 TTY で stdin を待ち続けるので `</dev/null` が必須だった
  （openai/codex#20919）。
