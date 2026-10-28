# worktree-lease — 1 つの linked worktree を、同時に 2 つのセッションが git で動かさない

実装は `crates/worktree-lease`。判定はルータ(`crates/hook-router/src/rules.rs` の
`worktree-lease`、PreToolUse の `Bash`)が呼ぶ。`pr-gate` の Stop も同じ印を読む。

## 困りごと(2026-10-27)

- 同じ worktree で 2 つのセッションが同時に git を操作し、一方の後始末(`git switch -`)のあとに
  ブランチが別のものへ動いた。気付けたのは reflog を読んだときだけだった。worktree の占有を示すものが無い。
- `pr-gate` の Stop が、このセッションが作っていない PR・commit を理由に止めた。同じ worktree で別セッションが
  動いており、ブランチ切替・commit・PR の作成が、このセッションのものとして扱われた。

## 仕組み

- **印(lease)**: linked worktree の git-dir(`git rev-parse --git-dir`、例 `.git/worktrees/<名前>/`)に
  `dotfiles-session-lease`(`{"session": "<session_id>", "ts": <epoch 秒>}`)を置く。git-dir の中なので、
  作業ツリーにも `git status` にも出ない。worktree を消せば一緒に消える。
- **更新**: PreToolUse の `Bash` のたびに、そのセッションの lease に更新する(5 秒以内の再更新は省く)。
- **止めるもの**: 別のセッションの lease が `ACTIVE_SECS`(120 秒)以内に更新されているとき、HEAD・ブランチ・
  commit を動かしうる git を deny する: `switch`・`checkout`・`reset`・`rebase`・`merge`・`cherry-pick`・
  `revert`・`commit`・`push`・`pull`・`am`、`branch` の削除・改名・強制。`-C`・`-c`・環境変数の前置・複合コマンド
  の後ろの文も見る。読み取りの git と、git 以外のコマンドは通し、相手の lease には触れない。
- **引き継ぎ**: lease が 120 秒より古ければ、黙って引き継ぐ(自分の lease に書き換える)。セッションの終了を知らせる
  hook が無いので、生きているかは更新時刻だけで見る。長く止まっている相手の後ろで作業が始まると、相手が戻ったとき
  今度は相手が止められる。
- **`pr-gate` の Stop**: 同じ worktree を別のセッションが今使っているとき(`held_by_other`)は、止めない。
  そちらが動かしたブランチ・作った commit・PR を、このセッションのものとして block しない。
- **外す**: `worktree-lease status` で見る。相手が終わっているのに待たされるときは、ユーザーが
  `! worktree-lease release` で外す。エージェントが自分で外さない(deny 文にもそう書く)。
- **対象外**: 本物の checkout(main worktree)。そちらは `main-checkout-guard` の責務で、置く場所が違う。
  セッション ID が不明(`unknown`)のときは、何もしない。

## 限界(検出のみ)

起動時に 1 worktree = 1 セッションを強制するのではない。次は止まらない: 120 秒以上止まっていた相手(誤って
引き継ぐ)、Bash を使わない書き込み(Edit・Write)、git の外の操作。lease を読めない・書けない環境では、
何も止めずに作業を通す(fail-open)。表現不可能にするには、herdr が worktree ごとに 1 エージェントしか
起動させない形が要る(herdr がその一覧を出せるかは未確認)。

## 既存手段と代替

- 既存手段: 自前 — 却下: `main-checkout-guard`。責務が「本物の checkout を変更させない」で、linked worktree は
  対象外と `lib.rs` の冒頭に明記されている。同じ worktree に生きているセッションが 1 つだけ、という別の責務を入れると、
  判定の向きが逆になる。
- 先行例: `crates/stack-base-guard` の、session_id で区切った状態ファイル(`src/lib.rs:391-419`、サブエージェントが
  読んだ範囲)。ここは状態を session でなく worktree の git-dir に置き、別セッションの書き込みを止める点が違う。
- 対抗馬: `git worktree lock`。`prune`・`move`・`remove` を防ぐ仕組みで、同時使用を防ぐかは `-h` の usage しか読んで
  おらず未確認のため採っていない。
- 軸: 検出のみ。

## 検査

- `crates/worktree-lease/src/lib.rs` のテスト: HEAD を動かす git の認識(通す形・止める形)、2 セッションの
  衝突と読み取りの通過、古い lease の引き継ぎ、本物の checkout・Bash 以外・`unknown` の素通り、`held_by_other`、
  `release`。
- `crates/pr-gate/tests/stop.rs`: 別セッションが使っている linked worktree では Stop を止めず、自分の lease なら
  止めること。
