# worktree のライフサイクル監査

## 目的

一時ディレクトリへ `git worktree add` した後、そのディレクトリだけが消えると、Git の
common directory には worktree 登録が残る。同じブランチを使おうとしたときの
`already used by worktree` はこの残留登録から起きる。

実際に確認した事例では、Claude が session scratchpad 配下へ直接 worktree を作り、
session cleanup が checkout だけを削除していた。Herdr が作った worktree ではなかった。
作成と終了処理を同じ管理主体に揃えるため、Claude と Codex からの直接の
`git worktree add` は hook で拒否し、次を正規の入口とする。

```bash
herdr worktree create --cwd /path/to/repo --branch feature/example
```

既存 worktree の `git status`、`git worktree list/remove/prune` は拒否しない。

## 検出と通知

`git audit-worktrees` は `~/.ghr` 配下の repository と、
`~/.herdr/worktrees` から参照される common repository を重複なく走査する。

```bash
git audit-worktrees                # 読み取り専用。検出時は終了コード 1
git audit-worktrees --prune        # 一覧を表示し、確認後に登録だけを削除
```

`--prune` が削除するのは checkout が存在しない worktree の管理メタデータだけで、
branch ref と commit は残る。非対話実行での `--prune` は誤操作を避けるため拒否し、
fixture や明示的な自動化だけが `--yes` を併用できる。

Home Manager は `git-worktree-audit.timer` を有効化し、1分間隔で同じ監査を行う。
新しい stale 集合を見つけると Herdr に通知するが、自動 prune はしない。同じ集合は
通知済み fingerprint として XDG state directory に記録する。Herdr に foreground client
がない、toast が busy などで表示されなかった場合は通知済みにせず次回再試行する。

Claude/Codex の SessionStart でも監査し、残留があれば agent context に一覧を渡す。
Codex は `~/.codex/hooks.json` の新しい command を初回だけ信頼確認する。Home Manager の
マージは Herdr integration を含む既存 hook を保持する。

## 確認

```bash
systemctl --user status git-worktree-audit.timer
systemctl --user start git-worktree-audit.service
journalctl --user-unit git-worktree-audit.service
```

実装の回帰テストは、一時 repository で checkout だけを削除し、検出、通知再試行、
通知重複抑止、prune 後の branch 保持を一連で確認する。

```bash
scripts/git-audit-worktrees --selftest
scripts/git-worktree-create-guard --selftest
scripts/register-codex-worktree-hooks --selftest
```
