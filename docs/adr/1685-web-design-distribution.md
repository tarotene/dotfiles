# ADR-1685 — Pages のデザインの正本は `config/web/` に置き、各面は commit で固定した URL と sha256 で複写を pin し、CI が検査する

- Status: Accepted
- Date: 2026-11-24
- Issue: No-Issue(ADR-1677 が「配り方は、実機での描画の実測の後に別の ADR で裁定する」と残した項目の裁定。3 つの型から、本人が選んで決めた。この ADR は、その結果のうち公開してよい規則だけを残す)

## Context

ADR-1677 は、Pages の基準を DADS を土台にした自分用のものとし、正本をこのリポジトリに置くと裁定した。残した問いは、(1) 共通のトークン(色・書体・寸法)をどの形で持つか、(2) 各面(Web のサイト・claude.ai の Artifact の雛形)の側に、どう届けるか、の 2 つだった。

2026-11-24 に読んだ・測った事実(いずれも取得 2026-11-24):

- 各面の CI は `actions/checkout` に `submodules:` を指定していない。このリポジトリの共有 action(`deploy-worker`・`docs-*`)も自分では checkout しない。npm を使わない面(Python の生成器・手書き HTML・Artifact)がある。CSP を閉じた面は、第三者のオリジンから CSS を読めない(ADR-1677 D6)。
- このリポジトリの GitHub のリリースは 0 件(`gh release list` の出力が空)。
- 公開の面と非公開の面の描画の実測と、UX の審査から、同じ意味の色の食い違いと、暗色で固定色の白文字が淡い強調色の上に載る組(`#fff` on `#e0907c` = 2.49:1、`#fff` on `#8db8f2` = 2.04:1)が見つかった。値を検査で縛る必要がある。
- DADS のデザイントークンの配布元 `digital-go-jp/design-tokens`(MIT)の `examples/tokens.css`(commit `9891ff60f0b35917eebec66d83094ff43ae24626`、2026-10-20)は、原色の階調・semantic の色・文字の大きさ・行高・角丸の尺度を CSS の変数で持つ。**明の画面だけを定め、暗の値は持たない。**
- claude.ai の Artifact の公式ガイド(`guide(topic=pages)`)は、「The viewer sets data-theme="dark" or data-theme="light" on the root element, or nothing at all, in which case only prefers-color-scheme tells light from dark」と書く。Artifact の CSP は `fonts.googleapis.com` を許すが、ADR-1677 D6 が禁じる。

## Decision

### D1: 配り方は pin 型。各面が、正本の複写を自分のリポジトリに持ち、出所を commit と sha256 で pin し、CI が一致を検査する

各面のリポジトリは、正本(`config/web/variables.css` など)を複写して持つ。複写の出所を `design-pin.toml` に書く(`commit` は 40 桁の小文字 16 進、`name` は正本の `config/web/` の下のファイル名、`sha256` は内容、`path` は複写の場所)。URL は pin に書かない。共通部品が、自分のリポジトリ(action の `github.action_repository`)と `commit` と `name` から組み立てる。したがって、各面のファイルに所有者名が入らず、`main` などの浮動した参照も、別のリポジトリへの参照も、書けない。正本を変えたら、各面で複写・`commit`・`sha256` を同じ変更で上げる。

本命: なし — 候補を、各面の制約(npm を使わない面がある、CSP を閉じた面がある、どの面の CI も submodule を取得しない、リリースが 0 件)から絞った。結論から出発したのではない。
対抗馬: GitHub のリリースの成果物を版で固定して取る型(同じ評価軸「取得の経路を、各面が追加の運用なしで持てるか」。リリースの運用を新設する必要があり、タグ固定の成果物 URL の形式は実測していない)。git submodule(同じ評価軸。全面の CI の checkout に指定を足す必要がある。Renovate の submodule 対応は beta で opt-in)。
外した候補: npm パッケージとして配る — npm を使わない面が多く、取得の経路が面ごとに分かれるため外した(分析)。CDN から読み込む — ADR-1677 D6 が禁じる(分析)。DTCG の JSON を正本にして Style Dictionary で CSS を出す — 感触で外した(ビルドの依存が増えるのが気になった。実測していない)。
既存手段: .github/actions/design-pin/check.sh — 自前 — 却下: `sha256sum -c` と Rust の crate(`sha256sum -c` は commit の形の検査と pin の TOML を読めない。Rust の crate は `pr-title-check` の型で、他のリポジトリの CI が `cargo build` する分、重く壊れやすい。先行例の bash + `yq` + `sha256sum` の形に揃える)。
先行例: 個人の記録のリポジトリの、裁定ページの保存先 Schema を、commit で固定した URL と sha256 で固定し、`curl` と `sha256sum` で照合する bash の検査(同じ型の実装。取得 2026-11-24)。shadcn/ui の「複写して以後は自分のもの」の型(https://ui.shadcn.com/docs 、取得 2026-11-23)。
差分: 異なる — shadcn は複写した部品を以後は別物として育てる。ここは、複写の sha256 と、pin の commit の正本の内容の sha256 を CI が照合し、ずれたら落とす(複写 + 同期の弱点を、検査で補う)。先行例の bash の検査は、1 つの Schema を 1 つの読み手が読む。ここは、複数の面が複数のファイルを持てるよう、`[[file]]` を並べる形にした。
戻せない: 外部契約 — 各面のリポジトリに `design-pin.toml` と複写と CI の step が入る。pin の commit は、このリポジトリの履歴の中の点を指すので、リポジトリの名前や公開の状態を変えると、各面の pin の照合(正本の取得)が取れなくなる(複写の sha256 の検査は、オフラインで動くので残る)。
軸: 表現不可能 — pin 固定 > 浮動。pin の欄を `commit`(40 桁の 16 進)と `name`(1 つのファイル名)に縛り、URL を書けない形にした(`main` などの浮動した参照も、別のリポジトリへの参照も、書けない)。複写が pin と食い違う状態は、CI が落とす。

### D2: 正本の形は、2 層(原色・役割)で、役割の集合を閉じ、明暗を `light-dark()` で 1 回だけ書く

`config/web/variables.css`。原色(`--ref-*`)は DADS のトークン名の階調で、16 進の値だけを持つ。役割(`--bg`・`--surface`・`--text`・`--text-muted`・`--border`・`--border-subtle`・`--accent`・`--on-accent`・`--accent-text`・`--focus`・`--ok`・`--ok-bg`・`--ng`・`--ng-bg`・`--warn`・`--warn-bg`)は、原色を `var()` で指し、明暗を `light-dark(明, 暗)` で書く。役割の層に 16 進を直書きしない。面ごとのテーマは、`--accent`・`--on-accent`・`--accent-text`・`--focus` の値だけを上書きしてよく、役割を足さず、状態の色の意味を変えない(ADR-1677 D4)。尺度(文字の大きさ・行高・角丸・余白・操作部の大きさ・動きの長さ)と書体のスタックも、同じ場所に置く。

明暗の切り替えは、OS の設定に追従するだけにし、切り替えの UI を置かない。Artifact のホストが root に付ける `data-theme` の契約には、`:root[data-theme="light"]`・`:root[data-theme="dark"]` が `color-scheme` を指定して応える。`@media (prefers-color-scheme)` と `[data-theme]` の二重書きは、正本に持たない(`light-dark()` は `color-scheme` に従うので、1 回の記述で OS の設定と `data-theme` の両方に追従する)。

DADS から変えた点(値の出所と理由)は、`variables.css` の冒頭に書いた。暗の値は DADS に無いので、同じ原色の階調から、WCAG 2.2 AA を満たす階調を選んだ。警告の文字は、DADS の orange-800 でなく orange-900 にした(明の面の上で、余裕を持って 4.5:1 を超えるため)。

先行例: DADS の `design-tokens`(`examples/tokens.css`、MIT)と、Material Design 3 の reference・system のトークンの 2 層(https://m3.material.io/foundations/design-tokens/overview 、取得 2026-11-23)。claude.ai の Artifact の公式ガイドの `data-theme` の契約。
差分: 異なる — DADS は明の画面だけで、役割の層と暗の値を持たない。M3 の 3 層(reference・system・component)のうち、component の層は持たない(M3 自身が「in development」とする。部品の層は、部品を起こす段で足すかを決める)。
軸: 表現不可能 — 閉語彙 > 自由記述 + 事後 lint(役割の集合を閉じる)、宣言 > 手続き(明暗を `light-dark()` で 1 回だけ宣言する)。

### D3: 正本の合格条件(コントラストと形)は、`repo-structure-check` の `web_variables` が `cargo test` で検査する

役割の集合の欠け、役割の層への 16 進の直書き、解釈できない色(`oklch()` など。通してしまわず、違反にする)、`color-scheme`・`data-theme` の規則・`prefers-reduced-motion` の欠け、`@media (prefers-color-scheme)` の二重書きを落とす。文字 4.5:1・図形 3:1 の 21 の組(本文・弱い文字・リンク・強調色の上の文字・成功/誤り/注意の文字・境界・フォーカスの輪)を、明暗の両方で計算する。例外は持たない。

先行例: 同じ crate の `web_assets`(ADR-1677 D7)と `docs/agents/repo-structure-check.md` の型(構造の不変条件を `cargo test` で検査し、負例の fixture を同じ場所に置く)。個人の記録のリポジトリの、チラシの配色を TOML で持ち、JSON Schema で形を縛り、コントラストを検査する型(取得 2026-11-23)。
差分: 一致 — 同じ crate に同じ型の検査を足す。コントラストの式は、WCAG 2.2 の相対輝度と同じ。
軸: 検出のみ — この検査が届くのは、正本 1 ファイルの値と形だけである。各面が自分のテーマで `--accent` などを上書きした値には届かない(未実装、意図あり。各面を正本に従わせる段で、同じ組を面のテーマに当てる)。

### D4: 各面の CI の検査は、共通部品 `.github/actions/design-pin` を `@main` で呼ぶ

このリポジトリの `.github/actions/design-pin` を `@main` で呼ぶ 1 step(`uses: <このリポジトリ>/.github/actions/design-pin@main`)を、各面の CI に足す。検査の本体は `check.sh`(`--selftest` を持ち、このリポジトリの CI が走らせる)。`commit` が 40 桁の 16 進で、`name` が `config/web/` の下の 1 つのファイル名であること、複写の sha256 が pin と一致すること、(`check-upstream` が `true` のとき)その commit の正本の内容の sha256 が pin と一致すること(取得できなければ警告して、この 1 つだけを飛ばす)を検査する。

先行例: このリポジトリの共有 action(`deploy-worker`・`docs-python`・`pr-title`)を、他のリポジトリが `@main` で呼ぶ型(`docs/agents/deploy-worker.md`。取得 2026-11-24)。
差分: 一致 — 呼び出し側の複写を作らず、検査の本体を 1 か所に置く。
軸: 表現不可能 — 単一正本 > 複写 + 同期。検査の本体が各面に複写されず、1 か所の `check.sh` が全面の検査を決める。

## Alternatives considered

D1 の対抗馬・外した候補を参照。D2 では、値まで全面で同一にする案(主色も 1 組にする)は、ADR-1677 D4 が退けている。D3 では、検査を面ごとに持つ案(各面のリポジトリが、自分のトークンを自分で検査する)を外した — 同じ検査を面の数だけ複写することになり、複写が正本からずれる問題を、検査のレベルで繰り返す(分析)。

## Consequences

- 各面は、複写と `design-pin.toml` を持つ。正本を変えるたびに、各面で pin を上げる PR が要る。Renovate の `git-refs` の datasource で、正本の commit の追従を自動にできるかは、未検証。
- DADS の利用条件への対応: トークンの冒頭に、DADS の編集・加工である旨と出典(commit)を書き、MIT の著作権表示と許諾文を `licenses/digital-agency-design-system.MIT` に残した。デジタル庁のロゴ・キーカラーの規定は使わない。**2026-11-18 の改訂前の本文との突き合わせは、Internet Archive が取得を制限して、取れていない**(更新履歴は、変更を Material Symbols の追記 2 点とする)。
- 部品(`components.css`)・ページの型の要件・Artifact をその場で組むときの手順書は、この ADR の範囲外で、後続の ADR で裁定する(未実装、意図あり)。
- 各面を正本に従わせる作業と、面のテーマの上書きの検査(D3 の軸の「検出のみ」)は、この ADR の範囲外である(未実装、意図あり)。

## 執行点

- `config/web/variables.css`
- `crates/repo-structure-check/src/web_variables.rs`
- `crates/repo-structure-check/tests/tree.rs`
- `.github/actions/design-pin/action.yml`
- `.github/actions/design-pin/check.sh`
- `.github/workflows/ci.yml`
- `licenses/digital-agency-design-system.MIT`

## Verification

- `cargo test -p repo-structure-check`: 単体テスト 93 件と、ツリー全体の検査 19 件が成功(2026-11-24)。`web_variables` の単体テスト 10 件は、正本が通ることと、負例(弱い文字の明の組・暗色の `--on-accent` を白にした組・役割の欠け・16 進の直書き・解釈できない色・`data-theme` の欠け・二重書き・`var()` の循環)が、期待した違反で落ちることを確かめる。ツリー全体の検査のうち、`the_design_tokens_satisfy_the_contrast_and_shape_invariants` が D3 を、`the_design_tokens_check_reads_what_it_is_about` が正本を読めずに通ってしまわないための番兵を担う。
- `bash .github/actions/design-pin/check.sh --selftest`: 12 件が成功(複写と pin の一致、手で直された複写、`main` への浮動した参照、40 桁でない `commit`、`config/web/` の外を指す `name`、隠しファイルの `name`、`sha256` の形、複写が無い、`[[file]]` が無い、欄が揃っていない、pin のファイルが無い、`SOURCE_REPO` の形)。
- 未実施: 実際の面(別のリポジトリ)の CI から `design-pin` を `@main` で呼ぶこと。このリポジトリのマージ後でないと、呼べない。`check-upstream` の照合(取得できる `url`)も、マージ後の commit でないと試せない。
- 未実施: 正本の値を、実機の描画で目視すること(明暗・幅 390px)。値は、WCAG の式で計算し、検査が通ることだけを確かめた。
