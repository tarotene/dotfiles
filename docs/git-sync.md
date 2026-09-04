# Git 同期設定 — herdr の並行 worktree が生む同期事故への手当て

worktree の作成・削除そのものと stale 登録の監査は
[`worktree-lifecycle.md`](worktree-lifecycle.md)を参照。

herdr は PR ごとに worktree を切るが、`git worktree add -b <branch> <path>` を
**親 checkout の HEAD から、fetch せず、upstream も張らずに**実行する(実測: 全
herdr ブランチの reflog が `branch: Created from HEAD`、fresh worktree に
`branch.*.remote` が無い)。herdr はこのリポジトリの管理外なので入口は直せない。
以下は、その結果として実際に起きた(または起きかけた)同期事故に対する、
`home/modules/git.nix` の config と 2 本の git hook による手当て。

Claude Code 側の advisory・hard gate(base 鮮度・push 忘れ・stash)は
`docs/claude/pr-gate.md` / `docs/claude/git-stash-guard.md` を参照。ここは
**マシン全リポジトリに効く git 自体の設定**を扱う。

## 対応表

| 設定 / hook | 効く事故 | 挙動 |
|---|---|---|
| `pull.ff = "only"` | 古い base に気づかず暗黙の merge/rebase commit が生える | 分岐した状態で `git pull` すると fail する。rebase/merge は人間が明示的に選ぶ |
| `fetch.prune = true` | 削除済み PR ブランチの remote-tracking ref が溜まる | `git fetch` のたびに `origin/*` の消えたものを自動で畳む |
| `push.autoSetupRemote = true` | herdr worktree が upstream 無しで作られ、初回 push が `--set-upstream` を要求してくる | 初回 `git push` が自動で upstream を張る |
| `rerere.enabled = true` | 同じ衝突を worktree ごとに手で再解決する | 一度解決した衝突の解決結果を再利用する |
| `merge.conflictStyle = "zdiff3"` | 衝突表示に共通祖先が無く、エージェントの解決精度が落ちる | 3-way diff に共通祖先を追加した表示に変える |
| `config/git/hooks/pre-commit` の protected-branch ガード | worktree を切ったつもりで親 checkout の `main`/`master` に直接 commit してしまう | `main`/`master` への直接 commit を `exit 1` で拒否する。`GIT_ALLOW_MAIN_COMMIT=1` で回避 |
| `config/git/hooks/prune-branches.sh` | ローカルに残った `[gone]` ブランチが溜まり続ける | `git prune-branches` で一覧確認 → 1 回だけ y/N 確認 → 削除 |
| `git shelve` / `git unshelve`(`scripts/git-shelve` / `scripts/git-unshelve`) | worktree 間で共有される stash スタックの取り違え(他 worktree の WIP を pop/apply/drop してしまう) | worktree の絶対パスをタグに積み、自分の entry だけを SHA で解決して apply/drop する。詳細は `docs/claude/git-stash-guard.md` |

## `git prune-branches`

`alias.prune-branches` は `config/git/hooks/prune-branches.sh` を呼ぶだけの薄い
alias。旧実装(`--merged=main` かつ `[gone]` を AND する 1 行 alias)は squash merge
運用と噛み合っていなかった —— squash merge では PR のコミットが `main` の祖先に
**ならない**ため `--merged=main` は常に偽になる(実測: ローカル 30 ブランチ中
`[gone]` は 21 本、旧 alias が実際に消せたのは 2 本だけ)。

新しい判定は `[gone]` 単独 —— これは「PR がマージされてリモートブランチが GitHub
側で削除された」(または誰かが手で消した)ときにだけ現れる、`--merged` より
信頼できる合図。`git branch -D`(unmerged でも強制削除)を使うが、削除前に:

1. 対象ブランチの一覧を表示し、y/N を一度だけ確認する
2. `[gone]` だが**いずれかの worktree に checkout 中**のブランチは別掲し、
   削除せずに「worktree を先に畳んでください」と案内する(`git branch -D` は
   checkout 中のブランチの削除を拒否するので、黙って失敗させない)

`main` は upstream が `[gone]` にならないので、判定に触れることすらない。

## `GIT_ALLOW_MAIN_COMMIT`

`config/git/hooks/pre-commit` の protected-branch ガードは `--no-verify` を回避手段に
**しない**。理由は `core.hooksPath` が全リポジトリで pre-commit を一本化しているため
——`--no-verify` は同じ pre-commit にチェインされている他プロジェクトの
`.pre-commit-config.yaml`(ruff/mypy 等)も一緒に飛ばしてしまう。専用の
`GIT_ALLOW_MAIN_COMMIT=1` は自分のガードだけをスキップし、チェイン先は必ず走る:

```bash
GIT_ALLOW_MAIN_COMMIT=1 git commit -m "..."
```
