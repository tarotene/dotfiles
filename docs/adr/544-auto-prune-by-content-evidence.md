# ADR-544 — worktree/branch の無人削除を「内容保全の証拠」で判定する

- Status: Accepted
- Date: 2026-09-28
- Issue: なし(グリルセッション `/grill-me` から直接起票、No-Issue)

## Context

herdr の並列 worktree 運用で、閉じた worktree とマージ済みブランチが
溜まり続けている。削除は現状すべて手動(`docs/worktree-lifecycle.md`
「自動削除はしない」)で、実際には dotfiles 以外で消化されていない。

グリルで確定した痛みは (a) 毎セッションのノイズ(`pr-gate.sh` の
「[gone] N 本」表示、herdr の stale worktree 通知)と (b) ディスク使用量。

実測(2026-09-28):

| 項目 | 値 |
|---|---|
| `[gone]` ブランチ(`~/.ghr` 配下の複数リポジトリ合計) | 数百本規模 |
| dotfiles の worktree | 15 本(herdr で閉じているのは 11 本、旧 `git audit-worktrees` が拾うのは 1 本だけ) |
| `~/.herdr/worktrees` の合計サイズ | 14 GB(うち1本(detached HEAD、先端が closed PR の head)だけで 11 GB) |
| ghr の一部 checkout | shallow 化していた(原因不明。祖先判定と unique commit 数が狂う) |

dotfiles のローカルブランチ 77 本を先端 SHA で分類した結果:

- 先端 = closed/merged PR の head: 10 本
- 先端が `origin/main` の祖先: 15 本(shallow を解けばさらに約 33 本が該当)
- 無関係、または本当の WIP: 4 本

**`[gone]` かどうかと「内容が保全されているか」は一致しない。** `[gone]`
は「upstream ブランチが削除された」という事実しか語らない —
squash-merge 後の `[gone]` もあれば、PR を作らず直接消されただけの
`[gone]` もある。一方、実際に取り戻せる保証があるのは次のいずれかの
場合だけである: (1) 内容が `origin/<default>` に既に入っている、また
は (2) 内容が GitHub 上の PR の head として `refs/pull/<N>/head` に
残っている(PR が CLOSED になっても、GitHub 側でこの ref は消えない)。

## Decision

### D1: 削除の根拠を「内容保全の証拠」の閉集合に限定する

`[gone]`/upstream tracking 状態を無人削除の根拠にせず、次の3クラス
(C1/C2/C3)のいずれかを満たす場合に限って自動削除の対象とする。

- **C1**: prunable な worktree 登録(checkout ディレクトリが既に無い
  — 保全すべき内容自体が存在しない)
- **C2**: 先端が `origin/<default>` の祖先(main に入っている)
- **C3:#N**: 先端が MERGED または CLOSED な PR の `headRefOid` と一致
  (`refs/pull/<N>/head` から復元可能)

対抗馬(実装前に比較): [gh-poi](https://github.com/seachicken/gh-poi)
(README "Safely clean up your local branches" — 取得 2026-09-28)は
「最新コミットで PR を特定し、MERGED/CLOSED なら削除」という原理は
同じだが、削除対象の worktree を `git worktree remove --force` で
無条件に消し(`conn/command.go`)、herdr で開いているか・`git shelve`
が乗っているかを見ない。開いている作業場を消さないという要件を
満たせないため、判定原理(C3 相当)だけを踏襲し、実装は本 ADR の
ガード(D2)を独自に持つ。

軸: 表現不可能性 — 削除対象を証拠クラスの閉語彙で定義し、証拠の無い
削除を表現できなくする。

### D2: worktree の削除には共通ガードを課す

C2/C3 の worktree 候補であっても、次のすべてを満たさない限り削除
候補にしない: Herdr で閉じている(到達不能なら候補にしない、
fail-closed)、clean(未コミットの変更なし)、`git shelve` の退避が
乗っていない、`locked` でない。C1(checkout が既に無い)はこのガードを
通らない — 保全すべき内容自体が無いため。

このガードは検出専用の `git audit-worktrees`(`--notify`/`--context`/
`--porcelain` の既存経路)が使う `scan_orphaned` の安全策
(`worktree_closed_and_clean`)を、`--evidence` モードと共有する
単一正本にした。

軸: 表現不可能性(単一正本 > 複写+同期)— 同じガードを2箇所に複写
しない。

### D3: 判定は検出側(`git audit-worktrees --evidence`)に集約し、削除は
`git prune-worktrees --auto` / `git prune-branches --auto` だけが行う

`docs/worktree-lifecycle.md` の既存の責務分離(検出=audit / 削除=
prune-*)をそのまま延長する。`gh pr list` の呼び出しは `--evidence`
経路にのみ存在し、1 分間隔の `--notify`/`--context`/`--porcelain`
経路は今回も `gh` を一切呼ばない。

### D4: `gh` 呼び出しが失敗したリポジトリは C3 を単に判定しない
(fail closed)

origin が github.com でないリポジトリ、または `gh pr list` 自体が
失敗したリポジトリでは、C3 判定を行わない。誤って削除するより、
削除しないほうに倒す — 既存の audit の「判定できなければ候補にしない」
という方針(`scan_orphaned` のコメント)と同じ。

### D5: shallow なリポジトリでは C2(祖先)判定を行わない

shallow なリポジトリでは `merge-base --is-ancestor` の判定結果を
信頼できない(実測で shallow 化した checkout の unique commit 数が
異常値になったことを確認した)。`git rev-parse --is-shallow-repository`
が true のリポジトリでは C2 判定そのものをスキップする。

軸: 検出のみ — shallow 状態はリポジトリ外の要因で作られるため、
表現不可能にはできない。

### D6: トリガーは 1 分間隔の検出 timer とは別の、1 時間間隔の timer

`git-audit-worktrees.timer`(1分、検出のみ)とは別に
`git-auto-prune.timer`(`OnCalendar=hourly`、`Persistent=true`)を
新設する。`gh` を呼ぶ経路を毎分の検出経路から分離し、通知経路の
API 依存を増やさないため。

軸: 還元性 — 既存の1分 timer に混ぜると、検出専用だったはずの経路が
`gh` 依存を持つことになり、責務が混ざる。

### D7: 削除の記録は追記専用の TSV ログに残し、削除ごとの通知は出さない

`$XDG_STATE_HOME/git-auto-prune/log.tsv` に、削除のたびに
`timestamp, kind, common, repo, path, branch, sha, evidence` を
追記する。Herdr 通知は出さない — auto-prune の目的そのものが
「人間が毎回見なくて済むようにする」ことであり、削除ごとに通知したら
本末転倒になる。`gh`/Herdr への到達性が失われている場合は、既存の
`#89` の教訓(`reason=disabled` の恒常的な設定不備は無音リトライに
せず unit を fail させる)と同じ方針で、unit の失敗として
`systemctl --user --failed` / journal に可視化する。

軸: 検出のみ — 誤削除そのものは表現不可能にできない(判定は外部の
GitHub の状態に依存する)。そのため取り戻し経路(C2 は main に、C3 は
`refs/pull/<N>/head` に残る)をログで担保する。

### D8: 実装は新しい Rust crate ではなく既存の bash 3本を拡張する

`scripts/git-audit-worktrees` / `scripts/git-prune-worktrees` /
`scripts/git-prune-branches` を拡張する。ADR-0024 は新規の hook/CLI に
Rust を既定とするが、既存スクリプトは個別の移行 Issue が消化するまで
bash のまま残るという漸進方針(`docs/adr/0024-hook-cli-scripts-target-rust.md`)
に従う。Rust に複写すると、ガード(herdr open・shelve・dirty)の
正本が2つになる。

軸: 還元性 — 既存の判定ロジックを別言語に複写するコストが、得られる
利益(型安全性等)に見合わない。

## Alternatives considered

- **`[gone]` のまま自動化する**: 実装コストは最小だが、実測で
  `[gone]` と内容の保全状態が一致しないことを確認したため不採用
  (Context 参照)。
- **`gh-poi` をそのまま採用する**: 単一リポジトリ前提・herdr 未対応・
  `--force` 無条件のため、D1 で述べた理由により不採用。判定原理のみ
  踏襲。
- **新規 Rust crate(`git-auto-prune`)を新設する**: ADR-0024 に忠実だが、
  audit のフィルタを Rust に複写することになり単一正本が崩れる。D8 で
  棄却。

## Consequences

- `scripts/git-audit-worktrees`: `--evidence` モードを追加(C1/C2/C3
  検出、`gh` 呼び出しはここに限定)。既存の `is_shelved` の grep 依存・
  Herdr 到達性ガードの fail-open バグも同時に修正した(段1)。
- `scripts/git-prune-worktrees`: `--auto` モードを追加(`--evidence` の
  worktree 行を消費し、確認なしで削除)。
- `scripts/git-prune-branches`: `--auto`/`--dry-run`/`--selftest` を
  新規に追加(元は無条件・無テストの1本スクリプトだった)。
- `home/modules/worktree.nix`: `git-auto-prune` の systemd timer
  (Linux)と 2 本の launchd agent(darwin、worktree 用・branch 用を
  分離 — launchd に systemd の複数 `ExecStart=` に相当する機構が無いため)。
- `home/modules/packages.nix`: `git-prune-branches` の配置コメントを
  更新。
- `.github/workflows/ci.yml`: `git-prune-branches --selftest` を追加。
- 導入直後の初回実行で、既知の残骸(dotfiles の worktree 数本、herdr
  worktree の一部)が実際に削除される見込み。timer を有効化する前に
  `--auto --dry-run` で候補を目視確認する運用手順は
  `docs/worktree-lifecycle.md` に残す。
- 自動化しない範囲: 一度も push していない unique commit を持つ
  worktree/branch、push 後に追加された commit、shallow リポジトリでの
  C2 判定。これらは従来どおり `git audit-worktrees` の通知と手動の
  `git prune-worktrees`/`git prune-branches` で扱う。

## 執行点

- scripts/git-audit-worktrees
- scripts/git-prune-worktrees
- scripts/git-prune-branches
- home/modules/worktree.nix

## Verification

- `scripts/git-audit-worktrees --selftest`(`selftest` + `selftest_evidence`
  の2関数、C1/C2/C3・herdr 到達不能・gh 失敗・shallow・detached HEAD を
  網羅)。
- `scripts/git-prune-worktrees --selftest`(`selftest` + `selftest_auto`)。
- `scripts/git-prune-branches --selftest`(`selftest` + `selftest_auto`、
  新規)。
- `nix flake check --all-systems --no-build` で vega/arcturus/altair
  いずれの評価も通ることを確認済み。
- timer を有効にする前に、実機で `git prune-worktrees --auto --dry-run`
  / `git prune-branches --auto --dry-run` を実行し、実測した既知の
  残骸(amelia-8679 等)が候補に出ること、開いている worktree や
  open な PR のブランチが出ないことを確認する(`docs/worktree-lifecycle.md`)。
