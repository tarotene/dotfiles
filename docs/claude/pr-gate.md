# PR completion barrier — Stop の 1 点だけで CI 待ち・push 忘れを弾く

herdr の並行 worktree セッションで起きる事故のうち、ローカル hook で扱える 4 つ
（CI 待ちのまま完了を宣言する／push し忘れたまま完了する／PR を Issue に繋がないまま
終わる／見た目の変更なのに視覚証跡が無いまま終わる)を Stop hook で hard gate する
仕組みの設計記録。実装は `config/claude/hooks/pr-gate.sh`、デプロイは
`home/modules/claude.nix`。main が進んだことに気づかない件と未コミット変更は
advisory に留める。

「push し忘れたまま完了する」は PR が既にある前提の判定（`G_push`）だけでは、
**PR を作る前**という最も起きやすい場面を素通ししていた。`G_unpushed` がその領域を
塞ぐ。同様に base 鮮度・`[gone]` ブランチ・残骸 worktree の advisory は、以前は
「open PR が無ければ SessionStart も完全沈黙」だったため、PR 作成前のセッションでは
一切表示されなかった。詳細は下の各節。

## 強制力はどこに置くか

Hooks を PR の SDLC に絡めるとき、選択肢は次の 3 段階ある。

```
PreToolUse(git push)  … push という「行為」を弾く
PostToolUse            … push の「後」に何かする
Stop                    … 「完了しよう」とした瞬間に弾く
```

このリポジトリでは **Stop の 1 点だけ**に強制力を置いた。

### `PreToolUse(git push)` を採らない理由

この choke point には既に `config/git/hooks/pre-push`（home-manager がグローバル配備、
`core.hooksPath` で全リポジトリに適用）があり、worktree からの push を全面ブロックしている。
除外を試みた PR #21 は CLOSED で終わっており、運用は「`--no-verify` で通す・都度ユーザーに
確認する」に定着している。ここに 3 枚目の push ガードを重ねると、`--no-verify` の判断が
「pre-push 用」「pr-gate 用」のどちらへの回答か曖昧になる。

stale base や `--force` を弾く発想自体も採らない。理由は次の「G_base を弾かない理由」と同じ
——履歴改変の安全境界は既存の道具（`arkedge/sbir-aocs-report` #135 の `/aocs-rebase`）に
委譲する。

### `PostToolUse` の async CI watcher を採らない理由

Claude Code の command hook の `async: true` と、「後から Claude の context に結果を注入する
非同期機構」は文書上どちらも存在しない。仕様として成立しないため検討の対象外とした。

### Stop を選ぶ理由

Claude が「完了しました」と言おうとする瞬間は、CI・push・base の状態がすべて確定している
唯一のタイミングである。push の可否は既存の `pre-push` に任せ、pr-gate は「終わろうとした
時点で本当に終わっていいか」だけを見る。

## G_base / G_worktree を block にしない理由

`sbir-aocs-report` の Issue #135 は、まったく同じ「base 追従の stale をどう扱うか」を
すでに決着させている:

> 検出は決定的な shell script、Hook は adapter。script がやるのは inspect と `git fetch`
> まで。ローカル履歴の改変（rebase）・リモートへの force-push は人間が明示起動する
> `/aocs-rebase` に委譲する。

`G_base`（origin/base に対する ahead/behind）を block にすると、Claude は rebase →
force-push という履歴改変に入る。この環境では `pre-push` + `--no-verify` の判断が絡む上、
squash merge のみで linear history は自動的に保たれるため、merge-time の実害は限定的
（並行 PR がぶつかるのは base の差分をレビューで見落とすリスクだけで、GitHub 側が
conflict を検出したらそもそも merge できない）。同じ理由で `G_worktree`（未コミット変更）も
block にしない —— PR があるブランチで WIP を残して終わるセッションを全部止める価値は無い。

どちらも **advisory**（block するときだけ相乗りで伝える。単独では終了を止めない）に留めた。
この決定そのものは変えていない —— 下の「SessionStart は PR の有無を問わず advisory を出す」
は Stop の block 判定には一切影響しない、別の話である。

## SessionStart は PR の有無を問わず advisory を出す

`cmd_session_start` は当初「open PR が無ければ完全沈黙」だった。この判断は
`cmd_stop` の「advisory は他の block への相乗りに限る」という設計とは別物のはずが、
実装上は同じ形の早期 `exit 0` になっていた。結果、**PR をまだ作っていないセッションは
base がどれだけ遅れていても、`[gone]` ブランチが何本溜まっていても一切知らされない**。

実測でこれが起きた: このリポジトリの worktree（`worktree/lucky-field-2794`）は
origin/main から 4 コミット遅れ、ローカルブランチ 30 本中 21 本が `[gone]` の状態で
新しいセッションを開いたが、pr-gate は無言だった —— PR が無いという理由だけで。

Stop の advisory が「他の block への相乗り」に留まるのは正しい判断のままにした
（`G_base` / `G_wt` を単独 block にしない理由は上のとおり、履歴改変を自動化しない）。
だが SessionStart には元々 block/pass の概念が無く、「単独発火のコストが無い」——
Stop と違って「単独では終了を止めない」という制約自体が意味を持たない場所だった。
そこで SessionStart だけ、PR の有無を問わず 3 つの hygiene advisory を計算し、
材料があれば単独で出すようにした:

- **stale base**（`stale_base_line`）: PR があれば `baseRefName`、無ければ
  `refs/remotes/origin/HEAD` の default branch に対する behind 数
- **`[gone]` ブランチ本数**（`gone_branches_line`）: `git prune-branches`（後述）が
  対応できる残骸の量
- **残骸 worktree 数**（`stale_worktrees_line`）: `[gone]` かつ未変更の worktree。
  dirty な worktree は本物の作業中の可能性があるので数えない（false positive を
  出さない側に倒す）

いずれも API を叩かず git だけで求まるので、縮退表・allowlist・fetch TTL には影響しない。
材料が無ければ(以前と同じく)完全沈黙する。

## G_unpushed — G_push が届かない領域

`G_push` は open PR の `headRefOid` とローカル `HEAD` を比較するので、**PR がまだ無い
セッションでは比較対象が無く、判定できずに完全沈黙していた**。これは「push し忘れたまま
完了する」という当初からの対象事故そのものが、PR 作成前という最も起きやすい場面では
素通しになっていたことを意味する。

`G_unpushed` はこの領域だけを塞ぐ。PR を作るべきかには踏み込まず、「積んだコミットが
1 個もリモートに無い」という事実だけを見る（upstream → `origin/<branch>` →
`origin/<default_branch>` の順で比較対象を探し、どれも無ければ判定を諦めて何もしない
——判定できないことを断定に変えない、という他の advisory と同じ姿勢）。0 件なら通す。
1 件以上なら block し、解消は `git push` 1 回で済む（`home/modules/git.nix` の
`push.autoSetupRemote` が upstream を自動で張るので `--set-upstream` の指定は不要
——履歴改変は一切伴わない）。

PR がある場合は既存の `G_push` がより正確に見るので、`G_unpushed` は
`pr_num` が空のときだけ動く。二重に block することはない。

## G_link — なぜ「PR 本文が Issue を閉じるか」を block するのか

### 直そうとしている事故

2026-08-31 に open Issue 17 件を棚卸ししたところ、**実質解決済みなのに open のまま
残っていたものが 2 件**あった（#28 jq、#29 pandoc）。どちらも `home/modules/packages.nix`
が既に宣言していて、`command -v` は nix 側を返す。作業は終わっていた。閉じていなかっただけ。

原因は単純で、解決した PR の本文に closing keyword が無く、マージが Issue に伝播しなかった。
merged PR 21 本のうち `Closes #N` を持つのは 4 本（**19%**）。

この事故の性質は、`G_push` や `G_CI` が扱うものと違う。CI 待ちや push 忘れは
**その場で**壊れる。リンク忘れは**その場では何も壊れない** —— PR はマージされ、コードは
正しく入る。コストが出るのは数か月後、誰かが Issue 一覧を見て「これは終わっているのか？」と
一件ずつ実測して回る時。遅延して現れる沈黙だ。

### なぜ Stop hook なのか（PR テンプレートでも CI でもなく）

**`.github/pull_request_template.md` は効かない。** このリポジトリの PR は全て
`gh pr create --body …` で作られており、`--body` を渡した時点でテンプレートは読まれない。
テンプレートが効くのは「本文を空で作った人間」だけで、実際の発生源（エージェント）に
一切当たらない。置いた瞬間から死に設定になる —— #33 で削った `enableSshSupport` と
同じ種類の負債を新設することになる。

**CI job にすると粒度が合わない。** `G_CI` は CI 全体の緑を完了条件にしているので、
本文チェックの job もその期待集合に入る。すると「本文に 1 行足す」で直る話に、
push → check 出現待ち → 再判定という CI ラウンドトリップを丸ごと 1 回払わせることになる。

Stop hook なら `gh pr edit --body` で即座に直り、`pr-gate.sh` が既に持っている
allowlist・縮退・block 上限・selftest の枠組みにそのまま乗る。settings.json も
CLAUDE.md も膨らまない。

### なぜ advisory ではなく block なのか

`G_base` / `G_wt` を advisory にした理由は「それ単独では事故にならない」だった。
`G_link` は違う。advisory はエージェントが読んで無視できるので、**遅延コストの構造を
そのまま再生産する** —— まさに「今は何も壊れないから後回し」が原因の事故に対して、
「今は何も壊れないという警告」を出すことになる。

### `No-Issue:` という逃がし方

対応 Issue が本当に無い PR は多い。実際、merged PR の大半はセッション中に生まれた
feature 作業で、閉じるべき Issue が存在しない。ここで「毎回 Issue を先に立てろ」に
すると、このリポジトリの実際の流れ（wrap-up inbox が**事後に** Issue を生む）と逆走し、
「PR を出すために形式的な Issue を 1 枚立てる」が常態化してトラッカーの信号が薄まる。

そこで本文に `No-Issue: <理由>` があれば通す。狙いは**沈黙を決定に変えること**で、
書く手間はほぼゼロだが、書く瞬間に「本当に対応 Issue は無いか」を一度だけ考えることになる。
#28/#29 は、その一考があれば残らなかった。

副次的だが重要な性質として、`No-Issue:` は **grep 可能な監査可能痕跡**を残す。advisory の
警告は流れて消えるが、これは後から「Issue 無しと宣言した PR」を数えられる。

理由の無い裸の `No-Issue:` は受理しない。それを通すと、ただのおまじないになって狙いが消える。

### 「言及はあるが keyword が無い」だけを見ない理由

一見すると「本文が `#N` に言及しているのに closing keyword が無い」ケースだけを弾くのが
穏当に見える。採らなかった。**#28/#29 を実質解決した PR は Issue に一切言及していない。**
その形の判定は、観測された失敗そのものを通してしまう。設計として、実際に起きた事例を
素通しするゲートを入れる意味はない。

### 判定基準は「GitHub がどう読むか」

GitHub は **コード内の closing keyword を解釈しない**。fenced code block の中も、
`` `Closes #30` `` のようなインラインのコードスパンの中も無視される。判定前に両方
落としておかないと、ゲートが「閉じないのに LINKED」と読む —— つまり `G_link` が防ごうと
しているまさにその事故（マージしても Issue が open のまま）を、**ゲート自身が見逃す側に
倒れる**。

これは机上の懸念ではない。`G_link` を入れた PR #46 の本文が「本文に
`` `Closes #30 / #33 / …` `` を明記し」と実例をコードスパンで引用しており、初回の実地検証で
`gh pr view --json closingIssuesReferences` が空を返して発覚した。規約や設計を説明する PR
ほど keyword を引用するので、踏む確率は低くない。

判定基準は「GitHub がどう読むか」であって「人がどう書いたつもりか」ではない。ゲートが
GitHub より緩くても厳しくても、どちらも嘘になる。

実 PR 6 本を ground truth（`gh pr view --json closingIssuesReferences`）と突き合わせて
一致を確認している:

| PR | `judge_link` | GitHub の実リンク |
|---|---|---|
| #45 | `LINKED` | `10, 11, 30, 33, 34` |
| #46 | `NO_ISSUE` | （なし） |
| #40 / #32 / #37 / #27 | `MISSING` | （なし） |

なお HTML コメント（`<!-- … -->`）は落としていない。GitHub はそこも解釈しないが、この
リポジトリは PR テンプレートを使わない（使えない — 上記のとおり `--body` がテンプレートを
読まない）ので、コメントに keyword が紛れ込む経路が無い。踏んだら足す。

### base が default branch でない場合を advisory にする理由

GitHub の仕様上、closing keyword は **default branch を狙う PR でのみ**解釈される
（[公式ドキュメント](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/linking-a-pull-request-to-an-issue)
は "If the pull request targets any other branch, then these keywords are ignored" と明記）。
つまり stacked PR では、本文に `Closes #N` と書いてマージしても Issue は閉じない ——
**この設計が直そうとしている事故と同型の沈黙**が、書いたつもりの側で起きる。

一方 stacked PR 自体は正当な運用なので、ゲートが「それは間違い」と断ずるべき場面ではない。
よって `G_base` / `G_wt` と同じ advisory の列に置く。

default branch は `refs/remotes/origin/HEAD` から読む。`gh repo view` を叩けば確実だが、
API 呼び出しが 1 本増えると下の縮退表がその分太る。`origin/HEAD` が未設定のクローンでは
この advisory を出さない —— 判定できないことを断定に変えない。

### 参照先 Issue の実在確認をしない理由

`Closes #43` と書いて実は #34 のつもり、という誤りは正規表現では素通りする。では
`gh issue view` で実在確認するか、というと採らない。捕まえられるのは「存在しない番号」
だけで、**より起きやすい「存在するが別の Issue」は捕まえられない**。検出力が低い一方で
API 呼び出しが 1 本増え、縮退経路（未ログイン・API 失敗時の fail-open）も 1 本増える。
`pr-gate.sh` は縮退の一覧を明示して不変条件を守っている作りなので、利得の小さい判定の
ために縮退表を太らせるのは割に合わない。

### block の位置と `MAX_BLOCKS` の引き上げ

`G_link` は判定だけ先に済ませ、**block は最後に回す**。`G_push` / `G_CI` が止める場面では、
その block メッセージに相乗りさせる（実装中の `$rider`）。本文の修正は CI を待たずに
済むので、単独で 1 往復を消費させる理由がない。

単独で block するのは push も CI も通った後 —— 「あとは終わるだけ」の一点で、まさに
リンクが忘れられる瞬間だ。

judgement が 1 つ増えたぶん、`PR_GATE_MAX_BLOCKS` の既定を 3 から **4** に上げた。
最悪の連鎖（push → 本文修正 → CI 待ち）が正当に 3 回 block しうるので、3 のままだと
最後の 1 回が escalate に化ける。相乗りのおかげでこの連鎖は実際には起きにくいが、
上限は最悪ケースで決める。

## G_visual — なぜ「Before/After の視覚証跡」を block するのか

### 直そうとしている事故

`G_link` と同じ形の事故。見た目に影響する変更(GUI/Web に限らずターミナル/TUI の
見た目も含む)を merge した PR の本文に、変更前後がどう見えたかの証跡が一切無いと、
その場では何も壊れない。壊れるのは数か月後 —— 別のレビュアーや将来の自分がコードの
履歴を追ったとき、コードを読んでも「実際どう見えていたか」は再現できない。`G_link`
の「遅延コストの沈黙」とまったく同じ構造なので、advisory では構造がそのまま
再生産される。よって block する。

### 判定基準(3 択の OR)

```
(a) 本文に user-attachments の画像       — gh --attach (>= 2.99.0) が
                                            アップロード後に書く形
(b) `## Before / After` 見出し配下の
    fenced code block が 1 つ以上         — テキスト差分で十分な変更向け
(c) `No-Visual: <理由>`                   — No-Issue: と同型のエスケープハッチ
```

(a)(c) は `strip_code_spans` を通した本文で判定する。`G_link` の inline-code
回帰と同じ理由で、画像記法や `No-Visual:` を「例として」コードスパン内に引用しても
証跡として数えない。(b) だけは逆に **strip しない** —— fence 自体が判定対象の証跡
なので、strip すると判定材料ごと消えてしまう。

ローカルパスのままの画像記法(`![x](./a.png)`)はアップロードの証拠にならないので
数えない。`gh --attach` は実行後に本文の画像記法を user-attachments の URL へ
書き換える(既存の記法があればその場で、無ければ末尾に追記)ため、アップロード済みか
どうかは本文の記法だけから機械的に判定できる。

### 検査しないこと

Before と After が実際にペアで揃っているか、画像が何枚か、fence の中身が本当に
対比になっているかは検査しない。`G_link` が参照先 Issue の実在を確認しないのと
同じ理由 —— 機械的に判定できるのは「証跡があるかどうか」までで、「対比として
十分か」を判定に持ち込むと、Before が存在しない正当なケース(新規追加など)を
誤って止める側に倒れる。この「対比として十分か」の判断は `pr-description` スキル
(LLM の判断)の責務にする。ゲートとスキルのどちらが何を担うかを曖昧にしないための
明示的な分担であって、単なる手抜きではない。

### block の位置と `MAX_BLOCKS` を上げない理由

`G_link` と全く同じ「あとは終わるだけ」の一点(push も CI も通過した後)で単独
block する。しかも **`G_link` と同じ block メッセージに合流させる** —— 本文の
不備は 2 種類あっても、修正は `gh pr edit` 1 回で両方直せるので、2 往復を消費
させる理由がない。

この合流のおかげで、`G_visual` を足しても `PR_GATE_MAX_BLOCKS` は 4 のまま
据え置いた。最悪の正当チェーン(push → 本文修正 → CI 待ち = 3)の長さ自体は
変わらない —— 本文修正のステップに直す項目が 1 つ増えるだけで、chain の段数は
増えないため。

## `stop_hook_active` を見ない理由

既存の `wrapup-stop-gate.sh` は `stop_hook_active == true` を見て即 `exit 0` する
（無限ループガード）。pr-gate に同じ形を持ち込むと、1 回目の block（例: 未 push）で
Claude が push した直後の 2 回目の呼び出しが `stop_hook_active=true` で即座に素通りし、
**CI の判定に一度も到達しない**。

これは `docs/claude/copilot-plan-review.md` の「第二次の非収束」（最終ラウンドに carry-over 判定の
機会が無く、`open set = ∅` が原理的に到達不能だった話）と同型の穴である。pr-gate は
`stop_hook_active` を無視し、**独自カウンタ**（`state/<sid>.count`）で上限を持つ。
`count == MAX_BLOCKS` に達したら 1 回だけ escalate 文言を返して
`state/<sid>.escalated` を touch し、以後そのセッションは無条件で素通る。
`escalated` のチェックは上限判定より**前**に置く（`docs/claude/copilot-plan-review.md` の closer /
escalated と同じ置き方）。block cap（Claude Code 側の「連続 8 回で打ち切り」）の消費は
wrapup 側と合わせて最大 4 回に収まる。

## required check がまだ無いと、ゲートは空振りする

`gh pr checks --required` は required status check が 1 件も設定されていないと
`no required checks reported` を出して **exit 0** する。つまり ruleset を先に直さないと、
pr-gate の CI ゲートは「常に緑」を返す no-op になる。実装順序として:

1. `.github/workflows/{ci,nix}.yml` の `pull_request` から path / branch フィルタを外す
   （required check は「報告されない = 満たされない」ので、フィルタで報告漏れがあると
   その PR は永久に block されたままになる）
2. `Ephemeral Initial` ruleset（id `19799324`）に `required_status_checks` を追加する

の順で行う。ruleset は `~DEFAULT_BRANCH` スコープのままにした（stacked PR まで対象を
広げると、同じ ruleset に同居する `pull_request` / `non_fast_forward` が全ブランチに
掛かってしまい作業不能になる）。stacked PR のマージ先 feature branch は自身が `main` への
PR を持ち、そちらが ruleset の対象なので、`main` への merge-time safety はそれで保たれる。

## 中心不変条件: 「揃っていない集合を緑と読まない」

pr-gate の G_CI を実装するときに Codex plan-review で 3 回連続して指摘された欠陥は、
すべて同じ形をしている——**「チェックが無い/揃っていない」を `gh` がそのまま exit 0
で返してくる 3 つの経路**を、素通りの理由にしてしまうことだった。

| # | 経路 | 何が起きるか |
|---|---|---|
| 1 | push 直後 | check run がまだ GitHub の API に現れていない（[cli/cli#7401]） |
| 2 | stacked PR | ruleset の対象外(base が `main` 以外)で required が 0 件 |
| 3 | 部分出現 | 6 件のうち一部だけが現れ、その部分集合が pass した時点で `--watch` が早期終了する（[cli/cli#9973]） |

[cli/cli#7401]: https://github.com/cli/cli/issues/7401
[cli/cli#9973]: https://github.com/cli/cli/issues/9973

3 が最も危ない。「1 件以上現れたら待機を始める」という素朴な実装では防げないため、
`gh pr checks --watch` を**待つための道具**に格下げし、**その exit code を acceptance
criterion にしない**。これは `docs/claude/copilot-plan-review.md` の「judge をシェルに置くのが要点。
acceptance criterion を確率的な写像に任せない」と同じ論理を、LLM critic ではなく `gh` という
外部コマンドに対して適用したものである。

具体的には:

1. **期待集合 E をサーバから取る** — `gh api repos/<nwo>/rules/branches/<base>` の
   `required_status_checks[].context`。単一の真実をサーバと共有することで
   「何件揃えば良いか」を推測しない
2. E が非空なら、報告集合 R が `E ⊆ R` になるまでポーリングして待つ（出現待ち）
3. E が空（stacked PR）なら、E の代わりに **quiescence**（報告件数が一定時間増えない）
   で「揃った」を近似する
4. `gh pr checks [--required] --watch --fail-fast` で terminal state まで待つ
   （**待つだけ**。この exit code は使わない）
5. **`--json` を取り直し、jq が判定する**: E の全 context が R にあり、かつ
   fail/cancel が無く、かつ pending が無いときだけ PASS

どの経路でも「0 件」も「部分集合」も PASS には落ちない。stacked PR で required が
取れないときも「緑」と主張せず、報告された全チェックで判定したことを注記した上で判定する
（`docs/claude/issue-index.md` の「嘘の正確な数字を出さない」と同じ立場）。

## 発火範囲

`~/.claude/pr-gate-repos`（1 行 1 nwo）にある repo だけで判定する。他ホスト・他リポジトリ
（特に会社の `arkedge/*`）ではスクリプト自体は無条件配備されているが、allowlist に無ければ
`git remote -v` で nwo を取った直後に完全沈黙する。会社リポジトリ側にはすでに
「検査ではなく道具を配る」（ADR 0011）という別の合意があり、その領域を無断で広げない。

## 運用

### 環境変数

| 変数 | 既定 | 意味 |
|---|---|---|
| `PR_GATE_DIR` | `~/.claude/pr-gate` | state の置き場所 |
| `PR_GATE_ALLOWLIST` | `~/.claude/pr-gate-repos` | 判定対象 nwo の一覧 |
| `PR_GATE_MAX_BLOCKS` | `4` | escalate までの block 回数（G_link 追加時に 3 から引き上げ。G_visual は G_link と同じ block に合流するので据え置き。下記参照） |
| `PR_GATE_CI_TIMEOUT` | `300` | `gh pr checks --watch` の timeout(秒) |
| `PR_GATE_CHECK_APPEAR_TIMEOUT` | `60` | 期待集合の出現待ち上限(秒) |
| `PR_GATE_QUIESCE` | `15` | stacked PR で「安定」と見なす無変化時間(秒) |
| `PR_GATE_FETCH_TTL` | `600` | SessionStart の `git fetch` を省略する TTL(秒) |
| `SKIP_PR_GATE` | — | `1` でスキップ |

### 生成物

```
~/.claude/pr-gate/
├── state/<session_id>.count       # 消費した block 回数
└── state/<session_id>.escalated   # 人間エスカレーションを 1 回に限定するフラグ
```

### エスケープハッチ

`touch ~/.claude/pr-gate/skip` または `SKIP_PR_GATE=1`。

### 自己検査

```bash
bash config/claude/hooks/pr-gate.sh --selftest
```

`gh` をスタブして hook 経路を end-to-end に駆動する。回帰テストとして特に重要なのは:

- **PASS 経路が到達可能であること**（`docs/claude/copilot-plan-review.md` の「第二次の非収束」と
  同種の穴——ゲートが弾くだけで一度も通さない実装になっていないかの検査）
- **`--watch` の exit code に依存していないこと** — stub の `--watch` を常に exit 0 に
  固定した上で、`--json` が部分 pending / fail を返すケースで block されること
  （上記「部分出現」の回帰テスト）
- **チェック 0 件・部分出現・stacked PR フォールバックのそれぞれで block/pass が
  正しく分岐すること**
- **`G_unpushed`**: PR 無し + 未 push commit あり(exit 2)/ PR 無し + 0 件(完全沈黙)
  の両方
- **`G_visual`**: 画像・fence・No-Visual: のいずれかで PASS すること、fence は
  Before/After 見出し配下でのみ証跡になること、コードスパン内の引用(画像記法・
  No-Visual:)は数えないこと、ローカルパス画像は数えないこと、`G_link` と同じ
  block メッセージに合流すること
- **hygiene advisory**: `[gone]` ブランチ・残骸 worktree(dirty なら数えない)・
  stale base(PR 有無どちらでも)がそれぞれ単独で `additionalContext` に出ること
