# ADR-468 — stacked PR の中間段では重い CI ジョブを skip する

- Status: Accepted
- Date: 2026-09-25
- Issue: Closes #419
- Amends: なし。`.github/workflows/nix.yml` の既存の required check 構成
  (ADR-0031 Amendment の merge 設定前提、docs/claude/pr-gate.md)には手を
  加えない。

## Context

stacked PR(直近 100 PR 中 39 本が base≠main)では、各段で `nix.yml` の
5 ジョブ(`flake-check`, `rust`, `build vega`/`build arcturus`/`build altair`)
と `ci.yml` の 3 ジョブが毎回フルに走る。`pr-gate.sh` は base≠main の段では
required check が定義できない(ruleset は `~DEFAULT_BRANCH` スコープ)ため、
報告された全チェックが quiesce するまで待つ設計になっている
(docs/claude/pr-gate.md「required check が無いと、ゲートは空振りする」)。
つまり中間段のセッションの Stop 待ちは、実質「一番遅いジョブの完了」で
決まる。

#419 が計画時点(2026-09-23)に実測した内訳:

| ジョブ | 時間 | required? |
|---|---|---|
| `build altair`(macos-latest) | 4m30s–13m49s(中央値 5m49s) | いいえ |
| `build <linux host>` ×3 | 2m26s–14m01s(中央値 ~3m15s) | いいえ |
| `rust workspace` | 1m45s–2m19s | いいえ |
| `Secret scan (gitleaks)` | 31–39s | いいえ |
| `nix flake check` | 32–40s | **はい** |
| `Escape-hatch dry-run` | 37–46s | **はい** |
| `Shell script validation` | 31–58s | **はい** |
| `PR title` | 4–9s | **はい** |

判明していた 3 つの事実(#419):

1. 重いジョブ(`build ×3` / `rust workspace`)は 1 つも live の required
   check ではない(2026-09-24 実測時点)。赤のままマージできる状態は
   skip しても悪化しない。
2. 待ち時間の主因は実行時間そのものではなく、`build altair` が
   macOS runner の同時実行枠に複数の stacked 段から同時に刺さる待ち行列
   だった(`concurrency` は SHA キーのため段どうしは互いをキャンセル
   しない)。
3. GitHub Docs("Troubleshooting required status checks"、取得
   2026-09-25)は次を明記する:「Successful check statuses are `success`,
   `skipped`, and `neutral`.」一方、「A workflow is skipped by path
   filtering, branch filtering, or a commit message. Associated checks
   stay in a 'Pending' state and block merging.」— つまり **job 単位の
   `if:` skip(`skipped` 扱い)と、workflow 単位の path/branch filter
   skip(`Pending` のまま滞留)は required check への影響が別物**。
   `nix.yml`/`ci.yml` が `pull_request` に path filter を掛けていない
   理由(既存コメント)は job-level `if:` には当てはまらない。

加えて本 ADR の起票セッションで新たに確認した事実:

4. `rulesets/quality.json`(#420、2026-09-24)は required check を
   `Shell script validation` / `Escape-hatch dry-run` / `nix flake check`
   / `rust workspace` / `build vega` / `build arcturus` / `PR title` の
   7 本と宣言しているが、live の GitHub Quality ruleset(id 23694400)は
   依然 4 本のまま(`apply-rulesets.sh --reconcile` が未実行)だった。
   `build altair` を required から意図的に外し、`rust workspace` /
   `build vega` / `build arcturus` を required に昇格する裁定は #420 で
   既に下りている。

## Decision

1. `.github/workflows/nix.yml` に `ubuntu-latest` の gate job
   `stack-position` を追加する。checkout 不要、`permissions:
   pull-requests: read` のみ。出力 `heavy` を次の優先順位で決める:
   - `github.event_name != 'pull_request'`(push / workflow_dispatch)→
     `true`
   - PR の base が `default_branch` と一致(bottom 段)→ `true`
   - PR に `ci:full` ラベルがある(手動脱出口)→ `true`
   - この PR の head を base にした open PR が 0 件(tip 段)→ `true`
   - 上記いずれでもなく、かつ `gh api` での子 PR 件数取得に成功 → `false`
   - `gh api` が失敗 → `true`(fail-open)
2. `rust` と `build-host`(`vega`/`arcturus`/`altair` の 3 行マトリクス)に
   `needs: stack-position` と
   `if: ${{ !cancelled() && needs.stack-position.outputs.heavy != 'false' }}`
   を付ける。`== 'true'` ではなく `!= 'false'` + `!cancelled()` にするのは、
   `stack-position` 自体が失敗・キャンセルされたときに `needs:` の既定
   (依存 job 失敗 → skip = fail-closed)が gate の fail-open 方針を
   上書きしてしまうのを防ぐため。
3. `pull_request` の `types` に既定の `opened`/`synchronize`/`reopened` に
   加えて `labeled` を追加する(GitHub Docs "Events that trigger
   workflows" 取得 2026-09-25: 既定 types に `labeled` は含まれない)。
   これが無いと、中間段に後から `ci:full` を貼っても Nix workflow が
   再発火せず、脱出口が機能しない。
4. `flake-check`(required)と `ci.yml` の 3 ジョブ(`gitleaks`,
   `shellcheck`, `dry-run`、いずれも 1 分未満)には手を加えない。全段で
   従来どおり走る。
5. `rulesets/quality.json` が #420 で宣言した required 7 本を
   `./scripts/apply-rulesets.sh --reconcile` で live の Quality ruleset
   に反映する。これにより base=main の PR(bottom 段)は `rust
   workspace` / `build vega` / `build arcturus` の完走を pr-gate が
   待つようになる — 「重いジョブが何も守っていない」状態を解消する。

### 担保水準の変化

- 下がらない。重いジョブは required 昇格後も skip されるのは非
  bottom・非 tip の中間段だけで、その段は元々どの required check にも
  数えられていなかった(#419 の事実1)。
- 各段は作られた瞬間に必ず tip なので heavy は少なくとも 1 回走る。
  次の段が積まれて非 tip に落ちるのはその後。
- bottom 段(head = main + stage 1..k)では常に heavy が走るため、
  マージ直前の合成木は必ず検査される。
- `nix flake check --all-systems --no-build` は中間段でも従来どおり
  走り、全ホストの evaluation は失わない。失うのは中間段での
  derivation のビルドと `cargo test` だけ。
- 残る穴: 中間段を後から編集して単独マージする場合、auto-retarget で
  base が `main` に変わっても head SHA が変わらないため Nix workflow は
  再発火せず、heavy は `skipped` のまま。`stacked-pr` スキル §4 は
  「下位段の修正時は必ずその場で全上位段を rebase する」を既定にして
  いるため、この穴が実際に踏まれるのは「中間段を修正し、かつ上位段を
  rebase せずにその段を単独でマージする」場合に限られる。この穴は
  `ci:full` ラベル(本 ADR の決定3)の手動運用でのみ塞ぐ — 自動検出は
  追加しない(下記 Alternatives considered)。

## Alternatives considered

- **merge queue の導入**: GitHub Docs("Managing a merge queue" /
  "Events that trigger workflows"、#419 が取得 2026-09-23)によれば
  ruleset は `~DEFAULT_BRANCH` スコープなので、中間段は base が main に
  張り替わって初めてキューに入り、そのとき合成される一時ブランチは
  「最新 main + その PR」= 本決定の bottom と同じ合成木になる。
  残差は「合成先が常に最新の main であること」と「失敗時に物理的に
  マージをブロックすること」の 2 点のみで、そのために必要な費用
  (全 workflow への `merge_group` 配線、`PR title` の手当て、required
  昇格との連動、9 段なら直列 60–120 分という即時マージの喪失)が見合わ
  ない。今回は採らず、再検討するなら独立 Issue で。
- **中間段の修正を `pull_request: edited` で拾い、auto-retarget を
  自動検出する**: 前述の残余の穴を塞げるが、`edited` はタイトル・本文の
  編集でも発火する。`pr-description` スキルの G_link/G_visual 対応で
  本文編集は日常的に起きるため、bottom 段では同じ head SHA に対して
  heavy が丸ごと再実行され、`skipped` だった check を上書きする形で
  証跡が消える。穴の実効サイズ(rebase 済みなら発生しない)に対して
  副作用が大きく採らない。
- **`pr-gate.sh` に「base=main かつ直近 Nix run で heavy が skipped なら
  block」を追加する**: body 編集の副作用は無いが、pr-gate に新たな
  `gh api` 呼び出しと判定分岐を増やす。gate 本体の複雑さは、塞ぐ穴の
  実効サイズに見合わない(還元性)。
- **`pull_request.paths` によるジョブ skip**: 本 ADR の Context 事実3の
  とおり、workflow 単位の path filter skip は required check を
  `Pending` のまま止め、merge を block する。required 化する
  `build vega`/`build arcturus`/`rust workspace` に使うと矛盾する。
- **判定ロジックを `scripts/` の bash + `--selftest` に出す、または
  Rust crate にする**: ADR-0024(`docs/adr/0024-hook-cli-scripts-target-
  rust.md`)の対象スコープは `rust-migration.toml` の `[scan].dirs`
  (`config/*/hooks`, `scripts/` 等)で、`.github/workflows/` は含まれ
  ない。`scripts/` に出すと migration-audit が未分類として fail し、
  `[[target]]` 追記と `max_remaining` 加算が必要になり ADR-0024 の
  既定に逆行する。Rust crate にすると gate job 自体が nix installer +
  cargo build を要し、守る対象(1 分未満のジョブ群)より重くなる。
  5 分岐の inline bash のまま `.github/workflows/nix.yml` に置く
  ——判定が増えて 1 ファイルの見通しが悪くなったら `scripts/` へ出す
  閾値として扱う。

## Consequences

- 中間段の wall-clock は「required 4 本 + gitleaks」(全て 1 分未満)に
  収束する。9 段の stack なら heavy が走る PR は 9 → 2(bottom・tip)。
- base=main の PR は `rust workspace`(2 分)・`build vega`/`build
  arcturus`(2–10 分)の完走を pr-gate が待つようになる(#420 の裁定の
  実行)。1 分未満だった従来の待ち時間から伸びる。
- 中間段を修正した場合の既定動作は「全上位段を rebase してから
  マージ」(stacked-pr スキル §4)。それを踏まずに中間段を単独マージ
  するときだけ `ci:full` ラベルが要る。この運用は `stacked-pr` スキル
  にも 1 行追記する(段2、後続 PR)。

## 執行点

- `.github/workflows/nix.yml` — `stack-position` gate job、`rust` /
  `build-host` への `needs:`/`if:`、`pull_request.types` への
  `labeled` 追加(本 PR で変更)
- `rulesets/quality.json` — #420 で宣言済みの required 7 本(本 PR では
  変更しないが、`apply-rulesets.sh --reconcile` の実行により live 側を
  この宣言に一致させる)

## Amendment (2026-09-26 — 変更 path が入力に含まれない PR でも heavy を skip する)

Decision 1〜3 の stack 軸は、bottom 段(base=main)を常に `heavy=true` に
した。しかし直近 60 本のマージ済み PR のうち約 33% は `flake.*` /
`home/**` / `patches/**` / `crates/**` / `Cargo.*` を一切触っていない
(docs のみ・`config/claude/skills/**` のみが典型)。bottom 段でも重い
ジョブを毎回待つ運用(ユーザー確認、2026-09-26)では、required でない
`build altair` を含む全ジョブの完走を Stop 前に待つため、stack 軸だけでは
このコストを取り切れない。

事実(2026-09-26 実測・確認):

- `config/**` `scripts/**` `rulesets/*.json` `packages/declarative/
  apt-packages.txt` は home/ から `home.file` / `xdg.configFile` の
  `source =` で**コピーされるだけ**で、build 時に実行・コンパイルされない
  (`home/`・`flake.nix` に config を処理する `runCommand` /
  `writeShellApplication` は `nixgl.nix` のラッパー以外に無い)。参照
  パスの欠落は常時走る `nix flake check --no-build`(flake-check job)の
  eval で捕まる。
- `rust workspace` は `rust-migration.toml` の `[scan].dirs`
  (`config/{claude,codex,copilot,git}/hooks`, `config/claude/statusline`,
  `scripts`)と `extra = ["bootstrap.sh"]` を migration-audit が走査し、
  `crates/fixture-oracle` が `config/claude/hooks/external-send-guard.sh`
  を実行する。よって hook・script の変更は rust の正当な入力。
- job-level `if:` による skip は required check を `success` 扱いにする
  (GitHub Docs "Troubleshooting required status checks" 取得
  2026-09-25、Decision 1 と同じ根拠)。

### Decision

1. `stack-position` の出力を `heavy`(1 本)から `build_host` / `rust`
   (2 本)に分ける。各出力は stack 軸(Decision 1〜3 のまま)と path 軸
   の AND。
2. path 軸: `event_name != pull_request` または `ci:full` ラベルでは
   常に触れた扱い(stack 軸と同じ脱出口を共有する — 脱出口を増やさない)。
   それ以外は `gh api repos/.../pulls/<N>/files --paginate` で変更ファイル
   一覧を取り、`BUILD_INPUTS`(build_host の入力)・`RUST_EXTRA_INPUTS`
   (`BUILD_INPUTS` に加えて rust だけが要る入力)への prefix/完全一致で
   `build_touched` / `rust_touched` を決める。`gh api` 失敗、または
   REST の 1 PR あたり上限 3000 件(GitHub Docs "List pull requests
   files" 取得 2026-09-26)に達した一覧の打ち切りは fail-open。
3. `config/` `scripts/` `rulesets/` `packages/` は `COPY_ONLY` として
   宣言だけしておく(判定には使わない)。`ci.yml` に drift 検査ステップ
   「nix.yml path gate covers every path the nix code reads」を足し、
   `flake.nix` / `home/**/*.nix` が読む非コメントの相対パスのうち
   `BUILD_INPUTS` にも `COPY_ONLY` にも載っていないものを `::error` に
   する。新しいトップレベルディレクトリを nix コードから参照し始めた
   のにこの PR がゲートのリストを触っていない状態を検出する
   (「Every --selftest is wired into CI」ステップと同型)。
4. push イベントと `.github/workflows/nix.yml` の `push.paths` は変えない
   — push 側は job を区別しない粗い集合のままで、ゲート側の job 別集合
   と単一正本化はしない(相互参照コメントで済ませる)。

### 担保水準の変化

- 下がらない。skip されるのは required check が `success` 扱いになる
  job-level `if:` のみで、Decision 1 の根拠(`skipped` は success)が
  そのまま当てはまる。
- bottom 段でも path 軸で skip され得るようになる点が Decision 1〜3 から
  の変更。「bottom 段は常に heavy」という本文冒頭の記述は、path 軸を
  導入した現在は成立しない — bottom かつ変更 path が入力に無ければ
  skip される。
- 残る穴: (a) Decision の「残る穴」と同じ auto-retarget の穴(head SHA
  が変わらないと再実行されない)。(b) `COPY_ONLY` 宣言そのものの誤り
  ——drift 検査は「未分類」しか検出せず、「本当は copy-only でない」
  という誤分類は検出しない。

### Alternatives considered

- **push イベントにも同じ判定をゲートで行い、`push.paths` を削って単一
  正本にする**: `github.event.before` が force-push や新規ブランチで
  全ゼロになる分岐が増えるだけで、push 側は既に required check の問題が
  無いため得るものが少ない(還元性)。
- **`dorny/paths-filter` action を使う**: PR イベントでは checkout 不要
  で REST API から変更一覧を取る点は同じだが、第三者依存が 1 つ増える。
  既存ゲートと同じ `gh api` + inline bash の延長で書ける(パターンは
  全て prefix/完全一致で glob 不要)ため採らない。

### 執行点

- `.github/workflows/nix.yml` — `stack-position` の `build_host`/`rust`
  出力分岐、`BUILD_INPUTS`/`RUST_EXTRA_INPUTS`/`COPY_ONLY` の宣言、
  `rust`/`build-host` の `if:` 更新(本 PR で変更)
- `.github/workflows/ci.yml` — drift 検査ステップ「nix.yml path gate
  covers every path the nix code reads」(本 PR で追加)
