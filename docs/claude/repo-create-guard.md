# repo-create-guard — `gh repo create` を作成時点で repo-charter 手順に載せる PreToolUse hook

判定エンジン: `config/claude/hooks/repo-create-guard.sh`
決定: `docs/adr/0013-repo-charter-schema.md` の Amendment (2026-09-29)
規約側: `config/claude/skills/repo-charter/SKILL.md`

`gh repo create`(および `gh api -X POST user/repos`・`orgs/*/repos`)を
作成時点で deny し、`repo-charter` スキルの手順(命名インタビュー →
README/CONTRIBUTING → GitHub メタデータ反映 → governance 播種)を踏むよう
機械強制する。

## なぜ必要だったか

`gh repo create` 自体には作成直後に走るフック機構が無く、作成時の強制点は
`repo-charter` スキルという散文的手順のみだった(ADR-0013 Decision 2)。
ある private リポジトリの新規作成セッションで、`repo-charter` SKILL.md §8
の `core` 型手順が `apply-repo-settings.sh`(squash-only 化・delete-
branch-on-merge 等の repository settings 適用)の呼び出しを欠いたまま
実行され、実際に settings drift(`github-audit settings` が 6 項目報告)が
発生した(2026-09-28 実例、実名は書かない — ADR-0034)。

手順書き漏れそのものは `repo-charter` SKILL.md 側の修正(§8 `core` 型に
`apply-repo-settings.sh` の呼び出しを追加)で直したが、「散文的手順を
読み飛ばせば常に起こりうる」という構造は残る。`gh repo create` の存在
そのものは、意味判断を要さず存在チェック(コマンド位置一致)だけで機械的に
deny できるため、ADR-0013 が「意味判断が絡み LLM なしでは無理」として
見送った Issue 起票時照合とは別種の判断であり、今回は作成時点だけ機械
強制する。

## 判定

1コマンド文字列から、対象コマンド(`gh repo create ...` または `gh api
...`)を**コマンド位置**で検出し、範囲ごとに判定する。コマンド文字列全体を
正規表現で見ない理由は `attribution-guard.sh`(`docs/claude/attribution-
guard.md`)と同じ — docs やコミットメッセージに例として書いただけで誤発火
させないため。

| 対象コマンド | 判定 |
|---|---|
| `gh repo create ...` | 常に deny(body や flag の有無を見ない) |
| `gh api ...`(`-X`/`--method` が明示的に `POST`、パスが `user/repos` または `orgs/[^/]+/repos` に一致) | deny |
| `gh api ...`(上記以外 — GET での一覧取得、PATCH での settings 変更など) | 対象外(通す) |
| `gh repo edit`/`gh repo view` 等 | 対象外(通す) |

`gh api -X PATCH repos/<owner>/<repo>`(`apply-repo-settings.sh` 自身が使う
形)を誤って deny しないことを selftest で確認している — これを塞ぐと
repo-charter の手順自体が実行不能になる。

## コマンド解析エンジンを共有する

対象コマンドの検出(コマンド位置判定)・heredoc 本体の分離・クォート解釈
トークナイザは `config/claude/hooks/attribution-guard.sh` を `source` して
再利用する(`stack-base-guard.sh`/`feedback-target-guard.sh`/`decision-
colocation-guard.sh` と同じ型)。`is_target_at`/`decide_tokens`/
`decide_api_tokens`/`main`/`selftest` を `source` の後に再定義することで
上書きする — bash の関数解決は最後の定義が勝つ。

## deny の理由文

`repo-charter` SKILL.md §1(命名インタビュー・閉語彙チェック)と §8
(`gh repo create` → `gh repo edit --add-topic` → 型別 governance 播種)の
手順をそのまま指す。バイパス手段(下記)も理由文に含める。

## 縮退・バイパス

判定不能はすべて fail-open(通す)— `jq` 不在、`tool_name` が `Bash` 以外、
`command` が空、のいずれも完全沈黙で通す。

バイパス: `REPO_CREATE_GUARD_BYPASS=1`(`crates/rulesets-write-guard` の
バイパス env var と同型)。`main()` の入口だけで見る — `decide()` 自体は
判定ロジックだけを持ち、バイパスは hook 入出力層の責務にする
(`stack-base-guard.sh` の `SKIP_STACK_BASE_GUARD` と同じ配置)。

## 既知の限界

`gh` 経由の作成は deny できるが、`curl` で直接 `api.github.com/user/repos`
を叩く経路や GitHub の Web UI からの作成はこの hook の対象外(そこまで
塞ぐには GitHub 側の仕組みが要る)。軸: 検出のみ — ADR-543 D1 の
「フルスクラッチ自体は表現不可能にできない」と同型の限界。

## 検査

- `repo-create-guard.sh --selftest` がネットワーク無しに全ケースを検査する。
- CI の `--selftest` 検査ジョブ(`.github/workflows/ci.yml`)から呼ばれる。

## 登録形

```
PreToolUse / matcher: "Bash|mcp__.*" / timeout 15
```

`attribution-guard`/`stack-base-guard` と同じ複合 matcher(Bash 単体だと
MCP 接続の瞬間に無検査になる、同じ理由の繰り返し)。現在 `mcp__github*` は
未接続のため実質 Bash のみが判定対象になる(`main()` が `tool_name ==
Bash` 以外を即 exit する) — 将来 MCP 接続時は `attribution-guard.sh` の
`decide_mcp` と同じ形で対応を追加する。判定は文字列処理のみで gh API 往復を
持たないため、timeout は `pr-title-guard.sh` 並みの短さでよい。
