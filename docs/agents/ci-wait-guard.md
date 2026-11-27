# ci-wait-guard — 不適切な CI 待ち方を実行前に止め、`pr-checks-wait` へ案内する

実装: `crates/ci-wait-guard`(Rust)。`hook-router` の規則として実行される(`crates/hook-router/src/rules.rs`、`tools: Named(["Bash", "Monitor"])`)。Claude Code と Codex で同じ表を参照するため Codex にも効く(Codex に `Monitor` はないが無害)。
待ち方の判定の正本: `docs/agents/pr-gate.md`「待ち方: 早期 NG・早期 OK」(`crates/pr-gate/src/gates/ci.rs` の `decide`)
Issue: No-Issue(セッション内の会話から直接起票。「CI が一つでも失敗したら Early Return で NG、Required Check が全部通ったら Early Return で OK という実行モデルをハーネスで強制しないといつまでもセッションが終わらない」という依頼から)

## 動機

PR を出した後の CI 待ちでセッションが終わらない問題があった。待ち方が散文の規約(共有 `AGENTS.md`「実装タスクの完了定義」、`config/claude/CLAUDE.md`「待ち方」)に書いてあるだけで、守られても規約が指す手段(`gh pr checks --required --watch`)自体が遅かった。

- `--required --watch --fail-fast`: required の `CI passed` は集約ジョブ(`needs: [...]` + `if: always()`)であり、全ジョブの完了後にしか結果を出さない。30 秒で失敗したジョブの NG が、最も遅い build の完了まで返らない。
- `--watch --fail-fast`(`--required` なし): 失敗は早く見えるが、required でない `Build altair` などの完了まで OK が出ない。
- `until ! gh pr checks … | grep -q pending; do sleep 20; done` を `Monitor` で張る形(2026-10-25、#1001)は、「全部終わるまで待つ」を自前で書いたものである。`sleep N; gh pr checks …` は Claude Code 本体が弾く(#1059)が、代わりの正しい手段がなかった。

正しい手段(`pr-checks-wait <PR>`、失敗 1 件で即 NG・required 全通過で即 OK)を用意し、それ以外の待ち方を実行前に止める。止めるだけで案内がない拒否は行き止まりになるため、拒否文には必ず `pr-checks-wait` を記載する。

## 判定

コマンド文字列(`Bash` のコマンド、または `Monitor` の `tool_input.command`)をトークン化して判定する。

| # | 条件 | 結果 |
|---|---|---|
| 1 | `gh pr checks` と同じ区切り内に `--watch`(`--watch=…` も対象) | deny |
| 2 | `gh run watch` | deny |
| 3 | 待つコマンド(`until` / `while` / `sleep` / `watch`、コマンド位置)と、CI の状態を取得する `gh` が同じコマンドにある | deny |
| — | それ以外(`pr-checks-wait`、1 回きりの `gh pr checks <N> [--required] [--json …]`、`gh run view --log-failed` など) | 通す |

「CI の状態を取得する `gh`」は、`gh pr checks`、`gh run view`、`gh run list`、`statusCheckRollup` を読む `gh pr view`、`check-runs` / `check-suites` / `/status` / `/statuses` を叩く `gh api` である。`bash -c '…'` の中身は 2 段まで再帰して見る。

検出はトークン単位で行い、`command-inspection` の字句解析を使う。クォートされた文字列は 1 トークンになるため、`git commit -m "… gh pr checks --watch …"` や `echo "until gh pr checks …"` には当たらない。heredoc の本文は見ない。トークン化できない(unmatched quote)コマンドは判定不能として通す(fail-open)。

### 限界

- ダブルクォート内のコマンド置換(`"$(gh pr checks …)"`)は 1 トークンになるため見えない。
- heredoc で渡したスクリプト(`bash <<'EOF' … EOF`)は見ない。
- 任意のシェル文字列内のポーリングを表現不可能にはできない(検出のみ)。待つための正しい手段を用意したうえで、よくある誤用を実行前に止めるところまでがこの検査の責務である。

## 逃げ道

PR に紐づかない workflow run(release など)は `pr-checks-wait` の対象外のため、`gh run watch` で待ちたい場合がある。そのときはコマンドに `CI_WAIT_GUARD_ALLOW=1` を前置する(`CI_WAIT_GUARD_ALLOW=1 gh run watch <id> --exit-status`)。値は `1` のみ(呼び出し単位)。コマンド履歴に残るため後から確認できる。利用者は起動前に環境変数 `SKIP_CI_WAIT_GUARD=1` を設定してもよい(セッション単位)。PR の CI 待ちに使うのはこの逃げ道の趣旨に反する。名前と真偽値は `agent_session::escape` の表が決める。

**この環境変数前置を付けた `gh` は sandbox の外で動かす必要がある(F15)。** `gh` は `sandbox.excludedCommands`(sandbox 外で動かすコマンド)だが、`CI_WAIT_GUARD_ALLOW=1` は Claude Code が除外パターンの照合前に剥がす変数(既知の安全な変数)に含まれないため、前置すると呼び出し全体が除外に一致せず sandbox 内に残る(`gh` は HTTP 401 になる)。`sandbox-compound-guard` もこの形を止める(前置の拒否)。外で動かすには、承認を得て `dangerouslyDisableSandbox: true` で呼ぶ(外の呼び出しは `sandbox-compound-guard` が止めない)。`SANDBOX_COMPOUND_GUARD_ALLOW=1` を追加しても呼び出し自体は sandbox 内に残るため不十分である。拒否文にも同じ案内を添える。

## 発火範囲

全リポジトリ(`pr-followup-guard` と同じ)。`pr-checks-wait` は ruleset のないリポジトリでも報告された全チェックの完了で判定して動くため、拒否が行き止まりにならない。

## 執行点

- `crates/ci-wait-guard/src/lib.rs`
- `crates/hook-router/src/rules.rs`
- `crates/pr-gate/src/checks_wait.rs`
