# ADR-0000 — 本物の checkout(main worktree)を変更させない

- Status: Accepted
- Date: 2026-10-02
- Issue: No-Issue(`/grill-me` セッション中の問いから始まった。「よそのリポジトリで作業する
  とき、本物の non-worktree で作業を始めてしまうことがある。作業単位を切りたくなったときに
  ローカルが main であることを保証できる場がないと困るので、lateral movement するときも
  worktree を切ることを強制したい。これってルールになってますか？」)

## Context

問いへの答え: **ルールになっていなかった。** 近い仕組みはあったが、どれも強制ではない。

- `git-worktree-create-guard`: 素の `git worktree add` を `herdr worktree create` へ誘導するだけ。
  worktree を切ること自体は求めていない。
- `config/git/hooks/pre-commit`(global `core.hooksPath`): main/master への直接 commit だけを止める。
  本物の checkout で `git switch -c` してから編集・commit する流れは止めない。この流れこそ
  「親 checkout が main でなくなる」事故そのもの。
- `worktree-fresh-base` / `git-checkout-freshness`: worktree の内側、または dotfiles の親
  checkout 1 つしか見ない。
- 共有 AGENTS.md に規範文はなかった。

herdr は作業単位ごとに worktree を作り、その土台に親 checkout の HEAD を使う(fetch は挟まない)。
親 checkout が main でクリーンであることは、独立した作業単位を切る前提そのもの。よそのリポジトリ
でセッションを本物の checkout で始めたり、作業の途中で本物の checkout へ移って編集したりすると、
その前提が崩れる。#661 が示したとおり、Write/Edit を止めるだけでは Bash 経由で迂回される。

## Decision

- **D1 対象**: どの本物の checkout にも変更を入れない。「自リポジトリ」と「よそ」を区別しない
  (herdr で作業する都合上、リポジトリの境界しか意味を持たない)。セッションが本物の checkout で
  起動した場合も同じ。
- **D2 判定**: 本物の checkout は `git rev-parse` の `--git-dir` と `--git-common-dir` が一致する
  main worktree。置き場所(`~/.ghr` 等)は問わない。linked worktree は対象外。
- **D3 PreToolUse**: 本物の checkout への Write/Edit/MultiEdit/NotebookEdit と、変更系の git
  (switch/checkout/commit/reset/merge/rebase/cherry-pick/revert/add/rm/mv/restore/am/apply/clean、
  stash(list/show 以外)、`pull`(`--ff-only` 以外)、ブランチを作る・動かす・消す `branch`)を deny する。
  `-C`・`--work-tree`・`cd` で向き先を畳み込む。読み取り系 git、`fetch`、`pull --ff-only`、
  `worktree list|prune` は通す。
- **D4 Stop**: 本物の checkout に初めて触れた時点(Read/Grep/Glob、読み取り系 git、Bash の cwd・
  絶対パス引数)で baseline(ブランチ・HEAD・`git status --porcelain=v2` のハッシュ)を
  セッション別に記録し、Stop で記録した checkout だけを再確認する。触れた時点でクリーンな
  default branch にいたものが、ブランチ変更・作業ツリー変更・非 fast-forward の HEAD 移動をしていたら
  block する。クリーンのまま HEAD が前進しただけ(`pull --ff-only`)は通す。報告した checkout は
  baseline を現在の状態へ更新し、次のターンで繰り返さない。Bash の `sed -i` やリダイレクトによる
  迂回は、字面ではなく状態で捕まえる。
- **D5 既に崩れていた checkout**: 触れた時点で dirty、または default branch 以外だった checkout は
  block しない(人の作業や別セッションの残骸をこのセッションの責任にしない)。代わりに
  `additionalContext` で、新しい作業単位は worktree で切るよう 1 回だけ伝える(Claude のみ)。
- **D6 逃げ道なし**: env も ask も設けない。直接作業したいときは、本人が `!` 付きのコマンドで
  自分で操作する(hook を通らない)。エージェントが自分で逃げ道を実行する状態を表現できなくする。
- **D7 Claude と Codex**: 規範文は共有の `config/agents/AGENTS.md` に置き、判定エンジンは 1 つ
  (`crates/main-checkout-guard`)。両方に登録する。Codex の編集ツール(`apply_patch`)は
  PreToolUse の対象外で、Stop の事後検出が受ける。

## Alternatives considered

- **cwd が本物の checkout にある Bash を、読み取り専用の許可リスト以外すべて deny する**:
  許可リストの保守と誤検知が重い。同じ「状態を崩させない」を、状態の検査で満たせる。
- **規範文だけで扱う**: 感触で外した(同じ事故が既に繰り返されている)。分析ではない。
- **git の hook(post-checkout 等)**: エージェントの Write/Edit を捕まえられず、事前の deny も
  できない。
- **SessionStart で `~/.ghr` 配下の全 checkout を記録する**: 並行している別のセッションや人の変更まで
  拾い、このセッションを block しうる。触れたものだけを記録する方を採った。
- **`~/.ghr` 配下だけを対象にする**: 別の場所へ clone した本物の checkout が穴になる。

## Consequences

- 本物の checkout でのエージェントの編集・ブランチ操作は、すべて worktree 経由になる。
- 既知の限界(検出のみ): Bash が絶対パスを引数に取らず、cwd も向き先でない書き込み
  (例: `cd` を伴わない `make -C <dir>` や、環境変数で向き先を指すもの)は、触れたことを
  推定できず baseline が記録されない。Codex の `apply_patch` は PreToolUse で止められない。
- Bash 1 回あたり、cwd と最大 8 個の絶対パス引数について `git rev-parse` を呼ぶ(実測で数 ms)。
- 先行例: 同種の「本物の checkout を守る」hook は見つからなかった。構成部品は
  `crates/git-worktree-create-guard`(global-opts の解釈)、`crates/wrapup-stop-gate`(Stop block)、
  `crates/worktree-fresh-base`(pristine 判定)の既存実装に拠る。

## 執行点

- `crates/main-checkout-guard/src/lib.rs`
- `crates/main-checkout-guard/tests/cli.rs`
- `home/modules/claude.nix`
- `config/agents/AGENTS.md`

## Verification

- `cargo test -p main-checkout-guard` — deny/allow の境界、baseline、Stop の事後検出。
- `nix build .#homeConfigurations.vega.activationPackage --no-link` — hook の配備と登録が通る。
