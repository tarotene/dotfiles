# plan-fresh-gate — ExitPlanMode 直前のコードベース鮮度ゲート

herdr worktree を並行 Plan モードでパイプライン駆動する運用(片方を Exit
Plan して走らせ、終わったら次を Exit Plan する、という一種の手動スケジュー
リング)では、後発のエージェントは先発の PR が merge された後もセッション
開始時点の古いコードベースを見たままプランを書き、そのまま承認されてしまう
ことがある。

既存の鮮度担保は [`worktree-fresh-base.sh`](worktree-fresh-base.md)
(SessionStart 限定の pristine ff-only 追従)のみで、長い Plan セッション中
の drift はノーガードだった。この hook(`config/claude/hooks/plan-fresh-gate.sh`,
PreToolUse / ExitPlanMode)はプラン承認の直前でその隙間を塞ぐ。

## 判定は 2 段

### 1) 移動(pristine のときだけ)

`worktree-fresh-base.sh` と同じ 5 条件(branch 非空・branch != base・
`git status --porcelain` が空・ahead==0・behind>0)を満たすときだけ
`git merge --ff-only` で origin/`<base>` へ追従する。base ブランチ自身の
上や非 pristine な worktree は動かさない — base を hook が無断で動かす事故
を避ける、という既存 hook と同じ方針を踏襲する。

### 2) deny 判定(移動の可否とは独立)

移動できたかどうかに関わらず、常に fetch し、`origin/<base>` の進行分
(`from..to`)が変更したファイルと、プラン本文が参照しているファイル
(フルパス部分一致 or basename 部分一致)が交差するかを判定する。交差が
あれば deny する。

交差判定のヒューリスティックはこのリポジトリ内・一般公開の先行実装のいず
れにも見つからなかった新規のものである(先行例なし。探した範囲は
`config/claude/hooks/` と `docs/` を fresh/stale/rebase/drift で grep)。
偽陽性(basename だけの一致で無関係なファイルを交差と誤判定する)のコスト
は「一度余計に再確認する」だけなので、安全側に倒す設計を採った。

交差判定は変更ファイル数に上限を設けない。表示件数だけ 50 件に丸める —
判定自体を打ち切ると、上限の外にある交差を見逃したまま ff してしまい、次回
は `merge-base` が進んで再検出の機会も失われる(Copilot plan-review の
BLOCKER 指摘で修正した設計上の教訓)。ファイル数に対して線形の外部プロセス
起動を避けるため、ファイルごとにループするのではなく `grep -F -o -f` 1 回
でプラン本文中に現れたパターン(フルパス + basename の集合)を洗い出し、
`awk` 1 回でどの変更ファイルがその集合と交差するかを求める。

## pr-gate.sh の advisory 方針からの意図的な逸脱

[`pr-gate.md`](pr-gate.md) の `G_base` は同種の base 遅れを advisory に
留めている。その根拠は「block すると rebase → force-push ループに誘導して
しまう」という点にあるが、この gate にはその根拠が当てはまらない —
deny が要求するのは履歴改変(rebase)ではなく「変更ファイルの再読 + 再
ExitPlanMode」だけであり、次項の SHA 記録によって有限回で収束する。

先行例としては GitHub の "Require branches to be up to date before
merging"(GitHub, "About protected branches",
<https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches>,
取得 2026-09-19)が、マージという承認点の直前で base 追従を強制する同型の
機構として存在する。

## 収束保証: deny 済み SHA のセッション state

deny した時点の `origin/<base>` の SHA をセッション単位の state file
(`~/.claude/plan-fresh-gate/<session_id>.denied_sha`)に記録する。次回判定
の起点(`from`)は:

- state file に記録された SHA が、現在の `origin/<base>` の祖先ならその SHA
- そうでなければ(記録なし、または force-push 等で祖先関係が崩れていれば)
  `git merge-base HEAD origin/<base>`

同じ `origin/<base>` の SHA への再 ExitPlanMode は無条件 allow(`from ==
to` で state file を削除して抜ける)。base がさらに進んだ場合は、前回確認
済みの差分を除いた増分だけを再判定する。これにより:

- pristine ケースは ff で `behind == 0` になり自然収束する。
- 非 pristine ケース(実装途中の再 Plan で ahead>0 / dirty)は ff しないため
  状態を持たないと同じ交差で永久に deny され続けるが、SHA 記録により
  「一度確認した差分」への回帰は起きない。
- 活発な並行マージ下で再確認中にさらに base が進むスターベーションも、
  増分方式なので毎回相対的に小さい差分に留まる。

## branch == base のケース

herdr worktree 並行運用では常に worktree ブランチなのでレアケースだが、
小リポで `main` 直上のまま Plan することもあり得る。この場合、移動
(`ff-only`)は行わないが、deny 判定は通常どおり行う — base 自身を hook が
動かす事故を避けつつ、drift を見た上でのプラン承認は同じく防ぐ。

## 対象範囲

Claude Code のみ。Codex CLI / Copilot CLI には Plan モード / ExitPlanMode
相当のフック点がなく、同等の介入点を作るには別設計(実装開始前の手動コマ
ンド等)が必要になるため、今回はスコープ外とした。

## 使い方

- hook として: `home/modules/claude.nix` の `registerHooks` が
  `PreToolUse`(matcher: `ExitPlanMode`)に登録する。plan-review / plan-view
  / plan-scope-gate / plan-precedent-gate と同じ matcher に 5 つ目のエント
  リとして並ぶ(並列実行、順序は保証されない)。
- 自己検査: `plan-fresh-gate.sh --selftest`(10 ケース: pristine+交差あり →
  ff+deny+state 記録 / pristine+交差なし → ff+allow / dirty+交差あり →
  未ff+deny / deny 後の同一 SHA 収束 / deny 後に base がさらに進んだ増分再
  deny / behind==0 / branch==base / basename のみ一致 / origin/HEAD 未設定
  の fail-open / 変更 200 件超でも上限なく交差検出)。

## スキップ手段

`touch ~/.claude/plan-fresh-gate/skip` または `SKIP_PLAN_FRESH_GATE=1`。

## 縮退

git/jq 不在、fetch 失敗、`origin/HEAD` 未設定、プラン本文が取得できない等
はすべて fail-open(交差判定ができないので pass_through、advisory のみ)。
