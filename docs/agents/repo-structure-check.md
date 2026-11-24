# repo-structure-check — リポジトリ構造の不変条件を、`cargo test` で検査する

実装: `crates/repo-structure-check`(Rust のライブラリ。bin は持たない)
走る場所: CI の `Rust workspace` ジョブの `cargo test --workspace`。手元は `cargo test -p repo-structure-check`
Issue: #1434(manifest)、#1435(hook の入出力)、#1436(Stop の block と再入)、#1437(逃げ道)、#1440(ホスト名)(親の tracking Issue は #1433)

## 動機

このリポジトリの規約は、文書に書いたまま検査が無いものが多い。外部依存の版は、97 の crate が各自の manifest に
書いていて、同じ依存の版と feature が crate ごとに分かれていた(`tempfile` は `^3.27.0` が 77、`^3` が 2、
`serde` は `^1.0.229` が 17、`^1` が 4、`toml` は 7 crate が `1.1.6`、4 crate が `1.1.8`)。依存を 1 つ上げるたびに、
全 crate の manifest を探して揃える必要があり、揃え忘れても何も落ちなかった。

構造の不変条件を `cargo test` で検査する型は、すでに 2 つある(`crates/record-layer/tests/dotfiles_declaration.rs`、
`crates/pipeline-map/tests/coverage.rs`)。検査が 3 つ以上になるので、責務のちがう既存の crate に足さず、1 つの
crate に集める。

## 検査

### manifest(`manifest::check_tree`)

| 検査 | 内容 |
|---|---|
| 継承 | メンバーの `[dependencies]`・`[dev-dependencies]`・`[build-dependencies]`(`[target.<cfg>.…]` も)の外部依存は、`name.workspace = true` で継承する。`path` の依存(workspace 内の crate)は対象外 |
| root の表 | root の `[workspace.dependencies]` の各項目は `version` と `default-features` だけを持つ(`path` は許す)。feature はメンバーが足す。どのメンバーも使っていない項目は残さない |
| 依存ゼロの例外 | `pr-title-check` と `pr-contract-check` は、`[dependencies]` と `[build-dependencies]` を空に保つ。`[dev-dependencies]` の明示の指定は許す |

依存ゼロの 2 つは、他リポジトリの CI(`.github/actions/pr-title/action.yml`)が、root の `Cargo.toml` とこの 2 crate
だけを sparse-checkout して `cargo build` する。依存を足すと、他リポジトリの build が重く、壊れやすくなる。
`戻せない: 外部契約` に当たるので、例外は名前で列挙する(`manifest::ZERO_DEPENDENCY_CRATES`)。

### hook の入出力(`hook_io::check_tree`)

`crates/*/src/**/*.rs` のうち、コメントの行と、ファイル末尾のテストの節(`#[cfg(test)]` から後)を除いた行を読む。

| 検査 | 内容 |
|---|---|
| 返答の封筒 | `hookSpecificOutput` の文字列は `agent-session`(`Reply::render`)だけが書く。ほかの crate の `src/` に書いたら落ちる |
| 標準入力 | hook の bin(`src/main.rs`・`src/entry.rs`・`src/bin/*.rs`)は、`std::io::stdin()` を直接呼ばず、`agent_session::entry::read_stdin` を呼ぶ。端末かどうかを調べるだけの行(`is_terminal`)は読みではない |
| Stop の block と再入 | Stop の block(`"decision"` と `"block"` が同じ行)を手で書く行と、`stop_hook_active` を読む行は、`agent-session`(`Reply::render` と `StopInput`)だけが持つ。例外は無い(ADR-1472) |
| 例外の鮮度 | 例外のファイルが、違反の形をもう持たない(または、ファイルが無い)と、例外のほうが落ちる |

例外は、どれも**恒久**の例外で、`hook_io::ENVELOPE_EXCEPTIONS`(返答を書かず、外から来た JSON を読む・読み替える 3 ファイル)と
`hook_io::STDIN_EXCEPTIONS`(hook でない CLI の 4 つと、遅延して読む `adr-number`)に、理由つきで列挙する。違反を先送りする
一覧には使わない。

### 逃げ道(`escape::check_tree`)

名前・スコープ・真偽・案内は `agent_session::escape` の表が唯一の正本である(ADR-1040 の 2026-11-09 の Amendment)。

| 検査 | 内容 |
|---|---|
| 表の外の綴り | guard の `src/`(コメントとテストの節を除く)に、逃げ道の名前の形(`SKIP_` で始まる、または `_ALLOW`・`_BYPASS`・`_SKIP` で終わる全大文字の文字列)を直書きしない。拒否文の `NAME=1` の直書きも同じ。名前と案内は `Hatch` から導く |
| skip ファイル | `join("skip")` を直書きしない。ディレクトリだけを `Hatch::active` に渡す |
| 退役した名前 | 改名した旧名 8 個と、廃止した旧フラグが、凍結する ADR(`docs/adr/`)とこの検査のほかに、ツリーのどこにも残っていない(テストとコメントも読む)。旧名は別名として読まない |
| 表と文書の対(`tests/tree.rs`) | `escape::ALL` の各項目の `doc` に、自分の名前(セッション単位を持つなら `SKIP_<STEM>`、呼び出し単位を持つなら `<STEM>_ALLOW`)が書かれている |

例外は無い。退役した名前の一覧は `escape::RETIRED`(後継の名前つき)。

### ホスト名(`host_names::check_tree`)

`home/**/*.nix` を Nix の字句解析(コメントを読まず、文字列の中身と `${…}` の中を区別する)で読む。ホスト名の正本は
`home/hosts/*.nix` のファイル名(`vega`・`arcturus`・`altair`)。module は機体の名前を、`home/modules/host.nix` が宣言する
option `dotfiles.host.name` で受け取る。

| 検査 | 内容 |
|---|---|
| 文字列のホスト名 | Nix の**文字列**に、ほかの機体のホスト名が単語として現れたら落ちる(自分の機体のファイル、`home/hosts/<host>.nix` と `home/hosts/<host>/**`、の中は自分の名前を持ってよい)。コメントは読まない。説明は `<host>` のように書く |
| 自分の名前の宣言 | `home/hosts/<host>.nix` は `dotfiles.host.name = "<host>";` を持つ。ファイル名と宣言がずれると、`~/.config/dotfiles/host`(`hms` が機体を決める元)が別の機体を指す |
| マーカーの書き手 | `~/.config/dotfiles/host` を書く(`xdg.configFile."dotfiles/host"`)のは `home/modules/host.nix` だけ。`xdg.configFile.<名前>.text` は文字列を連結する型なので、書き手が 2 つあると評価は通り、マーカーが `vega\n\nvega\n` のように壊れる(`lines` の合成を評価して確かめた) |

`dotfiles.host.name` は既定値を持たない。名前を宣言しない機体は、評価が「option を使っているが定義が無い」で落ち、別の機体を
指すマーカーを配備しない。綴りは `[a-z][a-z0-9-]*`(型で縛る)。例外は無い。

### 側(identity)の分岐(`side_symmetry::check_identity_branches`、ADR「個人と会社は対称な対」)

`home/**/*.nix` を同じ字句解析で読む。公開の module(`home/modules/**`)は、機体が個人か会社かで分岐しない。側ごとに違う値は
typed な option にして、identity のファイル(`home/identities/**`)と host のファイルが値を与える。

| 検査 | 内容 |
|---|---|
| 側を読まない | `home/modules/**` のコード(コメントを除く)に `dotfiles.identity` が現れたら落ちる。例外は、option を宣言して環境変数へ変換する `home/modules/identity.nix` だけ。説明のコメントに書くのは構わない |
| 番兵 | `home/modules/identity.nix` が無い、または `dotfiles.identity` を宣言していないと落ちる(検査が空振りして通らないため) |

### 環境変数・ホーム・XDG(`process_env::check_tree`)

`crates/*/src/**/*.rs` のうち、コメントの行と、ファイル末尾のテストの節(`#[cfg(test)]` から後)を除いた行を読む。環境変数が
空のときを未設定として扱う規則(bash の `${VAR:-}`)と、`${XDG_*_HOME:-$HOME/<既定>}` の形が、約 20 の crate に手で
複写され、複写ごとに空の扱い(`XDG_STATE_HOME=""` を未設定とするか)と `HOME` が無いときの扱い(空のパス・エラー・panic)が
違っていた。解決は `crates/process-env`(依存ゼロ)が持つ。

| 検査 | 内容 |
|---|---|
| XDG の変数名 | `XDG_CONFIG_HOME`・`XDG_STATE_HOME`・`XDG_CACHE_HOME`・`XDG_DATA_HOME` の名前の文字列は、`process-env` だけが持つ。ほかの crate の `src/` は、`process_env::Xdg::<種類>.resolve()`・`resolve_lossy()`・`resolve_under(home)` で基準を解決する |
| 補助関数の複写 | `env_nonempty`・`non_empty_env`・`nonempty_env`・`xdg_dir`・`xdg_state_home`・`config_home`・`env_path`・`home()` という名前の関数(と、前の 3 つのクロージャ)を、`process-env` の外に定義しない。非空の環境変数は `process_env::var_nonempty`、`HOME` は `process_env::home` |

例外は、どれも**恒久**で、理由つきで `process_env.rs` の定数に列挙する。`XDG_EXCEPTIONS`: `local-state-backup` は XDG の変数名を
子プロセスへ引き渡す名前の一覧として持つ(解決ではない)。`HELPER_EXCEPTIONS`: `backup-runtime` の `nonempty_env` は、値を返さず、
空なら失敗にする検証で、非空の値を読む仕事ではない。例外のファイルが違反の形を失ったら、例外のほうが落ちる。

`HOME` の 1 行読み(`std::env::var_os("HOME")`)は、論理を持たない定型なので対象にしない。基準ディレクトリの**解決**と、非空の判定の
**複写**が対象。

#### 寄せたときに挙動が揃った点

複写ごとに違っていた 3 点を、`process-env` の規則(bash の `${VAR:-}` と同じ)に揃えた。いずれも、通常の環境(`HOME` が
設定されていて、XDG の変数は未設定か非空)では変わらない。

- `XDG_STATE_HOME=""` や `XDG_CONFIG_HOME=""`(空)を、**未設定として扱う**。`due-remind`・`feedback-trial`(設定ファイルの場所)・
  `model-equivalence`(codex-plan の設定の場所)・`local-state-backup`・`promotion-detect`(状態の場所)は、これまで空を設定済みとして、
  相対パスを作っていた。
- `HOME` が無いときの結果は、`resolve_lossy` では空の `HOME` として相対パス(`.local/state` など)に揃った。これまでは
  `/.local/state`(絶対)と `.local/state`(相対)が混ざっていた。`HOME` を要る呼び出し側(`git-prune`・`git-audit-worktrees`・
  `due-remind`・`codex-plan`)は、これまでどおり失敗にする。
- `HOME=""`(空)は、`verdict-escalate`・`esa-mirror-context`・`daily-desk-reminder` で、これまで「無い」(`None`)としていたが、
  相対パスを作るようになった(シェルの `${HOME}/…` と同じ)。

### 暦の変換(`civil_time::check_tree`)

`crates/*/src/**/*.rs` を、`process_env` と同じく(コメントの行と、末尾のテストの節を除いて)読む。Howard Hinnant の
`days_from_civil` / `civil_from_days` が、8 つの crate(`agent-session`・`issue-inbox`・`worklog`・`detect-drift`・`github-audit`・
`daily-desk-reminder`・`model-catalog`・`agent-turn-log`)に手で複写され、さらに 3 つの crate(`agent-session` の transcript、
`due-remind`、`verdict-escalate`)が `days_from_civil` の本体を持っていた。月日の型も `i64` と `u32` が混ざっていた。変換は
`crates/civil-time`(依存ゼロ)が持つ。

| 検査 | 内容 |
|---|---|
| 暦の定数 | `719468` と `146097`(数のかたまり単位で比べる。`1_460_970` のような別の数には当たらない)を、`civil-time` の外の `crates/*/src` に書かない。`tests/` の独立な実装(逆変換を手で書いて突き合わせる)は対象にしない |

例外は無い。

### Web の資産(`web_assets::check_tree`、ADR-1677)

このリポジトリ内の `*.html`・`*.htm`・`*.css`(`.` で始まる名前、`target/`、`node_modules/` を除く)を読み込む。表示のたびに第三者
(Google Fonts など)へリクエストが送信されると、閲覧した事実と IP アドレスがその第三者に渡ってしまう。書体は機体のものを既定とし、第三者からの書体・
スタイル・スクリプトの読み込みは禁止する(ADR-1677 の D6。スクリプトを含めることの禁止は ADR-1703 の D4 で明確化された)。外部スクリプトが
必要な場合は、同梱して自サイトから配信し、`sha256` を pin する。コメント内も読み込む(コメントに書いた参照の削除忘れを防ぐため)。

| 検査 | 内容 |
|---|---|
| 書体の配信元 | `fonts.googleapis.com`・`fonts.gstatic.com` への参照が、どこにあっても落ちる。下の形に当たらない書き方も拾う |
| `<link>` | `rel` が `stylesheet`・`preconnect`・`dns-prefetch`・`preload`・`modulepreload` のどれかで、`href` が外部(`http://`・`https://`・`//`)のとき落ちる。`rel="canonical"` や `rel="icon"` の外部 URL は読み込みでないので対象にしない |
| `@import` | 先が外部のとき落ちる(`url("…")`・引用符つき・`url(//…)` のどれも) |
| `@font-face` | ブロックの中の `url(...)` が外部のとき落ちる。`@font-face` の外の `url(...)`(背景画像など)は、書体・スタイルの読み込みでないので対象にしない |
| `<script>` | `src` が外部のとき落ちる(引用符つき・引用符なし・`//…`・大文字小文字・属性が改行をまたぐ形のどれも)。`<scripted>` のような別のタグ、自分のサイトのスクリプト、インラインのスクリプトは対象にしない |
| スクリプトの import | `import("…")` と `from "…"` の先が、引用符つきの外部 URL のとき落ちる。引用符の無い散文の `from https://…` は対象にしない |

例外は無い。範囲はこのリポジトリの内側だけで、ほかのリポジトリの面はこの検査では守られない(他のリポジトリへの配り方は、
ADR-1677 の範囲外)。空のツリーを読んで通ってしまわないよう、`tests/tree.rs` が、裁定ページの `template.html`・`plan-view.css`・
`model-equivalence` の `page.html` を読んでいることを確かめる。

### デザイントークン(`web_variables::check_tree`、ADR-1685)

正本 `config/web/variables.css` を読む。値は、16 進(`#rgb`・`#rrggbb`)・`var(--名前)`・`light-dark(明, 暗)` だけを解釈する。コメントは
読まない(説明のコメントに、規則の綴りを書いてよい)。

| 検査 | 内容 |
|---|---|
| 層 | 原色(`--ref-*`)は 16 進の値だけを持つ。役割・尺度の層が 16 進を直書きしたら落ちる(原色を `var()` で指す) |
| 役割の集合 | `REQUIRED_ROLES` の 16 の役割(`--bg`・`--surface`・`--text`・`--text-muted`・`--border`・`--border-subtle`・`--accent`・`--on-accent`・`--accent-text`・`--focus`・`--ok`・`--ok-bg`・`--ng`・`--ng-bg`・`--warn`・`--warn-bg`)が、明暗の両方で色に解けること。欠けても、解けなくても(`oklch()` など)落ちる。読めない色を、通してしまわない |
| コントラスト | `PAIRS` の 21 の組を、明暗の両方で WCAG 2.2 の式で計算する。文字は 4.5:1(本文・弱い文字・リンク・強調色の上の文字・成功/誤り/注意の文字)、図形は 3:1(境界・フォーカスの輪・強調色) |
| 規則の形 | `color-scheme: light dark`、Artifact のホストの契約 `:root[data-theme="light"]`・`:root[data-theme="dark"]`、`prefers-reduced-motion` の規則が要る。`@media (prefers-color-scheme)` の規則(明暗の二重書き)は落ちる |

例外は無い。範囲は正本 1 ファイルの値と形で、各面が自分のテーマで上書きした値には届かない(各面を正本に従わせる段で、同じ組を
面のテーマに当てる。未実装、意図あり)。各面が持つ複写の検査は、`.github/actions/design-pin`(`check.sh --selftest` を CI が走らせる)。
空のツリーを読んで通ってしまわないよう、`tests/tree.rs` が、16 の役割と尺度の主な名前の宣言が正本にあることを確かめる。

### 共有の部品(`web_components::check_tree`、ADR-1687)

`config/web/components.css` を正本 `config/web/variables.css` の宣言と照合して読み込む。コメントは無視する。

| 検査 | 内容 |
|---|---|
| 色 | 宣言の値(最初の `:` より後)に、16 進の色(`#rgb`・`#rrggbb` など)の直書きがあれば落ちる。セレクタの `#add` のような id は、色と取り違えない |
| 変数 | `var(--名前)` の名前が、正本に宣言されていなければ落ちる |
| 必須の部品 | `.btn-main`・`.btn-sub`・`.btn-link`・`.actions`・`.choice`・`.status`・`.alert-ng`・`.empty`・`.skip-link`・`.table-cards`・`:focus-visible` のどれかが欠ければ落ちる |
| 操作部の大きさ | `.btn` が `min-height: var(--target);` を先頭に持たなければ落ちる |
| 置き方 | `.actions` が `flex-direction: row-reverse;` と `justify-content: space-between;`(主な操作を右端、副の操作を左端)を持たなければ落ちる |
| 二重書き | `@media (prefers-color-scheme)` の規則があれば落ちる |

例外はない。この検査が対象にするのは、部品の CSS 1 ファイルのみである。各面が部品を使う HTML の検査は、`未実装(意図あり)` である。

### 見本ページと生成した値(`web_specimen::check_tree`・`web_variables::check_tokens_json_tree`、ADR-1700)

見本ページ `config/web/specimen.html` は、正本を複写せず実際に読み込み、役割と部品のすべてを並べる。`variables.json` は、CSS から生成した派生物。

| 検査 | 落ちる条件 |
|---|---|
| 読み込み | `variables.css`・`components.css`・`specimen.css` を相対パスで読み込まない |
| 持たないもの | インラインの `style` 属性、`<style>`、インラインの `<script>`(本体のあるもの)、第三者の URL(`http://`・`https://`) |
| スクリプト | `<script>` は、本体の空の `<script src>` で、同一オリジンの `behavior.js`・`specimen.js` だけを読み込める。ほかの `src`・本体のあるスクリプトは落ちる。2 つのどちらかを読み込まなくても落ちる(動く見本を、共有の部品から外さない。ADR-1703 D2) |
| 役割の網羅 | 役割(`REQUIRED_ROLES`)に、`data-role` の見本か、`specimen.css` の `var(--役割)` で塗る規則が欠ける |
| 部品の網羅 | `components.css` のセレクタが使うクラスを、見本の HTML が使わない(部品を足して見本に載せ忘れた) |
| 値 | `specimen.css` に 16 進の直書きがある、または `var()` の名前が正本に宣言されていない |
| 基本 | `lang="ja"`・viewport・`title`・`main`・`h1` が無い |
| `variables.json` | 正本の CSS から生成した内容と違う(再生成: `cargo run -q -p repo-structure-check --example web_variables_json > config/web/variables.json`) |

例外はない。見本は、面ごとのテーマの上書き(accent の 4 つ)を持たない。

### 振る舞いの共有部品(`web_behavior::check_tree`、ADR-1703 D2)

`config/web/behavior.js`(共有の部品)と `config/web/specimen.js`(それを呼ぶ動く見本)を読む。コメントは読まず、文字列の中は読む。
共有の部品は、CSP `style-src 'self'` の下で動き、1 枚だけの HTML の区間 pin(ADR-1700)でインラインの `<script>` に複写でき、失敗を握りつぶさない。

| 検査 | 落ちる条件 |
|---|---|
| 外部の URL | `http://`・`https://` がある(第三者から読まない。ADR-1677 D6) |
| style の注入 | `.style`(`.stylesheet` のような続きの名前は除く)・`cssText`・`setAttribute("style", …)`。出し入れは、クラスと `hidden` 属性で行う |
| HTML・コードの注入 | `innerHTML`・`outerHTML`・`insertAdjacentHTML`・`document.write`・`eval(`・`new Function` |
| モジュールの構文 | 行頭の `import`・`export`(インラインの `<script>` に複写できなくなる) |
| 失敗の握りつぶし | 本体の空の `catch`(`catch {}`・`catch (e) {}`)、本体の空の関数を渡した `.catch(...)`(`() => {}`・`e => {}`・`function (e) {}`) |
| 公開する名前 | `behavior.js` が、`"use strict"`、`window.pageBehavior`、関数 `saver`・`keyguard`・`viewstate` と、その公開を持たない。`specimen.js` が、この 3 つを呼ばない |

限界: 正規表現リテラルの中の引用符・`//` は、文字列やコメントと取り違える(2 つのファイルは、正規表現リテラルを書かない)。実際の振る舞い
(保存中 → 保存済み、失敗と再保存、古い結果が新しい結果を上書きしない、ボタンの Enter・Space を奪わない、読み込み中・失敗・空の出し分け、CSP 違反 0 件)は、
`uv run --script config/web/behavior_selftest.py`(Chromium が要る。CI では走らない)が、見本ページを CSP つきで開いて確かめる。

### 選び方の対応表(`web_selection::check_tree`、ADR-1703)

`config/web/selection.toml` は、作業の種類(ダイアログ・保存表示・トースト・タブなど)ごとに、ブラウザ標準・共有部品・外部
ライブラリ・手書き・使わない、のどれを選ぶかを保持する。指定できる語彙は限定されている(`way` は `platform`・`shared`・`external`・`handwritten`・
`not-used`、`reason` は `requirement`・`no-surface`・`few-surfaces`)。要件 ID の照合先は `config/web/page-types.toml` である。

| 検査 | 落ちる条件 |
|---|---|
| 形 | TOML として読めない、表の先頭に `[[task]]` 以外のキーがある、`[[task]]` が無い、知らないキー、`id` が小文字・数字・`-` でない、`id` の重複、`title` が空 |
| 語彙 | `way`・`reason` が語彙の外。行ごとの必須のキーが欠ける、またはその `way` に書けないキーがある |
| `platform` | `baseline` が `widely` でない(Baseline が widely でない機能は、標準の行にできない) |
| `shared` | `files` が空、`config/web/` の下でない、または実在しないファイルを指す |
| `external` | `sha256` が 64 桁の小文字の 16 進でない(`pin` と `use` が無い場合は「必須のキーが欠ける」) |
| `not-used` | `reason` が `requirement` なのに、`req` が空、または `page-types.toml` に実在しない要件の ID を含む。`no-surface` なのに `req` がある |
| `handwritten` | `reason` が `few-surfaces` でない |

例外はない。`note` は人間が読むための補足であり、検査しない(Baseline の取得日や画面の実測値など、日付付きの事実を記述する)。空のツリーを読み込んで通過して
しまわないよう、`tests/tree.rs` が、5 つの選択肢それぞれの行と、`dialog`・`toast` の行が読み込まれていることを検証する。

### 原則の層(`web_principles::check_tree`、ADR-1708)

`config/web/principles.toml` は、ページの型の要件(`page-types.toml`)が、どの原則から起こされたかを、閉じた語彙の表で保持する。各原則は、
`id`・`title`・`kind`(`principle` = 外部の原則 / `decision` = このリポジトリの決定)・`scope`(`structure` / `general`)・`source`・`retrieved`・`quote`
(逐語の引用)をすべて持つ。各要件は `principles = [...]` で、原則を 1 つ以上指す。

| 検査 | 落ちる条件 |
|---|---|
| 形 | TOML として読めない、表の先頭に `[[principle]]` 以外のキーがある、`[[principle]]` が無い、知らないキー、必須のキーの欠け・空、`id` が小文字・数字・`-` でない、`id` の重複、`retrieved` が `YYYY-MM-DD` でない |
| 語彙 | `kind`・`scope` が語彙の外 |
| 出典 | `kind = "principle"` の `source` が https の URL でない。`kind = "decision"` の `source` がリポジトリ内に実在しない、または `quote` がそのファイルに、そのまま含まれない |
| 要件 → 原則 | 要件に `principles` が無い・空・配列でない、または実在しない原則の ID を含む |
| 死んだ原則 | `scope = "general"` の原則が、どの要件にも指されていない(`scope = "structure"` は、構造パターンの表が指すので対象外) |

外部の原則の `quote` は、検査では原文と照合できない(ネットワークを使わない)。載せるときに、取得した原文と逐語で照合した(取得日は `retrieved`)。取り直せなかった
原則は載せない。空のツリーを読み込んで通過してしまわないよう、`tests/tree.rs` が、外部・決定・構造の各行と、要件 C3 の結びつきが読み込まれていることを検証する。

### 構造の規則(`web_structures::check_tree`、ADR-0000)

`config/web/structures.toml` は、課題の性質の軸(`[[axis]]`。独立性・比較・反復・情報量・取り消し・件数・順序)と、構造パターン(`[[pattern]]`。`step`・`single-column`・`document`)を、
閉じた語彙で保持する。`fits` は、パターンが許す軸の値(空の表 = 何でも許す)、`default = true` は既定の構造、`measure` は描画したページで測れる約束
(`viewport-fit` = ページ全体が縦にスクロールしない)である。リポジトリ内のすべての `*.html`・`*.htm`(`web_assets` が走査する集合)は、`<head>` の `<meta>` で構造を宣言する。

| 検査 | 落ちる条件 |
|---|---|
| 表の形 | TOML として読めない、先頭に `[[axis]]`・`[[pattern]]` 以外のキーがある、知らないキー、`id` が小文字・数字・`-` でない・重複、軸の値が 2 つ未満・重複、`measure` が語彙の外、`default = true` が 2 つ以上、`fits` の軸・値が語彙に無い、`principles` が実在しない |
| 死んだ原則 | `scope = "structure"` の原則を、どのパターンも、どの要件も指していない |
| 宣言の有無 | HTML に `<meta name="page-structure">` が無い(ページを足すとき、宣言を忘れられない) |
| 宣言の語彙 | 構造が語彙に無い。`page-axes`(`軸=値;軸=値`)の軸・値が語彙に無い、`軸=値` の形でない、軸が重複している |
| 整合 | 宣言した構造の `fits` に挙げた軸を宣言していない。軸の値が `fits` の外にあるのに、`page-structure-deviation` に理由が無い。軸の値が既定の構造に合う(既定の `fits` の軸をすべて宣言し、値が許す範囲内)のに、別の構造を選び、理由が無い |

宣言を免除する一覧は持たない(例外を持たない)。属性は、`name="..."`・`content="..."` の二重引用符の形だけを読む。`<title>` を先頭に要求するページ(Artifact の断片)は、`<title>` の後ろに置く。
空のツリーを読み込んで通過してしまわないよう、`tests/tree.rs` が、表の 3 つの構造と既定の行と、既知の 3 つの HTML(裁定ページの雛形・見本ページ・モデル同等性マップ)が走査され、構造を宣言していることを検証する。

### 依存グラフ(`deps::check_tree`、#1446)

各 `crates/*/Cargo.toml` の `[dependencies]`・`[build-dependencies]`(`[target.<cfg>.…]` も)の `path` の依存を辺として読む。
`[dev-dependencies]` は試験の道具なので対象外。**機能 crate(hook・CLI)は、他の機能 crate に依存しない**。機能 crate が別の
機能 crate の関数を呼ぶと、その機能の変更が、呼び出し側の挙動を黙って変える。共有の仕事は、`docs/agents/agent-libraries.md` の
ライブラリが持つ。許される辺は次の 3 つだけ。

| 辺 | 内容 |
|---|---|
| ライブラリへの依存 | `deps::LIBRARIES`(責務表の 11 ライブラリと、同じ目的の共有 crate、依存ゼロの 2 crate)に名前で列挙する |
| 集約役からの依存 | `deps::AGGREGATORS`(`hook-router`・`session-wrapup`、互換入口の `wrapup-stop-gate`)に名前で列挙する |
| 既知の例外 | `deps::EXCEPTIONS` に `(依存する crate, 依存される crate, 理由)` で 1 件ずつ列挙する |

例外は、解消した(辺が消えた)ら、例外のほうが落ちる(鮮度の検査)。列挙した crate が workspace に無いときも落ちる。新しい違反を
足すには、共有の仕事をライブラリへ移すか、例外に理由つきで足す。初回(2026-11-10)の例外は 14 件(`pr-gate` → `new-tool-guard`・
`worktree-lease`、`stack-base-guard` → `pr-gate`、`session-retrospective` → `cmd-hash-log`・`issue-inbox`・`verdict-escalate`、
`promotion-detect` → `due-remind`・`issue-inbox`・`record-layer`、`detect-drift` → `update-own-tools`、`cmd-hash-log` →
`issue-inbox`、`codex-plan-gate` → `new-tool-guard`、`codex-plan` → `codex-launcher`、`github-app-snapshot` → `github-audit`)で、
どれも、共有の仕事を移す先のライブラリの設計が要る(意図は決まっていて、未着手)。

残りの作業の正本は `deps::EXCEPTIONS` の一覧そのものとし、追跡用の Issue は持たない(#1446 を閉じたときに、ここへ移した。
ADR-1529 の置き場 4)。例外を解消すると鮮度の検査が落ちるので、一覧は実体とずれない。解消の方向は、各例外の理由欄に書く。
共通の条件は 2 つある。(1) 状態の置き場を解決する関数(`cmd-hash-log` の `resolve_log_path`、`verdict-escalate` の `ledger_dir`)は、
ライブラリへ移さず、その状態を書く機能 crate に残す。ライブラリへ移すのは、純粋な部分(語彙・判定・型の読み)に限る。
(2) `stack-base-guard` → `pr-gate` は、stack の連鎖の判定(`gates::stack`)を `github-command` へ移すと消える。その前提として、
`pr-gate` の `jqv` を先に整理する。

`claude-plan-model` は `model-catalog` に依存しない(#1446 の最初の確認)。ADR-457 は、Opus Plan Mode の具体モデル ID を、
**インストールされた実行ファイルが持つカタログ**(`latest_per_family`)から読むと決めている(`config/zsh/modules/53-tools-claude.zsh`
も同じ。実行ファイルの更新に pin が遅れないため)。`model-catalog` は、リポジトリが持つ手書きの対応表で、情報の出どころが違う。
読ませると、実行ファイルのカタログに対して遅れる。

### 規則の置き方

- root の表に `default-features = false` を書いた依存(`jiff`、`toml`、`toml_edit`)は、メンバーが `features` で
  必要なものを足す。メンバーは `default-features` を書かない(cargo が無視して警告する)。
- 既定の feature が要るメンバーは、`features = ["default"]` で足す(`toml` の 4 crate)。
- 依存を足すときは、root の表に版を書き、メンバーは `name.workspace = true` にする。Renovate は root の表だけを更新する。

## 検査を足すとき

検査は純粋関数(文字列 → 違反の一覧)で書き、`manifest.rs` の単体テストに、違反を作る fixture(負例)を置く。実際の
ツリーへの適用は `tests/tree.rs` が行う。負例が落ちることを見ずに「違反なし」で通る検査は、検査していない
(`tests/tree.rs` の番兵は、空のツリーを読んで通ってしまうのを防ぐ)。

## 既存手段と代替

- 既存手段: 自前。却下した候補は、manifest を書き換える `cargo-autoinherit`(README に、継承を検査して失敗させる
  モードの記載を見つけられなかった。最終コミットの日付は未確認)と、`cargo-deny` の `bans`(版の重複と wildcard は
  見るが、メンバーに `workspace = true` を要求する項目の記載はない)。出典は、それぞれの README と設定の文書(取得
  2026-11-09)。
- 対抗馬: `crates/pipeline-map/tests/coverage.rs` に足す案(責務が自動発火機構の地図で、manifest や文書の検査とは別)、
  nix の flake check で `cargo metadata` を `jq` で読む案。
- 軸: 検出のみ。manifest の継承は cargo が強制しないので、継承漏れを表現不可能にはできない。root の表に版を 1 回
  書くだけで済む形にして、書き忘れる場所を 1 つに減らした。

## 執行点

- crates/repo-structure-check/src/manifest.rs
- crates/repo-structure-check/src/hook_io.rs
- crates/repo-structure-check/src/escape.rs
- crates/repo-structure-check/src/host_names.rs
- crates/repo-structure-check/src/side_symmetry.rs
- crates/repo-structure-check/src/process_env.rs
- crates/repo-structure-check/src/civil_time.rs
- crates/repo-structure-check/src/deps.rs
- crates/repo-structure-check/src/web_variables.rs
- crates/repo-structure-check/src/web_components.rs
- crates/repo-structure-check/src/web_specimen.rs
- crates/repo-structure-check/src/web_selection.rs
- crates/repo-structure-check/src/web_principles.rs
- crates/repo-structure-check/src/web_structures.rs
- crates/repo-structure-check/src/web_behavior.rs
- crates/repo-structure-check/src/web_assets.rs
- crates/civil-time/src/lib.rs
- crates/process-env/src/lib.rs
- crates/repo-structure-check/tests/tree.rs
- home/modules/host.nix
- Cargo.toml
