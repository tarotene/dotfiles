## Antigravity CLI(agy)固有の施行配線(ADR-0032 Amendment)

以下は Antigravity CLI(`agy`)に固有の運用で、共有の規範を agy の道具名・計画の受け渡しに
合わせて具体化したもの。Claude Code の `~/.claude/CLAUDE.md`、Codex の
`config/codex/AGENTS.codex.md` にある同種の節と対をなす。

### 道具名の読み替え

共有の規範は Claude Code の道具名で書かれている部分がある。agy では次のように読み替える。

| 規範の中の名前 | agy の道具 |
|---|---|
| `Bash` | `run_command`(引数 `CommandLine`) |
| `Read` | `read_file`(引数 `Path`) |
| `Edit` | `edit_file`(引数 `Path`・`Target`・`Replacement`) |
| `Write` | `write_file`(引数 `Path`・`Content`) |
| `Agent`・`Task` | `invoke_subagent`(引数 `Prompt`) |
| `ExitPlanMode` | `exit_plan_mode`(引数 `plan`) |

### 計画の書式(`exit_plan_mode` に渡す前)

agy は、メッセージを受けるたびに計画のターンから始まり、`exit_plan_mode` で計画を実行側へ
渡す。非自明な設計判断(機構の選び方・配置・分割・命名・プロトコル・アルゴリズム)を含む計画には、
`## 先行例との対比` 節を置き、判断ごとに `Dn:` 行(採った判断 / 出典と取得日 / 先行例との差分 /
`軸:`)を書く。設計判断を含まない計画は、代わりに 1 行の免除(`先行例: 該当なし — <理由>`)で
よい。書式は precedent-grounding / technology-selection スキル(`~/.agents/skills/`)に従う。
画面(`.html`・`.css`・`config/web/` など)に触れる計画は `構造:` の行も書く
(precedent-grounding スキル §3)。

複数項目を含む依頼には `## 要求インベントリ` 節を置き、逐語で `Rn` の ID を振って処分
(実装する段、または閉じたタグ)を書く(requirements-inventory スキル)。

計画の検査(`## 先行例との対比` の節または 1 行の免除、要求インベントリ)は、`exit_plan_mode` を呼ぶと
hook が自動で走り、足りなければ拒否される。**検査の道具(`~/.claude/hooks/` の `plan-precedent-gate`・
`plan-scope-gate` など、hook のスクリプト)を、自分で実行して試さない。** agy の安全監視が、hook を探る・回避する
動きとして検知する(2026-11-27 の実測)。計画は、上の書式で書いて、そのまま渡す。拒否されたら、理由が書式の
例を示すので、それに従って直して渡し直す。共有の skill(`precedent-grounding`・`technology-selection` など)に
「提出前の自己検査」として `--check` を実行する手順があっても、agy では実行しない。

### 検査(hook)

Claude・Codex と同じ検査が、agy の hook として働く(`~/.gemini/config/hooks.json` の `dotfiles` のキー)。
ツール呼び出しの前後の検査は `hook-router`、Stop・SessionStart の検査(PR の完了、ja-polish、main checkout の
事後検査、wrap-up など)は `hook-router wrap` 経由で走る。拒否されたら、理由に従って直して出し直す。
計画の関所(要求インベントリ・先行例との対比)は `exit_plan_mode` の前に deny でかかる。

### PR 運用(セッション内チェーン・完了定義)

同一セッション・同一 worktree で複数の PR を作るときは、常に作成順の単一チェーン(stacked PR)に
積む(ADR-0027)。コード変更を伴うタスクは commit → push → `gh pr create` を、途中で確認を挟まず
一続きで実行する。「PR を作成しますか?」と聞かない。PR 本文に未チェックの task list を残さず、
人の確認が必要な残作業は後続 Issue へ払い出す。

### 待ち方・テストの出力・プロセスの検索

- CI の完了は `pr-checks-wait <PR>` を背景で実行して待つ。`gh pr checks --watch` や自作の
  ポーリングで待たない。前景の `sleep` の後にコマンドを続けない。
- `cargo test`・`cargo nextest` の出力を `| head` / `| tail` で切らない。絞るなら
  `2>&1 | grep -E 'FAILED|panicked|test result'`、またはファイルに書いて終了コードを別に確かめる。
- `pkill -f` / `pgrep -f` の検索語は、自分のコマンド行に当たらない形で書く
  (`pkill -f '[t]rial.sh'`)。

### GitHub 投稿の生成元明示

共有 AGENTS.md の規範における「自分自身のエージェント名と URL」は、Antigravity CLI では固定で
この 1 行:

```
🤖 Generated with [Antigravity CLI](https://antigravity.google/)
```
