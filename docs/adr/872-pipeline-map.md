# ADR-872 — 自動発火する機構の地図を単一正本にし、登録箇所のポインタで網羅性を検査する

- Status: Accepted
- Date: 2026-10-20
- Issue: No-Issue(`/grill-me` セッション中に発生した依頼。「機構の動くタイミングと技術スタックを一枚の絵にしたい」)

## Context

hook(Claude Code 40 機構、Codex / Copilot の同じ判定エンジンのアダプタ)、systemd / launchd の timer、GitHub Actions、git hook、herdr の周期コマンドが積み上がり、
7 本のパイプライン(Plan 承認、実装から PR、気づきから Issue、repo governance、cloud routine、ホスト衛生と UI、活動の証跡)を形成している。
「何がいつ動き、何を次に渡すか」を一覧した文書は無く、図は 1 枚も無い。事実上の正本は、`home/modules/claude.nix` の hook 宣言と各 `home/modules/*.nix` の timer 宣言に散らばっている。

手描きの図は、機構が毎週増えるので 1 ヶ月でずれる。依頼は次の 3 つを一度に求めている。

1. 動くタイミングと状態遷移を見せる。
2. ベースの技術スタックを見せる。
3. どのドキュメントの入口に置いても遜色ない 1 枚にする。

## Decision

### D1: 図の主題は「トリガー、機構、出力」の有向グラフ 1 ジャンルにする

時間軸はレーン(発火のきっかけ)の並びと deny ループのエッジで、技術スタックはノードのクラス(色)で、LLM の関与はノードの形で乗せる。
カオスマップ(技術スタックの静的な分類)と状態遷移図は別図として描かない。1 枚の空間図に時間軸を押し込むと読めなくなるためである(diagramming スキルの cases.md 事例 #1)。

セッションの中で発火する機構(hook の event)は、エージェントセッション(ハブ)のコンテナの中に描き、ハブとの間のエッジは描かない。
deny を持つ機構は枠の色で示す。ハブへ向かうエッジは 25 本を超え、すべて描くと配線が図全体を囲んで読めなかったためである(試作で確かめた)。
manifest では、`session = true` のレーンと `hub = true` のノード(1 つ)でこれを表す。概観の段組は、レーンの `row`(1 = セッションと一連の流れ、2 = 常駐・手動・外部)で表す。
概観は「レーンの地図」にする。レーンを発火の時間順に番号を付けて並べ、グループ(レーン内で畳んだ単位)を色(スタック)・形(LLM)・パイプラインの番号で示し、
配線はレーン間の流れだけにする。機構同士の配線は、パイプライン別の詳細図が受け持つ。

### D2: 図のデータ(`pipeline-map.toml`)を正本にし、図はそこからだけ作る

root の `pipeline-map.toml` に lane、pipeline、node、edge を置く。`rust-migration.toml` と同型の machine-checkable な manifest で、lane / stack / kind は閉語彙の enum にする。
手描きの図を持たないので、図と実態のずれは「manifest と登録箇所のずれ」に還元され、D3〜D5 で検査される。

### D3: ポインタは 2 種類。Claude hook の宣言は nix 属性、それ以外はコメント

- `claudeHookDeclarations`(型付きの nix リスト、47 件)は、各要素に `node = "<id>";` を必須にする。nix の評価時に「node がある」「manifest に定義済み」を検査し、満たさない宣言は適用(`hms`)に至らない。
  `settings-reconcile` の宣言は未知のキーを拒否するので、`node` は渡す前に取り除く。評価結果の JSON は変わらない。
- それ以外の登録箇所(Codex / Copilot の登録呼び出し、timer / launchd agent、workflow、git hook、herdr の command 行、statusline の定義)は、登録の直前のコメント `pipeline-node: <id>` を持つ。
  nix の評価が及ばない形式(YAML、TOML、シェル文字列の引数)だから、検出は `cargo test` が担う。

後者を言語ネイティブの属性にするには、登録の宣言を型付きのリストへ作り直す必要がある。今回の範囲を超えるので、その移行は「意図は決まっていて未着手」とし、検査器の抽出規則へ 1 本足すだけで済む設計にしておく。

### D4: カバレッジ検査の母集団は、自動発火する登録箇所の閉じた 7 クラス

1. `claudeHookDeclarations` の各要素
2. `run ${registerCodexHooks}` / `run ${registerCopilotHooks}` の呼び出し
3. `systemd.user.timers.*` / `launchd.agents.*` / `systemd.user.tmpfiles.rules` の束縛
4. `.github/workflows/*.yml`(`on:` の直前)
5. `config/git/hooks/*`(先頭 15 行)
6. `config/herdr/config.toml` の `type = "command"` 行
7. `claude.nix` の `statusLineCmd` の定義

境界は「人間が名前を打たなくても走るか」で機械的に引く。手動 CLI・skill・外部サービス・状態ファイルは `auto = false` のノードとして図に描くだけで、ポインタを要求しない(ポインタされていたら fail)。

### D5: 検査は 3 方向。実装は新規の Rust crate `crates/pipeline-map`

- 登録箇所 → manifest: ポインタの無い登録箇所、manifest に無い id は fail。
- manifest → 登録箇所: `auto = true` のノードに登録箇所が 1 つも無ければ fail(機構が消えたのに地図に残る状態を許さない)。
- manifest 内: `hub` が 2 つ以上ある、`session = true` のレーンがあるのに `hub` が無い、`hub` が登録箇所を持つ(`auto = true`)または `session = true` のレーンに属している、`session = true` のレーンが複数の `row` にまたがる、重複 id、未定義の lane / pipeline / node 参照、両端に共通のパイプラインが無い edge、edge のパイプラインに両端の node が属していない(詳細図に端点が描けない)、実在しない `entity` / `doc`、ASCII でない(または引用符・バックスラッシュ・制御文字を含む)ラベルは fail。

`tests/coverage.rs` がリポジトリ本体に対して `cargo test` の一部として走らせる。実装言語は ADR-0024 の既定どおり Rust で、manifest を serde の型で読む。

### D6: 図のラベルは ASCII のみにする

D2 が SVG に埋め込むフォントは CJK を持たない可能性がある。ラベルの幅を実際の描画と合わせられないので、ノードのラベルはコマンド名、エッジのラベルは短い英語にし、日本語の説明は md の散文に置く。manifest の検査が非 ASCII を落とす。

### D7: 描画は D2(ELK)で SVG を生成してコミットし、CI が再生成差分で落とす

`d2` は flake の devShell にだけ足す。概観 1 枚(レーン × グループで約 30 ノード)と 7 本のパイプライン別の詳細図を、同じ manifest から生成する。
`.d2` の一致は `cargo test`、`.svg` の一致は CI の再生成差分で検査する。

### D8: 置き場所は `docs/pipeline-map.md` を正本とし、概観 SVG を 3 つの入口から参照する

root README の `## Background`、`docs/README.md` の冒頭、`docs/pipeline-map.md` に同じ SVG を参照で置く(複写しない)。
root に新しい md は置けない(ADR-0016)。画像は `docs/assets/` に置く(ADR-0028)。入口の README にはパスの列挙を足さない(ADR-0033)。

### D9: path gate の宣言に、検査が読む入力を足す

nix が読む `pipeline-map.toml` は `BUILD_INPUTS` に(足さないと CI の drift 検査が落ちる)、`cargo test` だけが読む `.github/workflows/`・`config/herdr/config.toml`・`docs/assets/pipeline-map/` は `RUST_EXTRA_INPUTS` に足す。
足さないと、manifest や登録箇所のコメントだけを変えた PR で `Rust workspace` job が skip され、検査が走らない。

## Alternatives considered

- **宣言元(claude.nix、timer 宣言、workflow)からの完全自動生成**: 正本が 1 つになる点は優れる。ただし、「何を次に渡すか」のエッジは宣言に無く各実体の中にあるので、エッジごとに抽出器が要る。還元性で劣るので採らなかった。
- **手描きのみ**: 感触で外した。1 ヶ月でずれるという感触で、定量はしていない。
- **全登録箇所をコメントに統一**: 仕組みは 1 種類で済む。ただし hook の宣言は nix の評価で守れず、ポインタの漏れは CI で初めて落ちる。表現不可能性で劣る。
- **Mermaid を md に直接埋める**: GitHub がそのまま描く点は魅力。ELK や frontmatter の設定への対応が限定的という印象で外した。検証はしていない(感触で外した)。
- **graphviz**: 仕上がりの印象で外した(感触で外した)。
- **draw.io(導入済み)**: `.drawio` は座標を自分で持つので、manifest から作るとレイアウトを自前で計算するか、図が manifest の複写になる。書き出しに Electron と Xvfb が要る。geometry を省いた入力を自動配置するかは未確認で、段2 の試作で確かめる。
- **カオスマップを主題にする**: 依頼文の「ネットワーク状」に合うのは有向グラフの側で、カオスマップは色分けと凡例に降格した。
- **`crates/migration-audit` に相乗りする**: その manifest は bash から Rust への移植の完了を検査するもので、`max_remaining = 0` に達して役目を終える。1 つの manifest が 2 つの寿命を持つことになる。

## Consequences

- 自動発火する機構を足すときは、`pipeline-map.toml` にノードを足し、Claude hook なら `node`、それ以外は `pipeline-node:` を書く。足し忘れると nix の評価か `cargo test` が落ちる。
- 機構を消すときは、manifest のノードも消す。登録箇所が無くなった `auto = true` のノードは `cargo test` が落とす。
- 手動 CLI と skill は地図に描くが、網羅は保証しない(母集団の外)。
- 描画(D7)と入口への埋め込み(D8)の実装は、同じセッションのチェーンの次の段が足す。この ADR の執行点には、段1 に実在するファイルだけを挙げる。
- 検査器は登録箇所の書式を文字列で読む。`claude.nix` の構造(リストの終端が行頭 2 空白の `];`、要素が `{` と `}` の行で区切られる)を変える場合は、検査器と、`crates/pipeline-map` のテストの fixture を合わせて直す。

## 先行例

- このリポジトリの `rust-migration.toml` と `crates/migration-audit`(manifest を実体と突き合わせる `cargo test`)。本決定は、コード側にも ID を持たせて逆方向(登録 → manifest)も検査する点で異なる。
- このリポジトリの ADR-0033 の `nav-doc-exempt` マーカー(grep で拾う閉じたコメントトークン)。D3 のコメントトークンはこの規約に倣う。
- このリポジトリの `docs/settings-reconcile.md`(hook は宣言を正とする)と `home/modules/apt.nix`(単一の repo ファイルを nix が読む)。D3 の nix 属性の根拠。
- このリポジトリの diagramming スキル(`config/claude/skills/diagramming/SKILL.md` の §1 と §2)。D1 と D7 の根拠。

## 執行点

- `pipeline-map.toml`
- `crates/pipeline-map`
- `home/modules/claude.nix`
- `.github/actions/stack-position/action.yml`

## Verification

- `cargo test -p pipeline-map`(単体 15 件と、リポジトリ本体に対する `tests/coverage.rs`)。
- `cargo clippy -p pipeline-map --all-targets -- --deny warnings`、`cargo fmt --check`。
- 陰性対照 3 本: (1) hook 宣言の `node` を 1 つ消すと nix の評価が落ちる、(2) manifest に無い id を書くと nix の評価が落ちる、(3) コメントのポインタを 1 つ消すと `cargo test` が落ちる。
- `claudeHooksSpec` の評価結果が、変更の前後で JSON として一致し、`node` が漏れていない(`nix eval` で比較)。
- CI の「stack-position action path gate covers every path the nix code reads」が通る(`pipeline-map.toml` を `BUILD_INPUTS` から外すと落ちることも確認した)。

## Amendment (2026-10-24 — D4 のクラスに「ルータの表」を足す)

ツール呼び出しに付く検査のうち、拒否だけを出す 15 件と記録役 4 件は、Claude Code に別々には登録せず、
`hook-router`(1 つのプロセス)が規則の表(`crates/hook-router/src/rules.rs`)に従って走らせる
(`docs/claude/hook-router.md`)。この 19 件は `claudeHookDeclarations` の要素を持たないので、D4 の 7 クラスの
どれにも数えられず、`auto = true` のノードが「登録箇所が無い」になる。

D4 に **8 つ目のクラス**を足す: 8. `crates/hook-router/src/rules.rs` の規則の表の、`node: "<id>",` の行
(`#[cfg(test)]` より前、コメントアウトした行は除く)。ポインタの書式は、この表の 1 行がそのままポインタで、
`pipeline-node:` コメントは要らない(表が型付きの Rust なので、id の書き間違いは `cargo test` が「定義が無い」で
落とす)。ルータ自身の登録(`claudeHookDeclarations` の 2 件、`hook-router-pre` / `hook-router-post`)は、
クラス 1 のままで、nix 評価時にも検査される。

### 執行点

- `crates/pipeline-map/src/lib.rs`
- `crates/hook-router/src/rules.rs`
- `home/modules/claude.nix`
