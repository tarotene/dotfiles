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

もう一つ、入口を揃えても残る問題がある: Herdr は workspace を閉じても worktree
checkout を削除しない(v0.8.2 時点、`herdr worktree remove` は `--workspace <ID>`
必須で、閉じた後の残骸には使えない — upstream にその cleanup 手段自体が無い)。
実測でマシン全体(全リポジトリ)で 100 件超・数十 GB の checkout が据え置きに
なっていた。これは「checkout ごと残る」問題で、以下の登録メタデータ監査とは
別クラスの検出・削除が要る。

## 責務の対称形

| コマンド | 責務 | 破壊度 |
|---|---|---|
| `git audit-worktrees` | 検出のみ(2 クラス)+ timer 通知 + SessionStart context | 読み取り専用(`--prune` は登録メタデータのみ削除) |
| `git prune-worktrees` | 残骸 **checkout** の対話確認つき削除 | `git worktree remove`(`--force` は使わない) |
| `git prune-branches` | `[gone]` **ブランチ**の対話確認つき削除(`docs/git-sync.md`) | `git branch -D` |

worktree → branch の順で畳む: `git prune-worktrees` が checkout を消すと、
その worktree に紐付いていた branch も `git prune-branches` の対象に入る
(checkout 中は `git branch -D` が拒否するため)。

## 検出: `git audit-worktrees`

`~/.ghr` 配下の repository と、`~/.herdr/worktrees` から参照される common
repository を重複なく走査し、2 つの独立したクラスを検出する。

- **prunable** — 登録は残っているが checkout ディレクトリが消えている
  (`git worktree list --porcelain` 自身の `prunable` 判定)。
- **orphaned** — checkout は存在するが、次の条件を**すべて**満たす:
  clean(未コミットの変更が無い)、Herdr で現在開いていない、
  `git shelve` の退避が乗っていない、かつ upstream が `[gone]`
  **または** upstream 未設定かつ default branch(`origin/HEAD`、無ければ
  `main`/`master`)に対して unique commit が 0(herdr が生成したまま
  何も積まれなかった worktree)。

判定できない場合は必ず「候補にしない」側に倒す(Herdr のソケットに繋がらない、
detached HEAD、dirty、実際の upstream がまだマージされていないだけ、等)。

```bash
git audit-worktrees                # 読み取り専用。検出時は終了コード 1
git audit-worktrees --porcelain    # class 付き TSV(git-prune-worktrees が消費)
git audit-worktrees --prune        # prunable な登録だけを一覧 → 確認 → 削除
```

`--prune` が削除するのは prunable クラスの登録メタデータだけで、branch ref と
commit は残る。orphaned クラスには一切触れない(そちらは `git prune-worktrees`
の責務)。非対話実行での `--prune` は誤操作を避けるため拒否し、fixture や明示的な
自動化だけが `--yes` を併用できる。

Home Manager は `git-audit-worktrees.timer` を有効化し、1分間隔で同じ監査を行う
(旧名 `git-worktree-audit` はコマンド名と語順が逆で打ち間違いの元だったため改名)。
新しい stale 集合を見つけると Herdr に通知するが、自動削除はしない。同じ集合は
通知済み fingerprint として XDG state directory に記録する。Herdr に foreground
client がない、toast が busy などで表示されなかった場合は通知済みにせず次回
再試行する。**この通知経路は Herdr 側の `[ui.toast].delivery` が `"off"`(既定値)
だと `{"shown":false}` を返し続け、誰にも見えないまま無音でリトライし続ける** —
`config/herdr/config.toml` は `delivery = "herdr"`(in-app トースト)を明示的に
設定してこれを避けている。

Claude/Codex の SessionStart でも監査し、残留があれば agent context に一覧を渡す。
Codex は `~/.codex/hooks.json` の新しい command を初回だけ信頼確認する。Home Manager の
マージは Herdr integration を含む既存 hook を保持する。

## 削除: `git prune-worktrees`

`git audit-worktrees --porcelain` の orphaned クラスだけを消費する。一覧表示 →
一度だけの y/N 確認 → **削除に取り掛かる直前に再スキャンし直し**、確認時点から
状態が変わった(Herdr で開かれた、push された、`git shelve` が乗った等)候補は
黙って削除せず「状態が変化したため見送り」として報告する。削除は
`git worktree remove`(`--force` は使わない) — Git 自身が拒否したらその
worktree はスキップして報告する。唯一の例外が submodule を含む worktree で、
Git は clean かどうかに関係なく plain remove を無条件拒否するため
(`man git-worktree` remove 節)、自前でより厳格な clean 判定
(`status --porcelain --ignore-submodules=none` が空、かつ submodule 内容も
含めて dirty が無いこと)を通った場合に限り `--force` で再試行する。dirty な
submodule はこの厳格判定に落ちるため従来通りスキップされる。ブランチには
触れない。

```bash
git prune-worktrees              # 一覧 → 確認 → 削除
git prune-worktrees --dry-run    # 一覧のみ、削除しない
git prune-worktrees --yes        # 非対話実行(fixture や自動化向け)
```

## 確認

```bash
systemctl --user status git-audit-worktrees.timer
systemctl --user start git-audit-worktrees.service
journalctl --user-unit git-audit-worktrees.service
```

実装の回帰テストは、一時 repository と実物のベア remote を使い、prunable/orphaned
両クラスの検出・除外条件(dirty・shelve・unique commit・herdr open)・通知再試行・
通知重複抑止・prune 後の branch 保持を一連で確認する。`git-prune-worktrees` 側は
スタブの audit 結果を使い、確認ゲート・`--dry-run`・削除直前の再検証(TOCTOU)を
確認する。

```bash
scripts/git-audit-worktrees --selftest
scripts/git-prune-worktrees --selftest
scripts/git-worktree-create-guard --selftest
scripts/register-codex-worktree-hooks --selftest
```
