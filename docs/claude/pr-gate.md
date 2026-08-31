# PR completion barrier — Stop の 1 点だけで CI 待ち・push 忘れを弾く

herdr の並行 worktree セッションで起きる 3 種の事故のうち、ローカル hook で扱える 2 つ
（CI 待ちのまま完了を宣言する／push し忘れたまま完了する）を Stop hook で hard gate する
仕組みの設計記録。実装は `config/claude/hooks/pr-gate.sh`、デプロイは
`home/modules/claude.nix`。3 つ目の事故（main が進んだことに気づかない）は advisory
に留める。

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

## `stop_hook_active` を見ない理由

既存の `wrapup-stop-gate.sh` は `stop_hook_active == true` を見て即 `exit 0` する
（無限ループガード）。pr-gate に同じ形を持ち込むと、1 回目の block（例: 未 push）で
Claude が push した直後の 2 回目の呼び出しが `stop_hook_active=true` で即座に素通りし、
**CI の判定に一度も到達しない**。

これは `docs/claude/codex-plan-review.md` の「第二次の非収束」（最終ラウンドに carry-over 判定の
機会が無く、`open set = ∅` が原理的に到達不能だった話）と同型の穴である。pr-gate は
`stop_hook_active` を無視し、**独自カウンタ**（`state/<sid>.count`）で上限を持つ。
`count == MAX_BLOCKS` に達したら 1 回だけ escalate 文言を返して
`state/<sid>.escalated` を touch し、以後そのセッションは無条件で素通る。
`escalated` のチェックは上限判定より**前**に置く（`docs/claude/codex-plan-review.md` の closer /
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
criterion にしない**。これは `docs/claude/codex-plan-review.md` の「judge をシェルに置くのが要点。
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
| `PR_GATE_MAX_BLOCKS` | `3` | escalate までの block 回数 |
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

- **PASS 経路が到達可能であること**（`docs/claude/codex-plan-review.md` の「第二次の非収束」と
  同種の穴——ゲートが弾くだけで一度も通さない実装になっていないかの検査）
- **`--watch` の exit code に依存していないこと** — stub の `--watch` を常に exit 0 に
  固定した上で、`--json` が部分 pending / fail を返すケースで block されること
  （上記「部分出現」の回帰テスト）
- **チェック 0 件・部分出現・stacked PR フォールバックのそれぞれで block/pass が
  正しく分岐すること**
