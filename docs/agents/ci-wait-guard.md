# ci-wait-guard — CI の待ち方を実行前に止め、`pr-checks-wait` へ案内する

実装: `crates/ci-wait-guard`(Rust)。`hook-router` の規則として走る(`crates/hook-router/src/rules.rs`、
`tools: Named(["Bash", "Monitor"])`)。Claude Code と Codex は同じ表に載るので、Codex にも効く
(Codex に `Monitor` は無いが、無害)。
待ち方の判定の正本: `docs/agents/pr-gate.md`「待ち方: 早期 NG・早期 OK」(`crates/pr-gate/src/gates/ci.rs` の `decide`)
Issue: No-Issue(セッション内の会話から直接起票。依頼は「CI が一つでもコケたら Early Return で NG、
Required Check が全部通ったら Early Return で OK という実行モデルをハーネスで強制しないと
いつまで経ってもセッションが終わらない」)

## 動機

PR を出したあとの CI 待ちでセッションが終わらない。待ち方が散文の規約(共有 `AGENTS.md`「実装タスクの完了定義」、
`config/claude/CLAUDE.md`「待ち方」)に書いてあるだけで、守られても、その規約が指す手(`gh pr checks
--required --watch`)自体が遅かった。

- `--required --watch --fail-fast`: required の `CI passed` は集約ジョブ(`needs: [...]` + `if: always()`)で、
  全ジョブの完了後にしか結果を出さない。30 秒で落ちたジョブの NG が、最も遅い build の完了まで返らない。
- `--watch --fail-fast`(`--required` なし): 赤は早く見えるが、required でない `Build altair` などの完了まで OK が出ない。
- `until ! gh pr checks … | grep -q pending; do sleep 20; done` を `Monitor` で張る形(2026-10-25、#1001)は、
  同じ「全部終わるまで」を自作したもの。`sleep N; gh pr checks …` は Claude Code 本体が弾く(#1059)が、
  そのあとの代替の手が無かった。

正しい手を 1 つ用意し(`pr-checks-wait <PR>`、赤 1 件で即 NG・required 全緑で即 OK)、それ以外の形を実行前に止める。
止めるだけで案内が無い拒否は行き止まりになるので、拒否文は必ず `pr-checks-wait` を指す。

## 判定

コマンド文字列(`Bash` のコマンド、または `Monitor` の `tool_input.command`)をトークン化して見る。

| # | 条件 | 結果 |
|---|---|---|
| 1 | `gh pr checks` と同じ区切り内に `--watch`(`--watch=…` も) | deny |
| 2 | `gh run watch` | deny |
| 3 | 待つコマンド(`until` / `while` / `sleep` / `watch`、コマンド位置)と、CI の状態を引く `gh` が同じコマンドにある | deny |
| — | それ以外(`pr-checks-wait`、1 回きりの `gh pr checks <N> [--required] [--json …]`、`gh run view --log-failed` など) | 通す |

「CI の状態を引く `gh`」は、`gh pr checks`、`gh run view`、`gh run list`、`statusCheckRollup` を読む `gh pr view`、
`check-runs` / `check-suites` / `/status` / `/statuses` を叩く `gh api`。`bash -c '…'` の中身は 2 段まで再帰して見る。

検出はトークン単位で、`command-inspection` の字句解析を使う。クォートされた文字列は 1 トークンになるので、
`git commit -m "… gh pr checks --watch …"` や `echo "until gh pr checks …"` には当たらない。heredoc の本文は見ない。
トークン化できない(unmatched quote)コマンドは判定不能として通す(fail-open)。

### 限界

- ダブルクォート内のコマンド置換(`"$(gh pr checks …)"`)は 1 トークンなので見えない。
- heredoc で渡したスクリプト(`bash <<'EOF' … EOF`)は見ない。
- 任意のシェル文字列の中のポーリングを、表現不可能にはできない(検出のみ)。待つ正しい手を用意したうえで、
  よくある形を実行前に止めるところまでが、この検査の責務。

## 逃げ道

PR に紐づかない workflow run(release など)は `pr-checks-wait` の対象外なので、`gh run watch` で待ちたいことがある。
そのときは、コマンドに `CI_WAIT_GUARD_ALLOW=1` の語を足す(`CI_WAIT_GUARD_ALLOW=1 gh run watch <id> --exit-status`)。
環境変数でも同じ。値は `1` だけ。コマンドに残るので、あとから読める。PR の CI に使うのは、この逃げ道の趣旨に反する。

## 発火範囲

全リポジトリ(`pr-followup-guard` と同じ)。`pr-checks-wait` は ruleset の無いリポジトリでも、報告された全チェックの
静止で判定して動くので、拒否が行き止まりにならない。

## 執行点

- `crates/ci-wait-guard/src/lib.rs`
- `crates/hook-router/src/rules.rs`
- `crates/pr-gate/src/checks_wait.rs`
