@~/.agents/AGENTS.md

<!-- Claude Code 固有の施行配線。原則そのものは上記 import 先(共有
     AGENTS.md、Codex/Copilot とも共有)を正本とする。ここに残るのは
     Claude Code のツール(ExitPlanMode・AskUserQuestion・PreToolUse/Stop
     hook)に紐づく書式・自己検査手順だけ。 -->

## 先行例確認: Plan mode での書式と自己検査

Plan mode で非自明な設計判断を書くときは、Plan に `## 先行例との対比` 節を
置き、判断ごとに 1 ブロック(`- Dn:` 行と、その下の従属行)で書く。各ブロックに
必要なトークン(`先行例:` + 出典 + `(取得 YYYY-MM-DD)` + `差分:`、または
`先行例なし:`。全ブロックに `軸:`。技術選定なら `本命:`/`対抗馬:`/`既存手段:`)
の正本は precedent-grounding スキル §3 と technology-selection スキル §3 で、
ここには複写しない。gate は従属行の順序・行数を見ず、ブロック内にトークンが
揃っているかだけを見る。設計判断を含まない Plan は代わりに
1 行の免除(`先行例: 該当なし — <理由>`)でよい。**ExitPlanMode を呼ぶ前に
`~/.claude/hooks/plan-precedent-gate --check <プランファイル>` で
自己検査して指摘ゼロを確認する**(gate の deny 往復を待たない)。

各 `Dn` には、上記に加えて `軸:`(`表現不可能` / `還元` / `検出のみ` の
いずれか、共有 AGENTS.md「技術・仕組みの選択は表現不可能性 → 還元性 →
先進性の順で決める」節に対応)を書く。技術・仕組みの選択を含む `Dn` で、
外部依存の新設・置換・撤去、新しい道具・単位の実装言語の選択、または
戻せないもの(データの損失・外部契約・人手の作業)が生じるときは
`本命:`/`対抗馬:`/`外した候補:` も書く。裁定は「コード 0 行の白紙から
選ぶなら何を採るか」だけで下し、書き直しの作業量・歴史的経緯を理由に
しない(ADR-751)。コストを理由にしてよいのは
`戻せない: データ|外部契約|人手 — <内容>` の行だけで、閉語彙のコスト語が
`Dn` に現れると同じ gate が deny する。書式は technology-selection
スキルに従い、自己検査は同じ `plan-precedent-gate --check` コマンド
1 本でまとめて行う(新しい gate は呼ばない)。

## フィードバックの Issue 化(auto memory 固有の配線)

「ユーザーからのフィードバックは不可視なローカルメモに閉じ込めない」原則は
共有 AGENTS.md に従う。Claude Code の auto memory(`~/.claude/projects/*/
memory/*.md`、`metadata.type: feedback`)にこの種のフィードバックを保存
するときは、Issue 化するまでの一時メモに限定し、Issue 化したら本文に
その Issue 番号(`#N`)を書く(Issue へのポインタで足り、内容を重複させない)。
形式検査は `wrapup-stop-gate`(Stop hook、wrap-up inbox と同じ経路)が
担う — 今セッション中に更新された `type: feedback` メモリで `#N` 参照が
無いものを検出して促す。gate に当たる前に自発的に Issue 化すること。

## セッション内 PR チェーンの形式検査(ADR-0027)

stacked PR に積む原則そのものは共有 AGENTS.md に従う。形式検査は作成時
`stack-base-guard`(PreToolUse deny)と完了時 `pr-gate` の `G_stack`
(Stop block)が担う。gate に当たる前に自発的に積むこと — gate は漏れを
拾うためのもので、一次的な手段ではない。

## 決定成果物の執行点の形式検査(ADR-396)

決定成果物と執行点を同じ PR に出す原則そのものは共有 AGENTS.md に従う。
形式検査は作成時 `decision-colocation-guard`(PreToolUse deny)と CI
required check(`decision-colocation-check`、どちらも `crates/decision-colocation`
の同じ判定関数 = 単一ソース)が担う。
gate に当たる前に自発的に執行点を含めること — gate は漏れを拾うためのもので、
一次的な手段ではない。**「実装を後続 Issue に分離する」という選択肢は
実行不能なので、`AskUserQuestion` の選択肢に出さない。**

## 要求インベントリの書式と自己検査

計画冒頭に `## 要求インベントリ` を置き、依頼文と参照 Issue の子項目を
逐語で 1 行 1 項目・`R1..Rn` の ID 付きで列挙してから設計に入る。列挙は
本線で行う(Plan/Explore サブエージェントは CLAUDE.md を読み飛ばす)。
ExitPlanMode を呼ぶ前に `~/.claude/hooks/plan-scope-gate --check
<プランファイル>` で節内整合性(処分・タグ)を自己検査する。実装中に
見つかった隣接負債は AskUserQuestion で stack か wrap-up inbox かを聞く。
手順は requirements-inventory スキルに従う。

## 実装タスクの完了定義の形式検査

commit → push → `gh pr create` を、途中で確認を挟まず一続きで実行する。
「PR を作成しますか?」と聞かない。`gh` の投稿は本文を Write でファイルにして
から `-R OWNER/REPO … --body-file <絶対パス>` で渡す(bleep の正準形。
`pr-description` スキル §0)。Stop hook(`G_pr` in pr-gate)がこの
漏れを検査する。

PR 本文に未チェックの task list を残さない・人の確認を後続 Issue に払い
出す原則の形式検査は `pr-followup-guard`(Rust、PreToolUse deny、全リポジト
リで発火)が担う。gate に当たる前に自発的に Issue 化すること — gate は
漏れを拾うためのもので、一次的な手段ではない。

## 待ち方: 前景の `sleep` の後にコマンドを続けない

Claude Code 本体が、前景の `sleep N` のあとにコマンドを続ける書き方
(`sleep N; gh pr checks <PR> …`、`sleep N; cat <背景タスクの出力ファイル>`)を
`Blocked: sleep N followed by …` で拒否する(この dotfiles の hook ではない)。
拒否のたびに 1 往復を失うので、最初から次の手で待つ(#1059)。

- CI の完了: `pr-checks-wait <N>` を背景(`run_in_background`)で実行し、完了通知を
  待つ。赤が 1 件でも出れば即 NG(exit 1)、required が全緑になれば即 OK(exit 0)で
  終わる(時間切れの pending は exit 8、判定不能は exit 2)。`gh pr checks --watch` や
  `until` のポーリングでは待たない — `--required --watch --fail-fast` は集約ジョブ
  (`CI passed`)が全ジョブの完了後にしか結果を出さないので NG が遅れ、`--required`
  を外すと required でないチェックの完了まで待つ。判定の正本は
  `crates/pr-gate/src/gates/ci.rs`(`docs/agents/pr-gate.md`)。待ってよい対象は
  required のチェックだけ(共有 AGENTS.md「実装タスクの完了定義」)。
- 背景タスクの出力: 完了通知を待つか、`Monitor` の `until` ループ(共有
  AGENTS.md「長い処理では…」の形)で待つ。
- 前景の `sleep` で待たない。
- 背景タスクを `(cmd; echo exit=$?)` の形で束ねない。完了通知の終了コードが `echo` のもの(常に 0)になり、
  `cmd` が失敗しても成功に見える(2026-10-31、`npm ci` が `EROFS` で落ちたのに完了通知は成功だった)。
  終了コードが要るときは、`cmd` を単体で背景に出し、通知の終了コードを読む。

形式検査は `ci-wait-guard`(PreToolUse deny、`hook-router` の規則)が担う。Bash と
`Monitor` で、`gh pr checks --watch`・`gh run watch`・`until`/`while`/`sleep` で
`gh pr checks` などを引き直す形を実行前に拒否し、`pr-checks-wait` を案内する。gate に
当たる前に最初から `pr-checks-wait` で待つこと — gate は漏れを拾うためのもので、一次的な
手段ではない。PR に紐づかない workflow run を待つときだけ、コマンドに
`CI_WAIT_GUARD_ALLOW=1` を足す(`docs/agents/ci-wait-guard.md`)。

## sandbox の外で動かすコマンドは、単体で呼ぶ(#1164)

`gh`・`git commit`・`git push`・`git add`・`git rebase`・`git merge`・`git cherry-pick`・`git revert`・`git tag`・
`git switch`・`git branch`・`nix build` などは sandbox の外で動く(`sandbox.excludedCommands`、
`home/modules/claude.nix`)。鍵束・gpg-agent・nix daemon が要るためである。`git switch`・`git branch` は、
上流の設定を `.git/config` に書く(`git switch -c <branch> origin/<ref>`、`git branch -D`)が、sandbox の中では
`.git/config.lock` の代役が見えて `could not lock config file … File exists` で途中で止まる(ブランチと作業ツリーは
更新されるが、HEAD は元のブランチのまま。2026-11-11 に実測。並行セッションとの取り合いではなかった)ので外に出した。
`.git/config` は、フックや別名を書けるので、sandbox の書き込み許可には入れない。除外コマンドについて、**呼び出し全体の
コマンドが除外パターンに合わないと、全体が sandbox の中に残る**(Anthropic, "Sandboxing"、
<https://code.claude.com/docs/en/sandboxing>、取得 2026-10-27)。残ると `gh` は認証できず、署名は
`No secret key` で失敗し、`nix` は `Read-only file system` になる。2026-10-28 に vega で、
`gh auth status; echo "exit=$?"` が失敗し、`gh auth status` 単体が成功することを確かめた。

- これらは単体で呼ぶ。`; echo …`・`cd <dir> &&`・`> file` のリダイレクト・`$(…)` を連結しない。
- git は worktree の cwd で、`-C` なしに呼ぶ。シェルの cwd が呼び出しをまたいで保たれるのは、プロジェクトの
  ディレクトリと追加したディレクトリ(`--add-dir`・`/add-dir`・`additionalDirectories`)の中だけで、外へ `cd` すると
  cwd は元に戻る(結果に `Shell cwd was reset to <dir>` が付く。Anthropic, "Tools reference"、
  <https://code.claude.com/docs/en/tools-reference>、取得 2026-11-05)。セッションのプロジェクトの外にある worktree
  (別のリポジトリの worktree など)で git を呼ぶときは、`cd <dir> && git …` と並べるしかなく、連結すると
  除外パターンに合わず sandbox に残る。このときは、`SANDBOX_COMPOUND_GUARD_ALLOW=1` を足し、承認を得て
  `dangerouslyDisableSandbox` で呼ぶ(2026-10-29 にこの形で運用した)。除外は末尾だけに
  `*` を持つパターン(`git commit *` など)だけが効き、`git -C <dir> commit` は sandbox に残って署名が
  `No secret key` で失敗する(2026-10-28 に vega で実測、#1164)。`git -C` は読み取りだけに使う。
- 出力の加工が要るときは、`gh … --jq` のように道具の側の機能を使う。
- **別のリポジトリの worktree で作業する手順**(`herdr worktree create` は sandbox の中では
  `Operation not permitted` で落ちた。2026-10-29・10-31・11-01 に実測):
  1. `herdr worktree create --cwd <repo> --branch <name>` を単体で呼ぶ(`sandbox.excludedCommands` に入れてある)。
  2. 作った worktree で、最初に `git fetch origin` を呼び、`origin/main` との差を `git log` で見る。worktree は
     古いローカルの `main` から切られることがある(2026-10-29 に、マージ済みの PR を含まない `main` から
     切られた)。ずれていれば `git rebase origin/main` で追従する。
  3. 編集は、worktree の絶対パスを指定した Edit/Write で行う。Bash の `sed -i` などは、worktree が
     sandbox の書き込み許可の外にあるので `Read-only file system` になる。git の書き込みは上の項のとおり。
  4. `gh` は `-R OWNER/REPO` を付ける(付けないと cwd のリポジトリを見に行く)。

形式検査は `sandbox-compound-guard`(PreToolUse deny、`hook-router` の規則)が担う。除外一覧(`sandbox.excludedCommands`)
のコマンドを `;`・`&&`・`|`・ループ・改行で別のコマンドと並べる形を実行前に拒否し、単体で呼ぶよう案内する
(`docs/agents/sandbox-compound-guard.md`)。gate に当たる前に最初から単体で呼ぶこと — gate は漏れを拾うためのもので、
一次的な手段ではない。

sandbox の中と外の線引き(#1103。2026-10-28 に vega で実測した範囲):

- **sandbox の中でよい**: `cargo test`・`cargo check`・`cargo metadata`・`cargo fmt`(`target/` は cwd 配下で書ける)、
  `issue-inbox --add`・`--set-status`(書き込み先 `~/.local/state/claude` と vault は書き込み許可にある)、
  scratchpad・`$TMPDIR` への書き込み。cargo を sandbox の外で回す必要は無い。
- **sandbox の外が要る**: 上の除外コマンド(`gh`・署名つきの `git`・`nix build`)。`new-tool-guard register` は
  `~/.local/state/claude/new-tool-guard/state` に書くので、sandbox の中でよい。
- **`nix develop` は除外一覧に無い**ので、sandbox の中では nix の fetcher の置き場(`fetcher-locks`)が
  `Read-only file system` で落ちる(2026-10-30 に `nix develop --command cargo run -p pipeline-map -- render` で発生)。
  図の再生成は `nix develop` を介さず `cargo run -p pipeline-map -- render` を直接呼ぶ(`d2` は PATH にある。
  `home/modules/packages.nix`)。CI と同じ clippy の再現は、次の項目の `nix build` を使う。
- **CI の clippy は手元の `cargo clippy` と toolchain の版が違う**(CI は flake の nixpkgs の rustc、手元は rustup。
  `--deny warnings` で `unnecessary_sort_by` が CI だけで落ち、上の段 4 本の積み直しになった。2026-10-30)。
  push の前に、CI と同じ derivation を `nix build .#checks.x86_64-linux.dotfiles-tools-clippy --no-link`
  (`flake.nix` の `checks`。単体で呼ぶ。2026-11-08 に vega で exit 0 で通ることを確かめた)で回す。
- **プロセスの存在確認(`ps`・`pgrep`)は sandbox の外で行う。** sandbox の中は PID namespace が隔離されていて、
  ホストのプロセス(Obsidian など)が見えず、自分のシェルの行にだけ当たって「動いていない」と誤判定する
  (2026-10-28 に `pgrep -f obsidian` で発生)。一覧が本物かは、ホスト側にしか無いプロセス(`cosmic-comp` など)の
  有無で確かめる。
- **新しく作った `.nix`・`.rs`・skill のファイルは、`nix build`・`nix flake check` の前に `git add -N <path>` する。**
  flake は git が追跡するファイルだけを見るので、未追跡のままだと `is not tracked by Git` で落ちる。もっと悪いのは
  落ちずに通る場合で、未追跡のスキルを数えない評価が通り、コミット後にスキル数の assert が落ちる状態で push した
  (2026-11-08)。`Git tree has uncommitted changes` の警告は、未追跡かどうかを区別しない。
- **cwd はリポジトリのルートに保つ。** sandbox は、cwd と `cd` で入ったディレクトリごとに、書き込み禁止の代役
  (`.claude`・`.mcp.json`・`.gitmodules`・`.bashrc` など)を置く。`crates/` に入ると `crates/.claude` ができる
  (#1100、#1166)。cargo の workspace は `.` で始まる名前を拾わず、`.gitignore` は代役の名前を無視する
  ので壊れないが、代役は `rm` で消せず(`Device or resource busy`)、セッションが続く間は残る。
- **`cargo test --workspace` の赤は、変更の不具合とは限らない。** 2026-11-08 に vega で sandbox の中で回すと、
  9 crate が赤だった。原因の型は 4 つ(`cargo test --workspace > <ファイル> 2>&1` で書き出して、失敗の行を
  `grep -E 'FAILED|panicked'` で読む):
  - **Unix ソケットを作るテストは sandbox の中では動かない**: `UnixListener::bind` が `Operation not permitted`
    になる(`claude-statusline` の integration、`herdr-agent-metadata` の claude・codex・copilot、`herdr-client`、
    `codex-plan` の `runtime`・`experiment`・`lifecycle`)。fixture では直せない。`herdr-agent-metadata` と
    `codex-plan` は一時ディレクトリを `/tmp` に決め打ちにしていて(`tempdir_in("/tmp")`)、sandbox の中では
    `/tmp` が読み取り専用で、そこで先に落ちる。決め打ちの理由はコードに書かれておらず、ソケットのパスの
    長さの上限(`sun_path`)に収めるためかもしれないが、推測で、未確認。`TMPDIR` に向けても、その先の
    `UnixListener::bind` が落ちる見込みで、直す意味が無い(`herdr-client` の `bind` が `Operation not
    permitted` で落ちるのは実測した。この 2 crate の `bind` までは未実測)。
  - **gpg**: `gpg-subkey` の selftest は、fixture の鍵の生成が gpg-agent のソケットに届かず落ちる(同上)。
  - **git の commit の署名**(直した型): ホストの `~/.gitconfig` が `commit.gpgsign=true` だと、一時 repo の
    `git commit` が `No secret key` で落ちる。テストの git 補助関数に `-c commit.gpgsign=false` を足した
    (`git-repository`・`cmd-hash-log`・`update-own-tools`。`core.hooksPath=` を足した #428 と同じ型)。
  - **`env_clear` で `TMPDIR` も消える**(直した型): 一時ディレクトリが `/tmp`(読み取り専用)になる。fixture の
    下に向けた(`codex-launcher`)。
  上の 2 つの型(ソケット・gpg)は直らないので、この 5 crate 以外の赤は、変更の不具合を先に疑う。ソケットの
  型は、CI の結果で確かめる。

## テストの出力を `head` / `tail` で切らない

`cargo test … | head -10` は、失敗の行(`FAILED`・`panicked`)が切れた範囲の外に出ると見えず、終了コードも
`head` のものになる(2026-10-28、落ちるテストを PR #1199 に載せた)。絞るなら
`2>&1 | grep -E 'FAILED|panicked|test result'`、またはファイルに書いて終了コードを別に確かめる。

形式検査は `test-output-guard`(PreToolUse deny、`hook-router` の規則)が担う。`cargo test`・`cargo nextest` の出力を
`| head` / `| tail` へ流す形を実行前に拒否し、絞り方を案内する(`docs/agents/test-output-guard.md`)。gate に当たる前に
最初から切らないこと — gate は漏れを拾うためのもので、一次的な手段ではない。

## `pkill -f` / `pgrep -f` の検索語は、自分のコマンド行に当たらない形で書く

`pkill -f 'パターン'` は、プロセスのコマンド行全体から探す。自分が打った Bash のコマンド文字列は、実行するシェル
自身のコマンド行に入るので、`pkill -f trial.sh` は**自分のシェルを止め**、呼び出しが終了コード 143・144 で落ちる
(2026-10-31 に同じセッションで 2 回)。`pgrep -f obsidian` は自分のシェルの行にだけ当たって「動いている」と
誤判定する(2026-10-28)。検索語の 1 文字を `[]` で囲むと、同じ語に当たり、検索語の文字列そのものには当たらない。

- 書き方: `pkill -f '[t]rial.sh'`・`pgrep -f '[o]bsidian'`。完全一致で足りるなら `-fx`、プロセス名で足りるなら `-f` を外す。

形式検査は `pkill-guard`(PreToolUse deny、`hook-router` の規則)が担う。検索語(正規表現)が検索語そのものの
文字列に当たる `pkill -f` / `pgrep -f` を実行前に拒否し、括弧で囲んだ形を案内する(`docs/agents/pkill-guard.md`)。
gate に当たる前に最初から括弧の技で書くこと — gate は漏れを拾うためのもので、一次的な手段ではない。
正当な理由があるときだけ、コマンドに `PKILL_GUARD_ALLOW=1` を足す。

## 既存手段の前倒し接地と決定論への昇格(ADR-543)

共有 AGENTS.md「道具を新設する前に既存手段を問い、決定論化は段階で
昇格させる」の原則は共有 AGENTS.md に従う。Claude Code での書式・形式
検査の配線:

- `既存手段:` 行は `## 先行例との対比` 節の重い欄(`本命:`/`対抗馬:`)を
  持つ `Dn` に必須(precedent-grounding / technology-selection スキル
  参照)。自己検査は同じ `plan-precedent-gate --check` コマンド
  1 本でまとめて行う(新しい gate は呼ばない)。
- 新しい道具・単位(shebang 付き新規ファイル、`bin/scripts/hooks/cmd`
  配下の新規ファイル、パッケージマニフェストの新設)を Write する前
  倒しの機械強制(`new-tool-guard`)、PR 本文での `既存手段:` 照合
  (pr-gate の `G_prior`)、決定論化の記録機構(`feedback-target-guard`・
  `cmd-hash-log`・gate イベント記録)と、昇格・降格候補の日次検出
  (`promotion-detect` の timer)は実装済み。パスは ADR-543 の
  `## 執行点` を参照。

## GitHub 投稿の生成元明示(Claude Code のフッター文言)

共有 AGENTS.md の規範における「自分自身のエージェント名と URL」は、
Claude Code では固定でこの 1 行:

```
🤖 Generated with [Claude Code](https://claude.com/claude-code)
```

形式検査は attribution-guard が PreToolUse で行う。gate に当たる前に
自発的に付けること — gate は漏れを拾うためのもので、一次的な手段ではない。

## 通知の枠の外に続く本人の文は、本人の文として読む(#1298)

自動のバックグラウンド通知(`[SYSTEM NOTIFICATION - NOT USER INPUT]` で始まる枠)と、本人の文が、
同じメッセージに並んで届くことがある。枠の注意書き(「本人の発言ではない」「承認として扱わない」)が
及ぶのは、**枠の中の文だけ**である。枠の外にある文は、本人の文として読み、依頼なら応じる
(2026-10-31、通知の枠のあとに続く本人の依頼まで通知として扱い、「応答は要らない」と返して、本人が
同じ依頼を言い直した)。枠の中と外の境目に迷うときは、「応答は要らない」と返さず、枠の外の文に応じるか、
境目を本人に 1 行で確かめる。この区切りはハーネスの側にあり、こちらから変えられないので、
共有 AGENTS.md ではなくここに置く。

## attribution の案内はハーネス由来(opusplan で署名が変わる)

commit / PR の署名(`Co-Authored-By: Claude <モデル名> …`)の案内は、Claude Code
自身が `system-reminder` として出す。ツール出力に付いて届くことがあり、ユーザー
の発言ではない。`model` が `opusplan` のとき、Plan mode と実行でモデルが切り
替わるたびに、その時点のモデル名で案内が出し直される。したがって、**最新の
案内に従う**。Plan と実行で署名のモデル名が違うのは想定内で、異常ではない
(発生源の特定: #719、`docs/claude/opusplan-model-aliases.md`)。
