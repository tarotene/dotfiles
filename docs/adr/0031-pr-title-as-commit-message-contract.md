# ADR-0031 — PR タイトルを commit-message 契約として機械強制する

- Status: Accepted
- Date: 2026-09-22
- Issue: tarotene/dotfiles#325(傘)

## Context

`tarotene` 配下の全リポジトリは squash-only 運用(`allow_squash_merge=true`、
`allow_merge_commit=false`、`allow_rebase_merge=false`)であり、
`squash_merge_commit_title=PR_TITLE` / `squash_merge_commit_message=BLANK`
の設定(`config/claude/skills/*-repo-governance/scripts/apply-repo-settings.sh`
に既存)により、**PR タイトルがそのまま `main` の commit subject になる**。
つまり PR タイトルは「squash merge 後に `main` に残る唯一のテキスト」であり、
non-conventional な commit が `main` に混入する経路は実質的に PR タイトル
1 箇所に収束している。

この Issue を計画するグリルセッション(2026-09-22)で目的を次のように
再定義した: 本件は「PR タイトルという特定フィールドの書式強制」ではなく
「**PR タイトルを経由して non-conventional な commit が main に混入する
ことを防ぐ**」ことが目的であり、混入しうる経路を総合的に塞ぐ設計とする。

### 混入経路の全体像

1. **squash merge(PR タイトル)** — 本 ADR の強制対象。client-side guard +
   server-side required check の二層。
2. **`main` への直接 push** — 既存の `github-audit rulesets` ドメイン
   baseline(`pull_request` rule type + squash-only 判定、ADR-0020/0021)が
   担う。本 ADR ではこの経路に新規作業を追加しない(既存の担保に依存する
   だけ)。
3. **ブランチ上の commit メッセージ / Issue タイトル** — squash によって
   `main` の履歴からは破棄されるテキストであり、「main に残る変更の記述」
   ではないため、本 ADR は明示的に**自由形式のまま強制対象にしない**
   (元 Issue #325 の宣言を維持)。
4. **GitHub UI からの revert(`Revert "..."` という自動生成タイトル)** —
   文法上 non-conventional なので CI red になる。運用は「`revert:` type に
   改題してから merge する」(`docs/claude/pr-title-contract.md` に記載)。
   revert 自体を特例扱いしない — 例外を増やすと文法の閉集合性が崩れる。

## Decision

### D1: 文法は Conventional Commits + Angular 慣行の 11 type 閉集合

```
type(scope)?!?: subject
```

- `type` は次の閉集合: `feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert`
  (Conventional Commits 1.0.0 の推奨集合 = `@commitlint/config-conventional`
  の既定集合)。
- `scope` は省略可。指定する場合は `[a-z0-9._/-]+` のカンマ区切り
  (実績: `feat(alacritty,herdr): ...`)。
- `!`(breaking change 標識)は type の直後・scope の直後のどちらでも可。
- `subject` は非空、言語自由(日本語・英語混在を許容 — 実績に合わせる)、
  長さ無制限(GitHub の表示上限に任せる。日英で情報密度が違うため一律の
  文字数上限は根拠が立てにくい)、末尾の `.`/`。` は禁止。
- 検査は 1 本の正規表現で決定的に行う。LLM 判定は使わない。

実測(2026-09-22、`git log --oneline -100` を全 20 non-archived リポで
集計): dotfiles は直近 87 commit 中非適合 1 件、全リポ合計で非適合 23 件。
既に大多数が自然にこの文法へ収束している。

### D2: merge 設定の宣言値

`squash_merge_commit_title=PR_TITLE` / `squash_merge_commit_message=BLANK`
/ squash-only。dotfiles の現状 API 値と一致し、governance skills の
`apply-repo-settings.sh` が既に適用している値と同じ — 新規の値を発明せず、
既存の適用側の契約を監査側(`github-audit`)に持ち込むだけ。

### D3: サーバ側検査は自作 reusable workflow

`.github/workflows/pr-title.yml`(`on: workflow_call`)を dotfiles に置き、
文法検査は `scripts/pr-title-check` 1 本に一元化する。各利用リポジトリは
dotfiles を checkout して同スクリプトを呼ぶ数行の呼び出し workflow を持つ。
job 名(= required check context)は `PR title` に統一する。

`amannn/action-semantic-pull-request`(確立された先行例)は不採用 — 文法
定義が action の YAML 設定と client-side guard の正規表現に二重化し、
どちらか一方だけ変更されて乖離するリスクを抱える。`pull_request.types` に
`edited` を含める設計(タイトル編集時にも再検査させる)は同 action の
先行例に従う。

### D4: client-side guard は `tarotene/*` 限定

`config/claude/hooks/pr-title-guard.sh` を新設し、`gh pr create`/
`gh pr edit --title` を検査して非適合タイトルを PreToolUse で deny する。
実装は `stack-base-guard.sh`(ADR-0027)と同じ様式 — `attribution-guard.sh`
を `source` して `is_target_at` を上書きする。

発火は owner が `tarotene` のリポジトリに限定する(`--repo`/`-R` フラグ、
無ければ `git remote get-url origin` から解決。解決不能時は fail-open)。
理由: 契約の根拠(squash title = main の履歴)は `tarotene` 配下の merge
設定に依存した話であり、`home/modules/claude.nix` の hooks は common 層
のため会社ホストにも配備される。会社リポには別の PR タイトル慣行が
ありうるため、guard の発火自体を owner でスコープする。escape hatch は
env `PR_TITLE_GUARD_ALLOW=1`(理由必須のタグ型 escape hatch ではなく
env にした — 単発の緊急 PR 用の一時解除であり、`No-*:` 系の「本文に恒久的に
残る決定」とは性質が異なるため)。

### D5: `github-audit titles` ドメインは仕組みの存在検査

新設ドメイン `titles` は「PR タイトル仕組みが機能する状態にあるか」を
検査する — 具体的には (a) `pr-title.yml` 呼び出し workflow の存在、
(b) その job の required check context が ruleset に登録されているか。
**open PR の個々のタイトルの適合を実測しない** — それは client guard と
required check(CI)の役割であり、audit が重複して判定すると「audit が
drift を報告しているのに guard は通す」ような整合しない状態が起きうる。
CI を持たないリポジトリ(workflows なし)は既存の `renovate` ドメインと
同じ `not-applicable` 判定にする(ADR-0020 の grandfathering 型)。

`settings` ドメインには `squashMergeCommitTitle`/`squashMergeCommitMessage`
の drift 検査を追加する(D2 の宣言値との比較)。

### D6: Issue タイトル・ブランチ commit は自由形式のまま(強制しない)

元 Issue #325 の宣言を維持する。Issue タイトルは「変更」ではなく「世界の
状態」を記述する別ジャンルであり、ブランチ上の commit メッセージは squash
により `main` の履歴から破棄されるテキストである。どちらも D1 の文法の
対象にしない。

### D7: バックフィル(既存非適合 commit の是正)を本件のスコープに含める

元 Issue #325 は「merged PR の履歴に対するバックフィルはしない
(ADR-0020 の `createdAt` grandfathering と同じ扱い)」とスコープ外に
していたが、グリルセッションでユーザーが明示的にスコープ内へ取り込んだ
(「どうせなら」)。

方式は履歴書き換え(対象 commit の subject 修正 + 書換後の全 commit の
再署名)と merged PR タイトルの編集を**セットで**行う — 元 Issue が
指摘していた「PR タイトル編集だけでは `main` の squash commit を書き
換えないため、両者が食い違う」という懸念そのものを解消する。実施は
強制層(client guard + required check)が全リポに播かれ、非適合の再流入が
止まってから行う(後続セッション、sub-issue S7)。本 ADR のスコープは
方針の確定までであり、実施そのものは含まない。

### D2 の追補: typst-repo-governance の先行実装との統合(Stage 5 で発見)

Stage 5(governance skills テンプレート更新)の実装中に、
`config/claude/skills/typst-repo-governance/templates/.github/workflows/
pr-title.yml` が既に `amannn/action-semantic-pull-request` を使い、
D1 と完全一致する 11 type の Conventional Commits 検査を実装済みだったと
判明した(理由コメントも「squash-merge では PR タイトルが main の commit
message になる」と ADR-0031 の Context と同じ論旨)。**訂正(#337 実装時、
2026-09-24)**: 「実際にこのテンプレートが適用された実リポジトリは存在
しなかった(2026-09-22 時点、typst 系リポ全数で 404 を実測)」という
当時の記述は誤りだった。private の typst リポジトリ 1 件が 2026-06-04
から旧 amannn 版 `pr-title.yml` を既に配備しており、その ruleset context
は `PR title (Conventional Commits)`(実値は ADR-0034 によりここに書かない)。
2026-09-22 時点の実測が誤っていた原因は未特定(該当リポジトリを見落とした
可能性が高い)。下記のユーザー裁定(既存 amannn 版を置き換える)自体は
変わらないが、実施時は「未配備のため移行コストがゼロ」の前提が崩れており、
この 1 件については置き換え移行(旧 context 名からの ruleset 更新を含む)が
必要になる。

D2 で「文法定義の二重化」を理由に `amannn/action-semantic-pull-request`
を不採用としたにもかかわらず、この既存先行例を見落としていた
(`config/claude/CLAUDE.md`「発明する前に先行例を確認する」の優先順位 2
「組織内の先行実装」を Plan フェーズで検索していなかったことが原因)。

ユーザー裁定: rust/typst/astro-site の 3 governance skill すべてを
dotfiles の reusable workflow(D3)に統一する。typst の既存 amannn 版は
置き換える。根拠: (a) 未配備のため移行コストがゼロ、(b) client guard
(`pr-title-guard.sh`)は owner ベース(`tarotene/*`)で発火し言語を問わず
これら 3 skill 対象リポにも既に効くため、判定根拠を統一しないと D2 が
懸念した「二重化」が今度は逆方向(governance skill 側 vs client guard 側)
で実際に発生する。

`quality.json` の required check context は `workflow_call` の連結命名
規則(`<呼び出し側 workflow の name> / <呼び出される job の name>`、GitHub
Community Discussion #46752)に基づき `"PR Title / PR title"` をベスト
エフォートの既定値として設定したが、実機未確認(どのリポにも未適用の
ため)。各 skill の SKILL.md / reference/manual-steps.md に「最初の実 PR で
Checks タブの実際の文字列を確認し、異なれば ruleset を訂正する」注記を
明記した。**この連結命名規則の理解は誤りだった — 2026-09-26 Amendment
参照。**

## Alternatives considered

- **`amannn/action-semantic-pull-request` の採用**: D3 で棄却(文法定義の
  二重化、第三者 action への依存)。
- **Issue タイトルも強制対象にする**: 元 Issue の宣言を覆す理由がない
  (D6)。「main に残るテキストを守る」という再定義後の目的とも整合しない
  — Issue タイトルは squash commit と無関係。
- **guard を全リポジトリでグローバルに発火させる**: 会社リポのチーム慣行と
  衝突したときに `PR_TITLE_GUARD_ALLOW=1` の連発になる。D4 で棄却。
- **バックフィルを引き続きスコープ外にする**: ユーザーが明示的に取り込む
  裁定をしたため不採用。

## Consequences

- `scripts/pr-title-check`(checker 単一ソース)、
  `config/claude/hooks/pr-title-guard.sh` + Codex/Copilot アダプタ、
  `home/modules/claude.nix` への登録(後続段)。
- `.github/workflows/pr-title.yml`(reusable workflow)、dotfiles の live
  ruleset への `PR title` required check 追加(後続段)。
- `scripts/github-audit` の `settings` ドメイン拡張 + 新設 `titles`
  ドメイン(後続段)。
- `config/claude/skills/{rust,typst,astro-site}-repo-governance/` の
  `rulesets/quality.json` テンプレートと呼び出し workflow テンプレート
  (後続段、他リポへの播きの準備)。
- 他 19 リポへの播き(呼び出し workflow 配布 + ruleset context 追加 +
  merge 設定適用)と D7 のバックフィル実施は、本 ADR 確定後に sub-issue と
  して切り出し、`github-audit-triage` の一括裁定ループ(ADR-0015)で
  後続セッションが処理する。

## Verification

- `scripts/pr-title-check --selftest` / `config/claude/hooks/
  pr-title-guard.sh --selftest`(後続段で実装)。
- `scripts/github-audit --selftest`(後続段で拡張)。
- 本 ADR 自体は docs のみの変更のため `nix flake check` への影響はない
  (回帰確認として実行する)。

## Amendment (2026-09-24 — グリルセッションでの前提強化, #325)

「PR タイトル CI を入れたなら、merge 時の Default Commit Message 設定や
個別 commit の検査も要るのでは」という grill-me セッションでの問題提起を
受け、実測を伴って本 ADR の前提を再検証した。

### D1 の追補: ruleset `commit_message_pattern` の棄却

D2/D5 が担保する「PR タイトル proxy + 前提設定の検査」に代えて、GitHub
ruleset の `commit_message_pattern` メタデータ制限で artifact 側から直接
強制できないかを検討したが、三重に不成立だった:

1. **プランへの限定**: metadata restrictions は GitHub Docs のソース上
   `{% ifversion repo-rules-enterprise %}` に囲まれており、対象は
   "Organizations on a GitHub Enterprise plan"(GitHub Docs, "Available
   rules for rulesets"、取得 2026-09-23)。`tarotene` は `type: "User"` の
   個人アカウントで、実測(全 20 リポ、61 ルールを横断)でも metadata 系
   ルールの使用は 0 件だった。
2. **squash 時の意味論が未確定**: 同ページ内で本文と NOTE が矛盾しており
   (本文は「squash では結果の 1 commit だけを検査する」、NOTE は「squash
   するならブランチ上の全 commit が要件を満たす必要がある」)、GitHub の
   Community Discussion #193197(2026-04-20、未回答)は後者(全 commit が
   評価されて merge がブロックされる)を実地で報告している。
3. **先行例ゼロ**: angular/angular・conventional-changelog/commitlint・
   semantic-release/semantic-release・vitejs/vite・electron/electron の
   ruleset を全数列挙し、`commit_message_pattern` の使用例はゼロだった。
   確立された squash 運用下の答えは
   `squash_merge_commit_title=PR_TITLE` の固定 + PR タイトルの CI 検査
   という、本 ADR が D2/D3 で既に採っている構成そのもの
   (`amannn/action-semantic-pull-request` README、取得 2026-09-23、
   "you'll want to configure your GitHub repository to use the squash
   & merge strategy and tick the option *Default to PR title for
   squash merge commits*")。

`Alternatives considered` に一行追加する:
「**ruleset `commit_message_pattern` による artifact 側強制**: 個人
アカウントでは利用不可(Enterprise 限定)、squash 時の適用範囲が GitHub
自身のドキュメント内で矛盾しており未確定、確立された先行例も無い。三重の
不成立により不採用。」

### D2 の追補: 前提設定の検査を監査から required check へ格上げ

D2/D5 が宣言・監査する「`squash_merge_commit_title=PR_TITLE` /
`squash_merge_commit_message=BLANK` / squash-only」という前提は、**可変な
リポ設定の上に乗った proxy** であり、設定が drift すると契約全体が無声で
バイパスされる。全 20 リポの実測(2026-09-23/24)で、実際に **PRIVATE
リポジトリ 2 件がこの前提から drift していた**(実名は ADR-0034 により
省略、`squash_merge_commit_title=COMMIT_OR_PR_TITLE` /
`squash_merge_commit_message=COMMIT_MESSAGES`)。`COMMIT_OR_PR_TITLE` は
「PR が単一 commit なら PR タイトルではなく commit のメッセージを採用する」
設定で、うち 1 リポは直近 25 PR がすべて単一 commit だったため
**契約が 25/25 で無声にバイパスされていた**(証拠: そのリポの `main` に
PR 番号が二重付与された commit が残っていた)。

`github-audit settings` ドメインはこの drift を検出できるが、監査は手動
実行のため「検出」であって「防止」ではない。この前提検査を
required check(`.github/workflows/pr-title.yml`)へ同居させ、drift した
状態での merge 自体を CI red で止める(後続段、`scripts/
pr-merge-settings-check`)。判定ロジックは新設せず、
`scripts/github-audit` の `judge_settings()` を関数として再利用する
(単一正本を複数の実行点で使う。判定ロジックを複写すると 2 箇所が drift
しうる別の不正状態を生む)。

### D6 の追補: 実測による裏付け

D6(ブランチ commit のメッセージは強制しない)の判断は変えないが、根拠を
実測で強化する。squash 運用の 4 リポ(PUBLIC 2: dotfiles・telepath、
PRIVATE 2、実名は ADR-0034 により省略)の直近 25 PR、ブランチ commit
計 109 件を集計したところ、非適合は 1 件のみ(適合率 99.1%)で、その 1 件も
上記の設定 drift を経由して
初めて `main` に漏れた(D2 の追補が塞ぐ経路と同じ)。先行例として
angular/angular の `.husky/commit-msg` hook は意図的に advisory
(無条件 `exit 0`)であり、実際の強制は CI 側の `ng-dev commit-message
validate-range` が担う。ただし Angular は squash せず rebase 運用のため
ブランチ commit がそのまま `main` に残る点が本リポ群と異なる — squash
運用では強制する対象("main に残るテキスト")自体が存在しないため、CI 側の
強制も置かない。

### D7 の縮小: バックフィル範囲を tag/release なしのリポに限定

D7(既存非適合 commit のバックフィルをスコープに含める)の実施範囲を
縮小する。実測(全 20 リポ、`main` の直近最大 100 commit)で非適合
25 件を確認したが、うち PUBLIC 1(telepath、2 件)・PRIVATE 2(計 8 件、
実名は ADR-0034 により省略)の計 3 リポは tag/release が非適合 commit を
祖先に持ち、履歴書き換えで外向き資産(telepath は crates.io に公開済みの
release 2 件、PRIVATE の 1 リポは稼働中の release 群)が全 orphan になる。
ADR-0020 の `createdAt` grandfathering、ADR-0007「既存ファイルの遡及的な
一括リネームはしない」と同じ扱いで、この 3 リポ・計 10 件を明示的に
grandfather する。残る tag/release を持たない PUBLIC 2(dotfiles 1 /
publish-guard 1)・PRIVATE 6(計 13 件、実名は ADR-0034 により省略)の
計 8 リポ・15 件は D7 の実施対象のまま残す(対象リポの識別子は host-local
な監査記録側で追跡する)。

### 執行点

- `scripts/pr-merge-settings-check`
- `.github/workflows/pr-title.yml`

## Amendment (2026-09-26 — required check context の理論訂正と自己検査の追加)

#337(19 リポジトリへの播き)で 15 リポジトリに `PR Title / PR title` を
required check として ruleset に登録したが、実際にどの PR でも check が
Expected のまま報告されず、required のため merge が恒久的に止まる事故が
起きた(telepath#243, bleep#33 で最初に顕在化)。

### D2 の追補の訂正: 連結命名規則は「呼び出し側 workflow の name」ではなく
### 「呼び出し側 job の name」

telepath#243 の Actions API(`GET .../actions/runs/{run_id}/jobs`)を実測
したところ、実際の check 名は `"check / PR title"` だった(取得
2026-09-26)。呼び出し側テンプレートの job id は `check` で `name:` が
無く、GitHub は無名の job を job id で表示する。2026-09-22 時点の
D2 追補は「呼び出し側 **workflow** の name(`PR Title`)/ 呼び出される job
の name(`PR title`)」という理論を GitHub Community Discussion #46752 から
読み取ったが、この discussion 自体が未解決(no consensus)であり
(2026-09-26 再読)、実測はこれを支持しない。正しい規則は「呼び出し側
**job** の name / 呼び出される job の name」— dotfiles の `check` job に
`name: PR Title` が無かったために起きた、テンプレート側の単純な記述
漏れが根本原因だった。

修正: `repo-governance-common/templates/.github/workflows/pr-title.yml`
(3 skill 共通の単一正本、rust/typst/astro は symlink に統一)の `check`
job に `name: PR Title` を固定する。これにより連結名は常に
`"PR Title / PR title"` になり、`quality.json` の既定値は変更不要
(誤理論だったが結果的に正しい文字列を書いていた)。

### D2 の追補の拡張: `permissions:` 未宣言による startup_failure

呼び出し側テンプレートに `permissions:` が無く、reusable job の
`contents: write` 要求をリポジトリ既定の Actions workflow permissions が
`read` のリポジトリ(bleep 含む 15 中 13)では満たせず `startup_failure`
になっていた(bleep#33 で実測)。既定が `write` の telepath だけ偶然
成功していた。呼び出し側テンプレートの job に
`permissions: {contents: write, actions: read}` を明示し、リポジトリ既定
に依存しない宣言にする。

### D5 の拡張: `titles` ドメインの完全一致化 + ground-truth 突き合わせ

`judge_titles()` は当初、接尾辞 `' / PR title'` の後方一致を ok として
いた(#337 実装時、誤検出回避のため)。この緩さが今回の事故を検出
できなかった直接の原因 — 呼び出し側 job に `name:` が無く実際の check 名
`"check / PR title"` も接尾辞条件を満たしていたため、audit は誤って
全リポジトリを ok と報告し続けた。完全一致(`"PR title"` / `"PR Title /
PR title"` の 2 値)に締め、新たに新設した `fetch_run_job_names()` /
`fetch_latest_pr_title_job_names()` で最新 run の実際の job 名を取得し、
ruleset の宣言値と一致するかを突き合わせる(`pr-title-context-mismatch`)。
run が一度も走っていないリポジトリ(まだ播いていない)は ground truth が
無いため drift 扱いにしない。

### 新設: reusable workflow 自身による実行時自己検査

「最初の PR で Checks タブを目視確認する」という手動手順が唯一の防波堤
だったが、実際には飛ばされた。`scripts/pr-title-context-check` を新設し、
`.github/workflows/pr-title.yml`(reusable workflow)の最終 step として
実行する — 呼び出し元の run ごとに自分自身の check 名を実測し、有効な
branch ruleset の required_status_checks と自己照合する。`PR title` 系の
required context があるのに一致しなければ CI を red にする(手動確認の
廃止)。判定ロジックは新設せず、`scripts/github-audit` の
`fetch_run_job_names()` を re-source して再利用する(ADR-0035 D1)。

### 執行点

新設・変更した実体は次のとおり(いずれも新設 `scripts/pr-title-context-check`
と、既存の `.github/workflows/pr-title.yml` / `scripts/github-audit` への変更):

- `scripts/pr-title-context-check`
- `config/claude/skills/repo-governance-common/templates/.github/workflows/pr-title.yml`
- `.github/workflows/pr-title.yml`
- `scripts/github-audit`
