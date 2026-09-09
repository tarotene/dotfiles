# pristine worktree の自動 fast-forward — herdr Workspace Fork の鮮度問題

herdr の Workspace Fork(≒新規 worktree の作成)は親チェックアウトの HEAD を
そのまま使い、fetch を挟まない。親を開いたまま何度も fork すると、新しい
worktree が origin より何コミットも遅れた base から生まれる。既存の
`pr-gate.sh` はこれを SessionStart の advisory(「base 追従: origin/main から
N コミット遅れています」)で伝えるが、**ブランチ自体は動かさない** —
履行歴を持つブランチを無断で動かすのは事故なので、これは意図した非対称
だった。

一方で「まだ何も積んでいない、正真正銘そのままの worktree」は履行歴を持たず、
動かしても失うものが無い。この hook(`config/claude/hooks/worktree-fresh-base.sh`,
SessionStart)はその一点だけを能動的に解消する: 条件を満たす worktree に限り
`git merge --ff-only` で origin/`<base>` へ黙って揃える。

## 安全条件(全て AND)

- `git branch --show-current` が非空(detached HEAD・rebase 中は触らない)
- カレントブランチが default branch(`origin/HEAD` が指すもの)自身ではない
- `git status --porcelain` が空(作業ツリークリーン)
- `ahead == 0`(自分のコミットが origin/`<base>` に対して 1 つも無い)
- `behind > 0`(origin より遅れている)

この 5 条件を fetch 後に満たすときだけ `git merge --ff-only --quiet
origin/<base>` を実行する。`reset --hard` ではなく `--ff-only` を使う理由は、
fetch とチェックの間に何かコミットされても前提条件を**原子的に再強制**して
黒歴史を作らず黙って失敗する点にある(TOCTOU 耐性)。

## base 検出は `origin/` 接頭辞を剥がす

`git symbolic-ref --short refs/remotes/origin/HEAD` は `origin/main` のような
値を返す。これをそのまま `origin/<base>` に補間すると `origin/origin/main`
という存在しない ref を参照し、ahead/behind 判定も merge も常に失敗する。
`pr-gate.sh:default_branch()` と同一の `${ref#origin/}` で接頭辞を剥がした値
を `default_branch()` の複製として持つ(変更時は両方揃えること)。

## fetch は base ブランチ名を明示する

`git fetch origin <base>` は `git fetch origin`(引数なし)と違って通信量が
小さく、かつ実機検証済みで `refs/remotes/origin/<base>` を正しく更新する
(clone 時に張られる既定の fetch refspec `+refs/heads/*:refs/remotes/origin/*`
に `<base>` が含まれるため)。fetch 済みかどうかは共通 git ディレクトリの
`FETCH_HEAD` の mtime を TTL(既定 600 秒、`WORKTREE_FRESH_BASE_FETCH_TTL` で
上書き可)として判定し、pr-gate.sh の SessionStart と同じパターンを踏む。

## 順序について: pr-gate との実行順は保証されない

「この hook が先に origin/`<base>` へ揃えてから pr-gate.sh SessionStart の
base 追従 advisory を計算する」のが理想だが、Claude Code は同一イベントの
hook を並列実行するため逐次実行は保証できない
(`home/modules/claude.nix` の plan-view の項に同じ注記がある)。

この hook はレースを許容する: 動かした場合だけ自分の `additionalContext` で
「他 hook の advisory はこの更新前の状態を見ている可能性がある」と明示し、
判断の材料は運ぶが、pr-gate 側の表示を待ち合わせたり書き換えたりはしない。
影響は cosmetic(base 追従の behind 表示が 1 セッション古いだけ)に留まる。

## 使い方

- hook として: `home/modules/claude.nix` の `registerHooks` が SessionStart
  (`startup|resume`)に登録する。matcher に `compact` を含めないのは
  sign-prewarm と同じ理由 — 同一プロセス内の再発火に価値がない。
- 自己検査: `worktree-fresh-base.sh --selftest`(6 ケース: FF 成功 / dirty /
  ahead>0 / behind==0 / detached / origin/HEAD 未設定)。ホストの
  user/global git 設定から隔離するため `GIT_CONFIG_GLOBAL=/dev/null` /
  `GIT_CONFIG_SYSTEM=/dev/null` を export する(#56/#59 の教訓 — selftest が
  その場のマシン状態に汚染されないように)。

## スコープ外(別 Issue)

親チェックアウト自身の `main` を常に新鮮に保つ(home-manager 管理の systemd
user timer で定期 fetch + ff-only pull する)案は、全リポジトリへの常駐動作
という別議論が要るため、この hook のスコープには含めない。
