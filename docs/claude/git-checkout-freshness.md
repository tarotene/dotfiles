# 親チェックアウトの鮮度を保つ systemd user timer

herdr の Workspace Fork(≒新規 worktree の作成)は親チェックアウトの HEAD を
そのまま使い、fetch を挟まない。`config/claude/hooks/worktree-fresh-base.sh`
はこの問題の一段下 — 「まだ何も積んでいない pristine な worktree」を
SessionStart で origin/`<base>` へ fast-forward する — を既に解決している
(`docs/claude/worktree-fresh-base.md`)。この doc が説明するのはその一段上、
**親チェックアウト自身**を常に新鮮に保つための home-manager 管理の
systemd user timer(#78)。

## 安全条件(全て AND、各 repo パスごとに fetch 後に判定)

- パスが存在し、git work tree である
- カレントブランチが非空(detached HEAD・rebase 中は触らない)
- カレントブランチが default branch **自身**である(worktree-fresh-base.sh
  とは逆の条件 — 親チェックアウトは常に main/master に留まる想定であり、
  それ以外の状態にあるなら、揃えるべきはこのスクリプトの仕事ではない)
- `git status --porcelain` が空(作業ツリークリーン)
- ahead == 0(自分のコミットが origin/`<base>` に対して 1 つも無い)
- behind > 0(origin より遅れている)

この 5 条件を満たすときだけ `git merge --ff-only --quiet origin/<base>` を
実行する。`reset --hard` ではなく `--ff-only` を使う理由は
worktree-fresh-base.sh と同じ: fetch とチェックの間に何かコミットされても
前提条件を原子的に再強制して黒歴史を作らず黙って失敗する(TOCTOU 耐性)。

## 縮退方針: スキップは失敗ではない

各 repo パスは独立に処理され、1 つで何が起きても他のパスの処理を止めない。
「dirty」「default branch 以外」は日常的に起きる正常な状態であり、systemd
の unit を failed 扱いにするような異常ではないため、`git-checkout-freshness`
自体は usage エラー以外では常に exit 0 を返す。スキップ理由は stderr に
流すだけ(`journalctl --user -u git-checkout-freshness` で追える)。

## allowlist は明示リスト

`home/modules/worktree.nix` の `checkoutFreshnessPaths` に列挙する。
`~/.ghr` 配下をディレクトリスキャンする設計は採らなかった — ghr の管理下には
このホストが鮮度を保つ責務を持たない他の repo も多数あり得るため、対象を
明示的に選ぶ方が事故が少ない。現状の唯一のエントリはこの dotfiles 自身の
親チェックアウト(`~/.ghr/github.com/tarotene/dotfiles`)。

## タイマー間隔: 10 分

`git-audit-worktrees` の 1 分間隔(読み取り専用スキャン)とは異なり、この
タイマーは実行のたびに repo パスの数だけ `git fetch` を打つ。頻度を
落とした 10 分間隔を採用した — 次の Workspace Fork までに 10 分以上
古い状態が続くことは稀な一方、origin への通信頻度は 1/10 に抑えられる。

## 参照

- 一段下(worktree 側)の対応: `docs/claude/worktree-fresh-base.md`
- 実装: `scripts/git-checkout-freshness`(`--selftest` あり)
- 配線: `home/modules/worktree.nix`(systemd.user.services/timers、
  darwin 向けの launchd.agents も同一ファイルに定義)
