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
| `git audit-worktrees` | 検出のみ(上流追跡の2クラス + 内容保全の証拠3クラス)+ timer 通知 + SessionStart context | 読み取り専用、一切削除しない |
| `git prune-worktrees` | prunable な**登録メタデータ**と orphaned な**checkout** を対話確認つきで削除(既定)、または `--auto` で証拠クラスの worktree を確認なしで削除 | `git worktree prune --expire=now`(prunable/C1)/ `git worktree remove`(orphaned・C2・C3、既定では `--force` を使わない) |
| `git prune-branches` | `[gone]` **ブランチ**を対話確認つきで削除(既定、`docs/git-sync.md`)、または `--auto` で証拠クラスのブランチを確認なしで削除 | `git branch -D` |

worktree → branch の順で畳む: `git prune-worktrees` が checkout を消すと、
その worktree に紐付いていた branch も `git prune-branches` の対象に入る
(checkout 中は `git branch -D` が拒否するため)。`--auto` の
`git-auto-prune` timer(下記)もこの順序で2つのコマンドを実行する。

対話確認つきの既定モードは upstream tracking 状態(`[gone]`、unique
commit 数)を根拠にする。`--auto` は別の判断軸(内容保全の証拠、下記)を
根拠にし、確認なしで timer から動く — 根拠の閉集合とその選定理由は
「worktree/branch の無人削除を『内容保全の証拠』で判定する」ADR を参照。

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
```

audit は検出専用で、どちらのクラスも削除しない(かつて `--prune` で prunable
クラスの登録メタデータだけ消せたが、削除は `git prune-worktrees` に一本化した —
検出=audit / 削除=prune-worktrees の単一責務)。

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

`git audit-worktrees --porcelain` の**両クラス**を消費する。一覧表示 → 一度だけの
y/N 確認 → 削除、の流れは共通だが、クラスごとに削除手段が異なる。

- **prunable**(登録メタデータのみ、checkout は既に消えている):
  `git worktree prune --verbose --expire=now` を該当 common directory ごとに
  実行する。git の既定 expire(`gc.worktreePruneExpire`、3 ヶ月)には従わず
  常に `--expire=now` を使う — audit が「消えている」と報告したものは、
  経過時間に関わらず必ず消えることを保証するため。branch ref と commit は残る。
- **orphaned**(checkout が実在): **削除に取り掛かる直前に再スキャンし直し**、
  確認時点から状態が変わった(Herdr で開かれた、push された、`git shelve` が
  乗った等)候補は黙って削除せず「状態が変化したため見送り」として報告する。
  既定では `git worktree remove`(`--force` は使わない) — Git 自身が拒否したら
  その worktree はスキップして報告する。唯一の組み込み例外が submodule を含む
  worktree で、Git は clean かどうかに関係なく plain remove を無条件拒否するため
  (`man git-worktree` remove 節)、自前でより厳格な clean 判定
  (`status --porcelain --ignore-submodules=none` が空、かつ submodule 内容も
  含めて dirty が無いこと)を通った場合に限り `--force` で再試行する。

`--force` フラグを付けると、再検証を通った orphaned 候補全てに対して
`git worktree remove --force` を直接使う。これは **Git 自身の削除拒否
(dirty な内容も含む)だけを押し切る**という意味で、削除直前の再検証
(状態が変わった候補を見送る安全層)はそのまま効き続ける。ブランチには触れない。

```bash
git prune-worktrees              # 一覧 → 確認 → 削除
git prune-worktrees --dry-run    # 一覧のみ、削除しない
git prune-worktrees --yes        # 非対話実行(fixture や自動化向け)
git prune-worktrees --force      # orphaned で Git 自身の削除拒否を押し切る
```

## 無人削除: `--evidence` と `--auto`

対話確認つきの既定モードとは別に、**内容が保全されている証拠**だけを
根拠にした無人削除の経路がある。根拠は次の3クラスの閉集合で、
`[gone]` のような upstream tracking 状態は一切使わない。

- **C1**: prunable な登録(checkout が既に無い — 保全すべき内容自体が無い)
- **C2**: 先端が `origin/<default>` の祖先(main に入っている)
- **C3:#N**: 先端が MERGED/CLOSED な PR の `headRefOid` と一致
  (`refs/pull/<N>/head` から復元可能)

C2/C3 の worktree 候補は、orphaned クラスと同じ安全ガード(Herdr で
閉じている・clean・shelve 無し)に加えて `locked` でないことも要求する。
C1 はこのガードを通らない(checkout 自体が既に無い)。shallow な
リポジトリでは C2 判定を行わない(祖先判定を信頼できないため)。

```bash
git audit-worktrees --evidence   # TSV: kind common repo path branch sha evidence
git prune-worktrees --auto        # --evidence の worktree 行を確認なしで削除
git prune-worktrees --auto --dry-run
git prune-branches --auto         # --evidence の branch 行を確認なしで削除
git prune-branches --auto --dry-run
```

`--evidence` は C3 判定のために `gh pr list` をリポジトリごとに1回呼ぶ
(結果はプロセス内でキャッシュする)。`--notify`/`--context`/`--porcelain`
(1分間隔の検出 timer が使う経路)は `gh` を一切呼ばない。origin が
github.com でないリポジトリ、または `gh` 呼び出し自体が失敗した
リポジトリでは、C3 判定を単に行わない(fail closed)。

`--auto` は削除直前に `--evidence` を再スキャンし、一覧時点と完全一致
しない行は「状態が変化したため見送り」として削除しない(既定モードの
TOCTOU 再検証と同じ設計)。削除のたびに
`$XDG_STATE_HOME/git-auto-prune/log.tsv` へ1行追記する(タイムスタンプ・
repo・path・branch・sha・証拠クラス)。Herdr 通知は出さない — 無人削除の
目的が「毎回人間が見なくて済むようにする」ことなので、削除ごとに通知したら
本末転倒になる。`gh`/Herdr へ到達できない場合は unit 自体を失敗させる
(`#89` と同じ、無音リトライにしない)。

Home Manager は `git-auto-prune.timer`(1時間間隔、`OnCalendar=hourly`)を
有効化し、`git prune-worktrees --auto` → `git prune-branches --auto` の
順で実行する(worktree を先に畳むと、その branch が同じ実行で
prune-branches の対象に入るため)。1分間隔の検出 timer とは別の timer
にしているのは、`gh` を呼ぶ経路を検出専用の経路から分離するため。

設計判断の全体(なぜ `[gone]` を根拠にしないか、gh-poi との比較、ROI)は
「worktree/branch の無人削除を『内容保全の証拠』で判定する」ADR
(`docs/adr/`)を参照。

## 確認

```bash
systemctl --user status git-audit-worktrees.timer
systemctl --user start git-audit-worktrees.service
journalctl --user-unit git-audit-worktrees.service

systemctl --user status git-auto-prune.timer
systemctl --user start git-auto-prune.service
journalctl --user-unit git-auto-prune.service
cat "${XDG_STATE_HOME:-$HOME/.local/state}/git-auto-prune/log.tsv"
```

timer を有効化する前に、必ず `--auto --dry-run` で候補を目視確認する:

```bash
git prune-worktrees --auto --dry-run
git prune-branches --auto --dry-run
```

実装の回帰テストは、一時 repository と実物のベア remote を使い、prunable/orphaned
両クラスの検出・除外条件(dirty・shelve・unique commit・herdr open)・通知再試行・
通知重複抑止を一連で確認する(audit は検出専用になったため、git 本体の
`worktree prune` で登録が消え branch は残ることも同じ selftest 内で直接確認する)。
`--evidence`(C1/C2/C3・herdr 到達不能・gh 失敗・shallow・detached HEAD)は
`selftest_evidence` という別関数で確認する(`git-audit-worktrees --selftest`
が両方を続けて実行する)。
`git-prune-worktrees`/`git-prune-branches` それぞれも、既定の対話確認モード
(スタブの audit 結果を使い、削除・確認ゲート・`--dry-run`・削除直前の
再検証(TOCTOU)・`--force` によるコミット拒否の押し切りを確認する
`selftest`)と、`--auto` モード(スタブの `--evidence` 結果を使い、
C1/C2/C3・`--dry-run`・TOCTOU・log.tsv への記録を確認する `selftest_auto`)
の2関数を続けて実行する。

```bash
scripts/git-audit-worktrees --selftest
scripts/git-prune-worktrees --selftest
scripts/git-prune-branches --selftest
scripts/git-worktree-create-guard --selftest
scripts/register-codex-hooks --selftest
```
