# ADR-0000 — stacked PR の中間段では重い CI ジョブを skip する

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
