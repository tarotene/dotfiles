# herdr-status-advisory — herdr の client と server の版の食い違いを、作業ツリーを切る前に知らせる

## なぜ必要か

herdr の client だけが更新され、server が古い版のまま残ると、`herdr worktree create` が
次のエラーで失敗する。

```
protocol_mismatch: client protocol 22 is newer than server protocol 20; restart the Herdr server
```

作業ツリーを切る唯一の許可経路(`main-checkout-guard` が素の作業ツリー作成を拒否する)が
使えなくなり、別リポジトリへの変更を伴う作業が止まる。問題は 2 つある。

- 気づくのが、作業ツリーが要る瞬間になってから。そのとき本人の操作(server の再起動)を待つしかない。
- server の再起動は、動作中の全ペインのプロセスを終了させる。エージェントが自分で復旧する手段はない。

`hms` の警告(`crates/hms` の `check_herdr_staleness`、#200)は、`hms` を実行した直後の 1 回だけで、
セッションを開始したときには何も出ない。この hook は、その隙間を埋める。

## 何をするか

`crates/herdr-status-advisory`(SessionStart、matcher `startup|resume`)が、`herdr status --json` を呼んで読む。

- `server.running` が真で、`server.compatible` または `server.endpoint_compatible` が偽: 食い違いを
  `additionalContext` で伝える。client と server の protocol 番号、`herdr worktree create` が失敗すること、
  復旧が本人の操作であること、再起動の手順と副作用(全ペインの終了)を書く。
- 互換だが `restart_needed` または `server_binary_stale` が真(`server` と `update` の両方を見る): 更新後の版に
  入れ替わっていないことを、弱い注意として伝える。
- それ以外(server が動いていない、`herdr` が無い、`herdr status` が失敗・タイムアウト、JSON を読めない): 何も出さない。
  全失敗経路は fail-open。

hook は何も実行しない。`herdr server stop` は全ペインを落とすので、自動復旧は設けない。再起動の手順は
`docs/operations.md` の「Restarting herdr after a switch that changes its binary or hooks」にあり、この hook
はそこへ誘導するだけ。

## 照合を自前で書かない理由

`herdr --version` が返すのは client の版だけで、server の版は socket 越しにしか取れない。それをやるのが
`herdr status` そのもので、`--json` なら機械可読に `client.protocol`・`server.protocol`・`server.compatible`・
`server.endpoint_compatible`・`server.restart_needed`・`server_binary_stale` を返す(herdr 0.9.3 の実出力で確認、
2026-10-11)。herdr に同梱の `herdr --skill` も「更新後は client と server の版が食い違いうる。新しい server の
機能を使う前に `herdr status` を確かめる。メソッドが無いことは、server を止めたり更新したりする許可ではない」と
書いている(同、取得 2026-10-11)。

`hms` の検査(#200)は、稼働中の server プロセスが指す store path と新しい世代の store path を比べる。この hook は
herdr 自身の互換判定を使う。両者が同じ条件で鳴るかは確かめていない(未確認)。

## 既存手段と代替

- 既存手段: 拡張 — `crates/worktree-fresh-base` の SessionStart hook の型(fail-open、動いたときだけ
  `additionalContext`)。照合は採用 — `herdr status --json`。
- 対抗馬: `docs/operations.md` に追記するだけの散文案。再起動手順と副作用は既に書かれているので、足りないのは
  「気づく時点」で、散文では解消しない。
- 軸: 検出のみ。server の再起動が全ペインを落とすので、hook が復旧まで還元できない。

## 登録

- `home/modules/claude.nix`: SessionStart hook(Claude)、`home.file` の配備、Codex の SessionStart 登録。
- 試験: `crates/herdr-status-advisory/tests/cli.rs` が、PATH の先頭に置いた偽の `herdr` で実バイナリを動かす
  (互換なら無出力、食い違いなら文面、`herdr` が無い・失敗・不正な出力・ハングなら無出力)。
