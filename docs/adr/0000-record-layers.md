# ADR-0000 — 記録の層(L0〜L4)を宣言し、Write 時の拒否と日次のずれ検出で守る

- Status: Accepted
- Date: 2026-11-05
- Issue: #1310(記録の層(L0〜L4)を ADR で定義し、宣言ファイル・Write 時の拒否・日次のずれ検出で守る)

## Context

個人のリポジトリに追跡されている記録には、入れ替わる速さが違うものが混ざっている。ADR や規約は月〜年のあいだ効く。
その日の状態、済んだことの記録、観測の行、台帳の行は、1 週間も経たずに次の出来事に置き換わる。後者を Git に置くと、
コミットの履歴が日々の更新で埋まり、PR の差分の検査は日々の行には効かず、読み手は「いま効いている記録」と「済んだ記録」を
区別できない。

「高頻度のものは Git で管理しない」という裁定は散文(AGENTS.md・skill)にだけあった。新しい高頻度の記録を Git に作ることを
止める仕組みが無く、同じ規範違反の残党が繰り返し残った。散文から機械検査への昇格(ADR-543 の昇格型)に当たる。

層の定義の正本はこの ADR にする(2026-10-31 に裁定済みの定義を、ここに起こす)。適用範囲は tarotene 個人のリポジトリで、
会社・org 共有のリポジトリには効かせない(共有 AGENTS.md には置かない)。

## Decision

### D1: 記録を、効いている期間で L0〜L4 の 5 層に分ける

| 層 | 効いている期間 | 例 | 置き場 |
|---|---|---|---|
| L0 不変 | 月〜年(追記のみ) | ADR、裁定ログ | Git |
| L1 年〜月 | 月〜年 | 規約・スキーマ・スクリプト・定義・手順書、status(試験・持ちネタなど) | Git |
| L2 週 | 週 | 計画、週次以下の digest | Git |
| L3 日〜都度 | 1 週未満 | 状態、済んだことの記録、観測の行、台帳の行 | Git 外(Obsidian Vault が第一候補) |
| L4 機械生成 | 生成のたび | 外部の写し、キャッシュ、生成物、スナップショット | Git 外(vault の外) |

- 層は「記録の種類」に当てる。ファイルのコミット数ではない。書き換え型は書き換えの間隔、追記型は 1 件が次の出来事に
  置き換わるまでの間隔で測る。境界は「現在の事実として効いている期間が 1 週未満か」。
- 機械(CI・クラウドの定期実行)の読み手がいても層は変えない。読み手は「Git 外のどこに置くか」だけを左右する。
  読み手のいる L3 は「vault 不向き」として別に扱う(この ADR の範囲外)。
- 混在ファイルは層ごとに分割し、分割後は 1 ファイル = 1 層にする。パスで層を宣言できるようにするための前提である。

### D2: 層は、リポジトリのルートの宣言ファイルで、パスに対して宣言する(opt-in)

宣言は `.config/record-layers.toml`(git hook `post-checkout` が読む `.config/post-worktree.sh` と同じ、リポジトリ側が宣言を置く場所)に、「パス(glob)→ 層」の配列で書く。

```toml
[[path]]
glob = "docs/adr/**"
layer = "L0"
```

- 層の語彙は `L0`〜`L4` の閉集合。未知の層・キーは読み込みで落ちる(`deny_unknown_fields`、`Layer` は enum)。
- glob はリポジトリのルートからの相対パス。`*` は `/` を越えない(`literal_separator`)。ディレクトリの下は `dir/**`。
- **1 パス 1 層。** 重なる glob は検査で拒否する。glob 同士の包含は調べず、「`git ls-files` の各ファイルに当たる glob が
  ちょうど 1 つ」の検査に落とす。漏れ(0 個)も重なり(2 個以上)も同じ検査で見つかる。`cargo test -p record-layer`
  が、このリポジトリ自身の宣言に対して回す。
- 宣言を置いていないリポジトリでは、何も発火しない。他のリポジトリへの宣言ファイルの配布は、この ADR の範囲外。

glob の照合は `globset`(BurntSushi/ripgrep の同梱 crate)を使う。自前の最小の照合は、境界(`**` の `/` の扱い、先頭のドット)の
誤りの責任を自分で持つことになるので採らない。

### D3: Write 時の拒否は、`hook-router` の規則 1 本(`record-layer-guard`)

宣言のあるリポジトリで、次の Write を PreToolUse で拒否する。作る瞬間に層を決めさせる(表現不可能性: 宣言の無いパスは
Write では作れない)。

- 宣言に無いパス(どの glob にも当たらない)。拒否文に、宣言へ `[[path]]` を足してから Write し直す手順を書く。
- L3・L4 と宣言したパス。拒否文に、置き場(Git の外)を書く。

素通りにするもの(fail-open):

- 宣言の無いリポジトリ、git の外、リポジトリの外のパス(別のリポジトリへの Write を、cwd の宣言で裁かない)。
- **宣言が壊れているとき**(読めない・`[[path]]` が空・glob の誤り)。壊れた宣言で拒否すると、宣言自身を直す Write まで
  止めうるので、素通りにする。壊れたことは日次のずれ検出(D6)が wrap-up inbox に流す。
- 1 つのパスが 2 つ以上の glob に当たる(宣言の欠陥)。どちらの層で裁くか決められないので、素通りにする。D6 と `cargo test` が拾う。
- `.gitignore` の対象(`git check-ignore`。`target/` など)と `.git/` の中。記録ではない。

拒否するのは `Write` だけ。Edit と Bash の書き込みは見ない。Edit が作るのは既存ファイルの変更で、新しい記録を作る瞬間ではない
(Bash のリダイレクトを正確に字面から判定することは、`main-checkout-guard` の経験でも誤りが多い)。Edit と Bash の迂回は、
D6 の日次のずれ検出で補う。skip(迂回)の仕組みは作らない: 層が誤りなら宣言を直す(宣言ファイル自身への Write は L1 として通る)。

### D4: 判定は lib crate `record-layer` に 1 つだけ置く

宣言の読み込み・パスの層の引き方・Write の判定・ずれの判定は `crates/record-layer`(lib、bin なし)に置き、`hook-router` の規則と
`promotion-detect` の日次検出の両方が同じ関数を呼ぶ(単一ソース。`decision-colocation` の guard と CI check が同じ判定関数を
呼ぶ型と同じ)。

### D5: dotfiles 自身の宣言と、層が混ざった生成物の扱い

dotfiles 自身の宣言(`.config/record-layers.toml`)は、トップレベルの全パスを覆い、重ならない。

- `docs/adr/**` は L0(追記のみ。Amendment を足す形)。
- `docs/assets/pipeline-map/**`(`cargo run -p pipeline-map -- render` が作る図)は L4 と宣言する。生成のたびに作り直されるからである。
  置き場(Git の外のどこか)は別の作業で決める。決まるまでは Git に残し、Write では作らない(拒否される)。
- `flake.lock`・`Cargo.lock` は機械が書くが、再現性の固定点(pin)で、意図して更新する記録なので L1 とする。
- `crates/model-catalog/catalog.toml` は、1 ファイルに手書きの部分と GENERATED ブロックが混在する(D1 の「混在ファイルは分割」に反する)。
  分割は別の作業で扱い、分割するまでは `crates/**` の一部として L1 と宣言する。「当面の宣言」であって、層の判断ではない。

### D6: 日次のずれ検出は、`promotion-detect` に並べて足す

`promotion-detect` の日次 timer が、宣言を持つチェックアウトだけを読む(`PROMOTION_DETECT_GHR_DIR`、既定 `~/.ghr` の下の
`<host>/<owner>/<repo>`。宣言の無いチェックアウトは読まない)。2 種類を wrap-up inbox に流す。

1. **宣言の欠陥**: 宣言を読めない、追跡ファイルに当たる glob が 0 個または 2 個以上。D3 の fail-open を補う。
2. **宣言した層と、実測の変化の速さのずれ**: 直近 7 日に、ファイルを書き換えたコミットがある日の数(`git log --name-only`)を数え、
   L0 は 3 日以上、L1 は 5 日以上、L2 は毎日書き換わっているファイルを報告する。L3・L4 は Git に置かない層なので速さを測らない。
   しきい値は「宣言と矛盾する強い兆候」だけを拾う値で、`crates/record-layer/src/drift.rs` の定数(`drift_threshold`)が正本。
   層は記録の種類に当てるので、速さは根拠ではなく兆候として報告し、直すかどうかは `/backlog-cleanup` で本人が裁定する。

この timer には、Git の外に置いた状態の整合を読む検査(ADR-1311 の `worklog mail check`)が同じ形で載っている。ずれ検出は並べて足す。

### D7: Git に残った L3 の移行は、この ADR の範囲外

L3 は今ある中身ごと vault へ移し、履歴は書き換えない。移行は別の作業で扱う。この ADR が入れるのは、新しい L3・L4 を
Git に作らせない拒否(D3)と、変化の速さのずれを拾う検出(D6)である。

## Alternatives considered

- **リポジトリごとの CI required check だけにする。** 作る瞬間ではなく PR 時に止まる。作った後に気づく点で、Write 時の拒否に劣る。
  宣言の網羅検査(`git ls-files` の各ファイルに当たる glob がちょうど 1 つ)は、CI の `cargo test` として併用する。
- **日次の検出だけにする。** 作られた後に気づく。感触で外した(分析ではない)。
- **glob の照合を自前で書く。** 境界の誤りの責任を自分で持つことになる。感触で外した(分析ではない)。
- **`new-tool-guard` の拡張。** deny の対象(新しい道具・単位)と、宣言の形(パス → 層)が別。同じ Write の規則として並べるだけで、同居はしない。
- **宣言を `pipeline-map.toml` のような単一ファイルに寄せる。** 層の宣言はリポジトリごとに別で、dotfiles の manifest に他のリポジトリの宣言は置けない。

## Consequences

- 宣言のあるリポジトリでは、新しいパスに Write する前に層を決める。未知のトップレベルのディレクトリは、宣言へ 1 行足さないと作れない。
- 宣言の保守は、新しいディレクトリを足す PR ごとに 1 行ずつ発生する。忘れた場合、`cargo test -p record-layer` が PR で落とす(CI)。
- Edit・Bash の書き込みは拒否されない。D6 の検出は事後(最大 1 日遅れ)で、拒否の代わりにはならない。
- 宣言の欠陥は fail-open なので、宣言が壊れている間、拒否は効かない。日次の検出と CI が拾うまで、最大 1 日の隙がある。

## 執行点

- `.config/record-layers.toml`
- `crates/record-layer/src/lib.rs`
- `crates/record-layer/src/hook.rs`
- `crates/record-layer/src/drift.rs`
- `crates/record-layer/tests/dotfiles_declaration.rs`
- `crates/hook-router/src/rules.rs`
- `crates/hook-router/tests/cli.rs`
- `crates/promotion-detect/src/main.rs`
- `crates/promotion-detect/src/lib.rs`
- `crates/promotion-detect/tests/cli.rs`
- `pipeline-map.toml`

## 参考文献

- Stewart Brand, "Pace Layering: How Complex Systems Learn and Keep Learning", Journal of Design and Science, 2018-01-18,
  <https://jods.mitpress.mit.edu/pub/issue3-brand/release/2>(取得 2026-10-31、出典は Issue #1310 の記載。この ADR のために本文を読み直してはいない)—
  変化の速さで層に分ける枠組み。この ADR は、記録の「効いている期間」を層の軸に取る。記録の分類(L0〜L4 の定義と境界)は本 ADR の裁定で、Brand の記述ではない。
- `docs/adr/543-existing-means-and-deterministic-promotion.md`(散文から機械検査への昇格、`既存手段:` の前倒し接地)。
- `docs/adr/396-decision-colocation.md`(判定関数を hook と CI で単一ソースにする型)。
- `docs/adr/1311-git-external-state-integrity.md`(Git の外の状態の整合を `promotion-detect` の日次 timer で読む型)。
