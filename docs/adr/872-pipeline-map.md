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
  (2026-10-27 の Amendment: timer / launchd agent は、nix の評価時に確かめる `dotfiles.pipeline.timerNodes` に改めた。)
  nix の評価が及ばない形式(YAML、TOML、シェル文字列の引数)だから、検出は `cargo test` が担う。

後者を言語ネイティブの属性にするには、登録の宣言を型付きのリストへ作り直す必要がある。今回の範囲を超えるので、その移行は「意図は決まっていて未着手」とし、検査器の抽出規則へ 1 本足すだけで済む設計にしておく。

### D4: カバレッジ検査の母集団は、自動発火する登録箇所の閉じた 7 クラス

1. `claudeHookDeclarations` の各要素
2. `run ${registerCodexHooks}` / `run ${registerCopilotHooks}` の呼び出し
3. `systemd.user.timers.*` / `launchd.agents.*` / `systemd.user.tmpfiles.rules` の束縛(2026-10-27 の Amendment: 前 2 つは `dotfiles.pipeline.timerNodes.<名前> = "<id>";` の行)
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
- このリポジトリの diagramming スキル(`config/agents/skills/diagramming/SKILL.md` の §1 と §2)。D1 と D7 の根拠。

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
(`docs/agents/hook-router.md`)。この 19 件は `claudeHookDeclarations` の要素を持たないので、D4 の 7 クラスの
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

## Amendment (2026-10-27 — D3・D4 のクラス 3 のうち、timer と launchd agent を、nix の評価時に確かめる)

D3 は、timer / launchd agent のポインタを直前のコメント `pipeline-node: <id>` にし、検出を `cargo test` に任せた。
コメントを書き忘れる・書き間違えると、ローカルの `hms` は通り、CI の `cargo test` で初めて落ちる(#992)。

Issue の案は、`node` を必須の引数にする nix のヘルパ(`mkTimer` など)を経由させる形だった。ヘルパは、
生の `systemd.user.timers.*` を書けば迂回できる(迂回の検出は、結局コメントの走査に戻る)ので、採らない。
採るのは、**最終の config を列挙する検査**である:

- `home/modules/pipeline.nix` が、option `dotfiles.pipeline.timerNodes`(`attrsOf str`、名前 → ノード id)を置く。
  各モジュールは、timer / agent を定義するそばに、`dotfiles.pipeline.timerNodes.<名前> = "<id>";` を 1 行書く
  (同じ名前の systemd timer と launchd agent は 1 行)。
- 同じモジュールが、全モジュールの合成後の `config.systemd.user.timers` と `config.launchd.agents` の名前を
  列挙し、`assertions`(home-manager が `activationPackage` の評価時に強制する)で、次の 2 つを落とす:
  対応付けの無い名前、`pipeline-map.toml` に無い id。生の `systemd.user.timers.*` を書いても、最終の config に
  現れるので、迂回できない。
- 検査の対象は、**このリポジトリの中のファイルが定義した** timer / agent の名前だけにする。定義したファイルは、
  モジュールシステムが `options.systemd.user.timers.definitionsWithLocations`(launchd は
  `options.launchd.agents`)に残しているので、`toString ../..` で始まるファイルの定義を数え、最終の config の名前と
  突き合わせる(`lib.mkIf` が偽の定義は数えない)。上流のモジュール(`services.gpg-agent` が作る launchd agent
  など)と、別の flake(private wrapper)が作る timer / agent は、こちらの pipeline-map.toml にノードを持てないので、
  自動で対象から外れる。許可リストは持たない。最初の版は、最終の config の名前を全部数える形で、private wrapper が
  定義する timer 4 本(名前はここに書かない)を未対応として、`hms .` の評価を落とした(2026-10-27、実機の適用で
  発見。dotfiles 単体の 3 ホストの評価では見えなかった)。
- クラス 3 の `cargo test` 側は、`dotfiles.pipeline.timerNodes.<名前> = "<id>";` の行を登録箇所として数える
  (1 行 1 束縛の形だけを読む。まとめ書きは問題にする)。生の `systemd.user.timers.*` へのコメントは、もう求めない
  (評価時の検査と二重にしない)。`systemd.user.tmpfiles.rules`(名前の無いリスト)は、直前のコメントのまま。
- **逆向き**(`auto = true` のノードに登録が 1 つも無い)は、`cargo test` に残す。登録は `lib.mkIf (isLinux && …)` の
  条件付きで、1 ホストの評価では、ほかのホストの登録を見られない。そのため、評価時には逆向きを確かめない。
- activation(`home.activation.*` の中の registrar 呼び出し)・Codex / Copilot の登録呼び出し・statusline は、
  今までどおりコメント `pipeline-node: <id>` のまま。シェル文字列の引数や `let` の束縛で、最終の config に
  列挙できる属性が無い(裁定で、この形にはしない)。

### 先行例

- home-manager `modules/default.nix`(`collectFailed` と `moduleChecks`、rev 24f8f8e、2026-10-27 に読んだ):
  `config.assertions` の失敗は、`activationPackage` を取る評価で `throw` になる。ここは、この機構を
  使うだけ(新しい検査器は作らない)。NixOS 側の同種の機構(`nixos/modules/misc/assertions.nix`)は、読んでいない。

### 検証

- `nix eval .#homeConfigurations.<host>.activationPackage.drvPath` が、vega・arcturus・altair の 3 ホストで通る。
- 陰性対照: (1) 対応付けの行を消す(`due-remind`)と、vega の評価が「対応付けが無い: due-remind」で落ちる。
  darwin だけの timer(`downloads-clean`)の行を消すと、altair だけが落ちる。(2) 値を manifest に無い id にすると、
  「定義の無いノード id」で落ちる。(3) 対応付けの行を消すと、`cargo test -p pipeline-map`(`tests/coverage.rs`)が
  「auto = true だが、ポインタする登録箇所が 1 つも無い」で落ちる(逆向き)。

### 執行点

- `home/modules/pipeline.nix`
- `home/common.nix`
- `crates/pipeline-map/src/lib.rs`
- `flake.nix`

`flake.nix` の執行点は `checks.x86_64-linux.pipeline-external-timer`(このリポジトリの外が足した timer で評価が落ちないこと)。

## Amendment (2026-11-08 — 登録箇所に、ログインで起動する常駐のもの(user service・XDG autostart)を加える)

D3 と 2026-10-27 の Amendment は、nix 側の登録箇所として timer と launchd agent だけを認めた。ログイン・セッションの
開始で起動する常駐のもの — `Install.WantedBy` を持つ `systemd.user.services.<名前>` と、XDG autostart の
`xdg.configFile."autostart/<X>.desktop"` — は、自動で発火するのに地図から漏れた。`atop-trace` は、この漏れを避けるために、
常駐の service でなく timer で書き直した(`home/modules/atop-trace.nix` のコメント)。登録箇所の種類に合わせて道具の設計を
曲げる形は逆で、地図の側を直す(wrap-up の気づき)。

- `home/modules/pipeline.nix` の検査を、同じ option `dotfiles.pipeline.timerNodes` のまま、次の名前にも広げる:
  `Install.WantedBy` が空でない、このリポジトリが定義した `systemd.user.services.<名前>`(timer が起動する service は
  `Install` を持たないので、timer の側の 1 行で足りる)と、`autostart-<X>`(`autostart/<X>.desktop`。`Hidden=true` の
  エントリ — システムの autostart を無効にするもの — は何も起動しないので対象外)。option 名は変えない(既存の行と
  `cargo test` の走査を壊さない)。
- `cargo test` 側は、同じ `timerNodes.<名前> = "<id>";` の行を登録箇所として数えるので、変えない。逆向き(`auto = true`
  のノードに登録が 1 つも無い)は、これまでどおり `cargo test`。
- manifest に、レーン `login`(Login / autostart)を足す。

### 検証

- `nix eval .#homeConfigurations.<host>.activationPackage --apply 'p: p.drvPath'` が、vega・arcturus・altair の 3 ホストで通る。
- 陰性対照(対応付けを足す前の評価): vega は「対応付けが無い: autostart-fcitx5, autostart-obsidian」、arcturus は
  「対応付けが無い: webcam-builtin-relay」で落ちた。arcturus の `webcam-builtin-relay` は、これまで地図に載っていなかった
  常駐の service で、この検査が実際に拾った。足したあと 3 ホストとも通る。
- `nix build .#checks.x86_64-linux.pipeline-external-timer`(このリポジトリの外が足したもので評価が落ちないこと)が通る。

### 執行点

- `home/modules/pipeline.nix`
- `home/modules/webcam-builtin.nix`
- `pipeline-map.toml`

## Amendment (2026-11-09 — hook のレーンの自動ノードは、設計を読める文書を指す)

ADR-0007 §5 は「doc を持たない hook(孤立 hook)を作らない」と決めた。この規約は散文のままで、検査が無く、hook の自動ノード
(`session = true` のレーンの `auto = true`)の 5 つが `doc` を持たないまま残っていた: `new-tool-guard`・`feedback-target-guard`・
`cmd-hash-log`(tool-call)、`atuin-history`(tool-call、外部の道具)、`agent-turn-log`(stop)。専用の `docs/agents/<名前>.md` が
無いものは、設計と記録先を説明する別の文書(機能全体の文書、記録の方式を決めた ADR)を指せば足りる。`audit` の検査は、`doc` の
実在(従来)に加えて、この「hook なのに `doc` が無い」を見る。

- `crates/pipeline-map/src/lib.rs` の `audit` に、「`session = true` のレーンに属する `auto = true` のノードは `doc` を持つ」を足す。
  timer・workflow など、ほかのレーンには広げない(孤立の再発の記録が無い。広げるのは、再発が出てから)。手動の CLI と skill
  (`auto = false`)は対象外。
- 5 つのノードに `doc` を足した: `new-tool-guard` は `docs/agents/new-tool-guard.md`、`feedback-target-guard` と `cmd-hash-log` は
  それらの出力(`Target:` の閉語彙、`cmd-hashes.jsonl`)を読む `docs/agents/promotion-detect.md`、`agent-turn-log` と `atuin-history`
  は記録の方式を決めた `docs/adr/0011-local-activity-log-capture.md`。専用の文書を新しく書くことはしない(読む先が既にある)。
- 「doc 欄が無いノードは 3 件」としていた見積りは誤りで、全体では 22 件、hook のレーンでは 5 件だった。検査は hook のレーンに
  限るので、残る 17 件(timer・workflow・git hook など)は検査の対象外のまま。

### 検証

- 単体テスト 4 件(`a_hook_node_without_doc_fails`・`a_hook_node_with_an_existing_doc_passes`・`a_non_hook_lane_does_not_need_a_doc`・
  `a_manual_hook_lane_node_does_not_need_a_doc`)が通る。`doc` を足す前のリポジトリ本体に対する `tests/coverage.rs` は、5 つのノード
  ぴったりを名指しして落ちた(陰性対照)。足したあとは通る。
- `cargo run -p pipeline-map -- render` の差分が空(`doc` は図の生成が読まない欄)。

### 執行点

- `crates/pipeline-map/src/lib.rs`
- `pipeline-map.toml`

## Amendment (2026-11-09 — timer と launchd agent の形を共有する関数は、2026-10-27 の裁定の外)

2026-10-27 の Amendment は、`mkTimer` のような**ポインタ強制の執行点としての関数**を退けた(生の `systemd.user.timers.*` で迂回でき、
強制は `home/modules/pipeline.nix` の評価時の assertion が持つため)。同じ文を、timer と launchd agent の**定義の形を共有する**
関数まで退ける裁定としては読まない。8 つの組(`due-remind`・`detect-drift`・`hms-follow`・`promotion-detect`・`herdr-issue-counts`・
`gpg-subkey-remind`・`git-audit-worktrees`・`git-checkout-sync`)が、oneshot の service、その service を起動する timer、`timers.target`
への install、同じ PATH 一覧を渡す launchd agent、を手で複写していた。

- `home/lib/timer-pair.nix` の `timerPair { … }` が、その形を 1 つにする。説明・プログラムと引数・PATH・スケジュール・launchd 側の
  スケジュールは引数に残る。`dotfiles.pipeline.timerNodes.<名前> = "<id>";` の 1 行は**書かない**。ポインタが呼び出し側から消えると、
  assertion が「関数が構成上満たすもの」になり、検査の意味が薄れる。呼び出し側が手で書く。
- `schedule` は 2 つの閉じた形(`{ calendar; persistent; … }` と `{ afterBoot; every; … }`)だけを受ける。`Persistent=` は
  `OnCalendar=` の timer にしか効かない(#442)ので、経過時間の timer に `persistent` を書くと評価が落ちる。死んだ宣言を書けなくする。
  これで、`git-audit-worktrees` と `git-checkout-sync` の `Persistent = true`(`OnBootSec=` + `OnUnitActiveSec=` だけの timer なので効いていなかった)が消えた。
- 生の `systemd.user.timers.*` で書いた timer も、これまでどおり assertion が `timerNodes` との対応を見る。`git-auto-prune` は
  1 つの service が `ExecStart=` を 2 行持ち、launchd 側が別名の 2 つの agent に分かれる形なので、`timerPair` を使わず、そのまま残した。

### 検証

- 配備される unit と plist の、新旧の JSON(vega と arcturus の `systemd.user.services` / `timers`、altair の `launchd.agents`。`detect-drift` と
  `local-state-backup` を有効にした構成)を比べた。差は、上の 2 つの timer から `Persistent` の行が消えた 4 行(vega と arcturus に 2 行ずつ)だけで、
  それ以外の unit と plist はバイト単位で同一。
- `schedule` の検査: 有効な 2 つの形は通り、経過時間 + `persistent`、カレンダー + `accuracy`、形なし、`afterBoot` だけ、の 4 つは評価が落ちる。

### 執行点

- `home/lib/timer-pair.nix`
- `home/modules/worktree.nix`
- `home/modules/due-remind.nix`
