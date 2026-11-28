# 親チェックアウトの鮮度を保つ systemd user timer

herdr の Workspace Fork(≒新規 worktree の作成)は親チェックアウトの HEAD をそのまま使い、fetch を挟まない。`crates/worktree-base-sync` はこの問題の一段下 — 「まだ何も積んでいない pristine な worktree」を SessionStart で `origin/<base>` へ fast-forward する — を既に解決している(`docs/agents/worktree-base-sync.md`)。この文書が説明するのはその一段上、**親チェックアウト自身**を常に新鮮に保つための home-manager 管理の systemd user timer である(#78)。

## 安全条件(全て AND、各リポジトリパスごとに fetch 後に判定)

- パスが存在し、git work tree である
- カレントブランチが非空(detached HEAD・rebase 中は触らない)
- カレントブランチが default branch **自身**である(worktree-fresh-base.sh とは逆の条件 — 親チェックアウトは常に main/master に留まる想定であり、それ以外の状態にあるなら揃えるべきはこのスクリプトの仕事ではない)
- `git status --porcelain` が空(作業ツリーがクリーン)
- ahead == 0(自分のコミットが `origin/<base>` に対して 1 つもない)
- behind > 0(origin より遅れている)

この 5 条件を満たすときだけ `git merge --ff-only --quiet origin/<base>` を実行する。`reset --hard` ではなく `--ff-only` を使う理由は worktree-fresh-base.sh と同じで、fetch とチェックの間に何かコミットされても前提条件を原子的に再強制して履歴を壊さず黙って失敗させるためである(TOCTOU 耐性)。

## 移動した PR の残骸は削除せず退避する(#1505)

ファイルを別のディレクトリへ移した PR を fast-forward したあと、旧ディレクトリに未追跡ファイルだけが残ることがある(ビルド成果物、submodule の作業コピー、無視された `.env` など)。2026-10-23 に約 2,700 ファイルが本物の checkout に残り、`main-checkout-guard` の Stop が「作業ツリーが変わった」と block し、本人が `!` 付きのコマンドで消す往復が必要になった。残骸があると以後の同期でも `status --porcelain` が空にならず毎回止まる。

- fast-forward の後、`before..after` で消えたパスの親ディレクトリのうち、**追跡ファイルが 1 つも残っていない**もの(入れ子の内側は外側に含める)を削除せず、`${XDG_STATE_HOME:-~/.local/state}/claude/git-checkout-sync/leftovers/<repo 名>-<UTC 時刻>/<元の相対パス>` へ丸ごと移す(rename)。移した先は stderr の `MOVED` 行に出力する。
- 削除(`git clean -ffdx`)にしない理由: `.gitignore` は `*.pem`・`*.secret`・`secrets/`・`.env` を無視するため、旧ディレクトリに追跡ファイルがないことは残りが捨ててよい残骸である証拠にならない(Copilot の計画レビュー指摘、2026-11-10)。退避であればデータは失われない。
- 追跡ファイルが 1 つでも残るディレクトリには触れない。移せなかったとき(退避先を作れない、別ファイルシステム)はそのまま残し、stderr の `LEFT` 行で知らせる(exit 0 のまま)。退避先の掃除は本人の判断に委ね、機構側では消さない。
- テスト用に `GIT_CHECKOUT_SYNC_LEFTOVERS_DIR` で退避先の親を差し替えられる。

## 縮退方針: スキップは失敗ではない

各リポジトリパスは独立に処理され、1 つで何が起きても他のパスの処理を止めない。
「dirty」「default branch 以外」は日常的に起きる正常な状態であり、systemd の unit を failed 扱いにするような異常ではないため、`git-checkout-sync` 自体は usage エラー以外では常に exit 0 を返す。スキップ理由は stderr に流すだけとする(`journalctl --user -u git-checkout-sync` で追える)。

## allowlist は明示リスト

`home/modules/worktree.nix` の `checkoutFreshnessPaths` に列挙する。公開リポジトリが固定で持つのは dotfiles 自身だけで、私的なリポジトリのパスは `dotfiles.checkoutFreshness.extraPaths`(`home/modules/checkout-freshness.nix`)に private wrapper flake が注入する(ADR-0034)。
`~/.ghr` 配下をディレクトリスキャンする設計は採らなかった — ghr の管理下には
このホストが鮮度を保つ責務を持たない他のリポジトリも多数あり得るため、対象を
明示的に選ぶ方が事故が少ない。このリポジトリが持つエントリはこの dotfiles 自身の
親チェックアウト(`~/.ghr/github.com/tarotene/dotfiles`)だけ。社内 wiki のミラー
(`docs/agents/esa-mirror-context.md`)の clone は option 経由で追加される。

## タイマー間隔: 10 分

`git-audit-worktrees` の 1 分間隔(読み取り専用スキャン)とは異なり、このタイマーは実行のたびにリポジトリパスの数だけ `git fetch` を打つ。頻度を抑えた 10 分間隔を採用した — 次の Workspace Fork までに 10 分以上古い状態が続くことは稀な一方、origin への通信頻度は 1/10 に抑えられる。

## 参照

- 一段下(worktree 側)の対応: `docs/agents/worktree-base-sync.md`
- 実装: `crates/git-checkout-sync`(`--selftest` あり)
- 配線: `home/modules/worktree.nix`(systemd.user.services/timers、darwin 向けの launchd.agents も同一ファイルに定義)
