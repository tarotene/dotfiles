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
message になる」と ADR-0031 の Context と同じ論旨)。ただし実際にこの
テンプレートが適用された実リポジトリは存在しなかった(2026-09-22 時点、
typst 系リポ全数で 404 を実測)。

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
明記した。

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
