# herdr-sidebar-metadata — エージェント毎のモード/モデルを Herdr サイドバーに常時表示する

複数の Claude Code エージェントを Herdr で並走させると、「どのペインが plan モードで
止まっているのか」「どれが bypass で走っているのか」がサイドバーから読めない。
Herdr が自前で検出するのは agent_status(working/blocked/…)とターミナルタイトル
だけで、Claude Code 側の permission mode・モデル・context 使用率は Herdr のデータ
モデルに存在しないからだ。

これを Herdr の **pane metadata**(`pane.report_metadata` API + サイドバー行の
`$name` トークン)で埋める。表示側の行定義は `config/herdr/config.toml`、報告側は
Claude Code の hook(`config/claude/hooks/herdr-claude-metadata.sh`)と
statusline スクリプト(`config/claude/statusline/claude-statusline.sh`)の 2 本。

## なぜ 2 チャネルか

Claude Code は必要な情報を 1 か所で公開していない:

| 情報 | hook input JSON | statusline JSON |
|------|-----------------|-----------------|
| `permission_mode` | ✅ あり | ❌ 無い(mode 変更時に再実行はされる) |
| model / context% / cost / effort | ❌ 無い | ✅ あり(プリ計算済み) |

そのため `herdr-claude-metadata.sh`(hook、source=`claude-hook`)が mode を、
`claude-statusline.sh`(statusline、source=`claude-statusline`)がモデルとメトリクス
を、それぞれ独立に報告する。トークン名は source 間で完全に分離してあり
(`mode_*` vs `model`/`ctx`/`cost`/`effort`)、Herdr 側のマージ仕様がどちらでも
壊れない。実測では source を跨いでトークン名単位でマージされる(0.7.5、未再検証)。

`herdr-claude-metadata.sh` はもう一つ、hook input JSON の `.cwd` から
`git branch --show-current` を取った `branch` トークンも同じ source で報告する
(`worktree/` プレフィクスは表示幅節約のため落とす)。git 失敗時・非 git cwd では
空 = `null` を送るだけで、herdr 外でも無害。mode の変更検知でスキップする経路
では git を呼ばないので、頻発イベント(PreToolUse)でのコストは増えない。

## モードの色分けは「モード毎に別トークン」で表現する

Herdr のサイドバートークンは `{ token = "$mode", fg = "#..." }` の**静的**スタイル
しか持てず、値によって色を変える手段がない。そこで mode は 1 トークンではなく
4 トークン(`mode_plan` / `mode_default` / `mode_accept` / `mode_bypass`)にし、
hook がアクティブな 1 つにだけ値を入れ、残りを `null` でクリアする。config.toml
側は 4 トークンを同じ行に並べて別々の fg を割り当てる — 値を持つのは常に 1 つ
なので、行には 1 色のモードだけが現れる。0.8.2 のバイナリを逆アセンブルして
確認した制約: サイドバー行トークンのスタイルは `fg`(`#RGB`/`#RRGGBB` 厳格。
named color は不可)+ `bold`/`dim` のみで、`bg` は指定できない。

さらに色だけに頼らないよう、ラベルの記号も形で差別化する(色覚多様性対策。
状態は色 + 形の冗長エンコードが定石):

- `◇ plan` — 輪郭のみ(低リスク)
- `◆ default` — 塗り(基準)
- `✓ accept` — チェック(承認済み)
- `▲ bypass` — 三角(警戒)

未知のモード(`auto` / `dontAsk` など)は `mode_default` トークンに `◆ <実名>`
で流す。古い表示を残すより実名表示のほうが正直。

## 配色: Catppuccin Mocha の役割トークン

Dracula から Catppuccin Mocha への着せ替え(2026-09)で、`config/herdr/config.toml`
/ `claude-statusline.sh` / `config/alacritty/alacritty.toml` の 3 ファイルが共有する
role→color 対応表。hex は ADR-0002 に従い各ファイルにリテラルで置く(共通定義
ファイルは持たない)ので、色を変える際は 3 箇所とも手で揃えること。

| 役割 | Mocha トークン | hex | 使用箇所 |
|---|---|---|---|
| mode: plan | blue | `#89B4FA` | sidebar `$mode_plan` |
| mode: default | mauve | `#CBA6F7` | sidebar `$mode_default`(未知モードもここ) |
| mode: acceptEdits | green | `#A6E3A1` | sidebar `$mode_accept` |
| mode: bypass | red | `#F38BA8` | sidebar `$mode_bypass` |
| model 名 | pink | `#F5C2E7` | sidebar `$model` / statusline model |
| リポ名 | peach | `#FAB387` | statusline 先頭(セッション識別子、狭幅でも残す) |
| ctx OK(<60%) | green | `#A6E3A1` | statusline |
| ctx 注意(60–79%)/ fast | yellow | `#F9E2AF` | statusline |
| ctx 危険(>=80%) | red | `#F38BA8` | statusline |
| cost | teal | `#94E2D5` | statusline |
| effort | lavender | `#B4BEFE` | statusline |
| 控えめ情報(branch, metrics 行) | subtext0 | `#A6ADC8` | sidebar `$branch`/`$ctx`/`$cost`/`$effort` |
| 区切り | overlay0 | `#6C7086` | statusline `·` |
| sidebar 背景(端末透過に委ねる、明示) | reset(端末既定背景) | `"reset"` | `config/herdr/config.toml` `[theme.custom].sidebar_bg` |
| sidebar active row 背景(非純正ブレンド) | lavender タイント | `#52567A` | `config/herdr/config.toml` `[theme.custom].active_row_bg` |
| sidebar navigate カーソル行背景 | surface1(手動上書き) | `#45475A` | `config/herdr/config.toml` `[theme.custom].selection_bg` |
| ui accent(ハイライト/ナビ) | lavender(手動上書き) | `#B4BEFE` | `config/herdr/config.toml` `[theme.custom].accent` |

green=「許可/OK」、red=「危険」、yellow=「注意」で全ファイル一貫させ、
サイドバーと statusline で色→意味が食い違わないようにしている(旧 Dracula 版は
cyan がサイドバーでは plan・statusline では cost という食い違いがあった)。

## イベント選定(hook)

同一 command を 5 イベントに登録する(スクリプトが `hook_event_name` で分岐):

- **SessionStart** — 初期値の報告と、前セッションの残留トークンの上書き。
- **UserPromptSubmit** — 最頻の遷移「アイドル中に Shift+Tab → プロンプト送信」を
  1 プロンプト 1 回のコストで拾う。
- **PreToolUse**(全ツール)— ターン中の遷移(plan 承認 → acceptEdits)を最小
  遅延で拾う。全ツール呼び出しで発火するので、前回報告したモードを
  `$XDG_RUNTIME_DIR/herdr-claude-mode.<pane>` にキャッシュし、同じなら jq 1 回で
  即抜ける(python3 の起動コストを毎回払わない)。
- **Stop** — ttl のリフレッシュ。
- **SessionEnd** — 自 source の全トークンを null クリア(残留表示の即時解消)。

PostToolUse は登録しない — PreToolUse と同じ情報で遅延だけ悪い。

## 間引き(statusline)

statusline はストリーミング中 ~300ms 毎に再実行され得る。ソケット書き込みは
(1) 表示を先に stdout へ出してから detach したサブシェルで行い、
(2) 丸め後の値のフィンガープリントが前回と同じなら送らず、
(3) 前回送信から 2 秒未満も送らない。丸め(ctx は整数%、cost は 2 桁 USD)に
よって、実際の書き込みはターンあたり数回に落ちる。

## 消し忘れ対策

主経路は SessionEnd での null クリア(チャネル A)。チャネル B(statusline)には
終了イベントが無いので、両チャネルとも `ttl_ms = 4h` を保険にする — クラッシュや
kill でも残骸は 4 時間で消える。SessionEnd がチャネル B を `applies_to_source` で
横断クリアできるかは未検証のため、初版では ttl 任せにしている。

## `$oshi` — worktree 名(hololive タレント名)のファンマーク

`patches/herdr-worktree-names.patch`(herdr 本体の worktree 名生成を hololive
タレント名の単語リストに差し替えるパッチ、83 名)により、worktree は
`worktree-<name>-<hex4>` という名前を持つ。サイドバーの `workspace` トークンは
この名前をそのまま表示するので、その横にタレント対応のファンマーク(推しマーク)
絵文字を添えると視認性が上がる。herdr 本体・パッチは変更せず、既存の
`pane.report_metadata` チャネルにカスタムトークン `$oshi` を追加するだけで足りる。

- **データ**: `config/herdr/oshi-marks.tsv`(`name<TAB>mark`、非コメント行は
  タレント名 1 行 1 件)。`xdg.configFile` で `~/.config/herdr/oshi-marks.tsv`
  に verbatim 配布(ADR-0002)。マークは各タレントの公式表記をそのまま使い
  (複数絵文字の組・ZWJ シーケンスも切り詰めない)、確証が取れないタレントは
  mark 列を空にする(= サイドバーに非表示。存在しないと決めつけない)。出典
  (各タレントの X プロフィール / hololive 公式)と取得日は TSV ヘッダに記載。
- **抽出元は worktree ディレクトリ名**(`git rev-parse --show-toplevel` の
  basename)であって branch 名ではない。branch は作業中に `feat/...` 等へ
  リネームされることがあり(実例あり)、`$branch` トークンと違って本トークンは
  worktree 生成時の値を指し続ける必要があるため。
- **3 reporter すべてが同じ lookup を行う**: `herdr-claude-metadata.sh` /
  `herdr-codex-metadata.sh` / `herdr-copilot-metadata.sh` が、それぞれの
  branch 取得ロジックのすぐ後で `worktree-*-*` パターンにマッチしたときだけ
  `awk` で TSV を引き、`tokens.oshi` として同じ `pane.report_metadata` 送信に
  同乗させる(SessionEnd/clear では他トークンと同様 null)。
- **`config.toml`**: `rows_by_agent` の 3 節すべて、1 行目の `workspace` の
  直後に `{ token = "$oshi", fg = "#CDD6F4" }` を追加。`fg` はカラー字形が
  引けなかった場合の保険（次節）。
- **整合性**: 名前リストの正本は patch 内の Rust 配列。CI(`ci.yml`
  `oshi-marks.tsv matches herdr-worktree-names.patch talent list`)が patch の
  名前集合と TSV のキー集合を双方向突合し、片方にしかない名前があれば fail
  する(mark 列が空なのは許容 — 行の有無だけを検査)。

## トークンが出ないときの 2 系統(#305)

`$oshi` を入れた直後は出ていたのに、あるとき全ペインから消えた。原因は
**独立した 2 系統の合成**で、片方だけ直しても見えるようにはならなかった。
「実機で見えた」は「動いている」の証明にならない — 見えなくなり方が
2 通りあるため、切り分けは必ず `herdr api snapshot`(値が来ているか)と
画面(値が見えているか)の両方で行う。

### 系統 A — 値が届いていない(データ層)

hook は payload を jq で読むが、かつては `@tsv` の 1 行を単一の `read` で
分解していた。**タブは POSIX の IFS whitespace** なので連続タブが 1 個の
区切りに潰れ、optional フィールドが空だと以降が 1 個ずつシフトして、
末尾の `cwd` が空になる。`cwd` が空なら `branch` も `oshi` も引けない。

```
$ printf 'SessionStart\t\t0\t/tmp/x' | { IFS="$(printf '\t')" read -r a b c d; echo "[$a][$b][$c][$d]"; }
[SessionStart][0][/tmp/x][]
```

現在は 3 本とも `parse_payload()`(jq が 1 フィールド 1 行を出し、逐次
`read` する)に統一し、`--selftest` を CI に接続している。`mode` / `model` が
空でも `branch` / `oshi` は送り、モード表示トークンだけを落とす。

- `config/claude/hooks/herdr-claude-metadata.sh --selftest`
- `config/codex/hooks/herdr-codex-metadata.sh --selftest`
- `config/claude/statusline/claude-statusline.sh --selftest`

`config/copilot/hooks/herdr-copilot-metadata.sh` は最初からフィールド毎に
`jq` を呼んでいたため無傷だった — 切り分けでは「copilot ペインだけ `oshi` が
入っている」が最初の手がかりになった。こちらは payload に event/model を
持たないため `parse_payload()` 型ではなく、action 判定・cwd 抽出・
`settings.json` の model 読み取り・推しマーク照合を関数に切り出して
`--selftest` で検査し、同じく CI に接続している(#390)。

- `config/copilot/hooks/herdr-copilot-metadata.sh --selftest`

### 系統 B — 値はあるが見えない(表示層)

1. **選択行のコントラスト**: アクティブ行の背景は当時 `#45475A`(surface1、
   `[theme.custom].active_row_bg` の旧値)。`fg` 未指定のトークンは既定の控えめ色
   で描かれ、背景とほぼ同化する(画素サンプリングで `(70,73,88)` vs
   `(69,71,90)`)。同じ行でも `$model`(`#F5C2E7`)は読めるが
   `$branch`(`#7F849C`)はほぼ見えない。現在の `active_row_bg` は accent 系の
   青紫タイント `#52567A` に変わっている(下記「アクティブ行の視認性」節)が、
   この節の結論(fg 未指定は背景に同化し得る)自体は変わらない。
2. **絵文字がモノクロ字形**: fontconfig の既定解決順は
   `Noto Sans Symbols2` → `Unifont Upper` → `DejaVu Sans` / `FreeSerif`
   → `Noto Color Emoji` で、カラー絵文字フォントが最後尾に回る。モノクロ
   字形は前景色を継承するので 1 と合成して完全に消える。

対策は `config/fontconfig/conf.d/75-color-emoji-fallback.conf`
(`home/modules/desktop.nix` が配布)。fontconfig の照合順位は
**FAMILY_STRONG > LANG > FAMILY_WEAK** なので、weak な family 追加では
lang カバレッジの広い `DejaVu Sans` などに負ける(実測で推しマーク 94
コードポイント中 18 個がモノクロのまま)。strong binding で追加すると
90/94 がカラー化する。ただし strong を全パターンに効かせると generic
family の弱いエイリアスにも勝ってしまい `fc-match monospace` が
`Noto Color Emoji` を返すため、**端末のフォント(`FiraCode Nerd Font`)を
明示要求したパターンに限定**する。影響範囲は端末だけで、Chrome や COSMIC の
文字送りは変わらない。

残り 4 つ(☠ ☺ ♀ ⚡)は `FiraCode Nerd Font` 自身がモノクロ字形を持つため
`append` では勝てない。`$oshi` に `fg = "#CDD6F4"` を明示しているのはこの
4 つのための二重防御であって、冗長ではない(カラー字形は `fg` を無視する)。

運用上の注意:

- **fontconfig は端末の起動時に読まれる**。`hms` 後に既存の Alacritty
  ウィンドウは変わらない — 目視検証は新しいウィンドウで行う。
- **サイドバーの再描画はイベント駆動**。`herdr api snapshot` に値があっても
  即座に描かれないことがあるので、目視検証は再描画を誘発してから行う。
- Alacritty は ZWJ シーケンスを合成しない(👯‍♀️ は 👯 と ♀ に分かれて出る)。
  TSV の公式表記は切り詰めないので、これは受容する既知の制約。
- 検証は**選択行・非選択行の両方**で行う。片方だけ見て「出ている」と
  判断したのが #301 の見落としだった。

## Codex / Copilot ペイン

Claude 以外のエージェントペインも同じ `pane.report_metadata` API で埋められる。
ただし表示は `$branch` + `$model` の 2 トークンだけに絞っている — mode/ctx/cost/
effort に相当するものは、upstream の一次情報(developers.openai.com/codex/hooks +
openai/codex ソース、docs.github.com hooks-reference、2026-09 時点)を確認した
うえで **確実に取れないため意図的に見送った**。

| データ | Codex CLI | Copilot CLI |
|---|---|---|
| model | hook payload に documented(SessionEnd 以外の全イベント必須フィールド) | payload に無い |
| cwd(→branch) | hook payload に documented | hook payload に documented |
| permission/approval | payload の `permission_mode` は **lossy**(実測では `bypassPermissions` か `default` の 2 値しか出ない)→ 表示しない | 無し |
| reasoning effort | hooks に無い(config.toml か undocumented な rollout JSONL のみ) | n/a |

Claude 版と違い statusline 相当のチャネルが無いので、reporter は各ツールにつき
1 本(単チャネル)。

### Codex: `config/codex/hooks/herdr-codex-metadata.sh`

`~/.codex/hooks.json` の SessionStart / UserPromptSubmit / Stop / SessionEnd に
登録(PreToolUse には登録しない — model が変わる頻度は低く、他 3 イベントで
十分足りるため、Claude 版のような debounce キャッシュも持たない)。model は
hook payload の `.model` から、branch は `.cwd` から git 呼び出しで取る。
`agent_id` が付くサブアージェントイベントは親ペイン共有のため無視する。

登録は `scripts/register-codex-hooks`(variadic な `(event, matcher, command,
timeout)` タプルを何個でも受け取る idempotent jq マージャー。旧
`register-codex-worktree-hooks` を一般化・改名したもので、`home/modules/worktree.nix`
の worktree guard/context hooks もこの同じスクリプトを使う)。書き込み先が
`home/modules/worktree.nix` の activation と同じ `~/.codex/hooks.json` なので、
`home/modules/herdr.nix` の `registerCodexHerdrMetadataHooks` はその後ろに
`entryAfter` で明示的に順序付けている(lost-update 対策、#61 と同種)。

**Codex の hook trust**: `~/.codex/config.toml` の `hooks.state.*.trusted_hash`
はエントリ単位で、command・matcher・timeout の変更や並び替えは trust を無効化
するが、**末尾への追記は既存エントリの trust を壊さない**(確認済み)。ただし
新しいエントリ自体は初回、Codex 側で `/hooks` から対話的に trust するまで
無音で発火しない — 各ホストで switch 後に一度だけ手作業が要る。

### Copilot: `config/copilot/hooks/herdr-copilot-metadata.sh`

Copilot CLI の hook payload には event 名も model も乗らない(公式リファレンス
確認済み: 全イベント共通で `sessionId`/`timestamp`/`cwd` のみ)。そのため:

- イベントは **argv** で渡す(herdr 自身の `herdr-agent-state.sh session` と
  同じパターン)。`sessionStart`/`userPromptSubmitted`/`agentStop` に
  `… report` を、`sessionEnd` に `… clear` を登録する。
- model は payload からではなく **`~/.copilot/settings.json` の `.model`**
  (Copilot CLI 自身が永続化する現在のモデル設定)から読む。`/model` 直後は
  Copilot 側の書き込みタイミング次第で遅延しうるが、表示専用なので許容する。

登録は新規の `scripts/register-copilot-hooks`。Copilot の native hook 形は
Claude/Codex の `{matcher?, hooks:[...]}` ネストと違い `{type, bash,
timeoutSec}` を camelCase イベント配列に直置きする形なので(herdr 自身が
`~/.copilot/settings.json` の `.hooks.SessionStart` に入れている既存エントリと
同じ形)、専用のマージャーにした。イベント名は camelCase の native 形を使う
(Claude 形式の PascalCase も公式に受理されるので、herdr の `SessionStart` と
共存する)。`~/.copilot/settings.json` を activation で書くのはこれだけなので
`entryAfter [ "writeBoundary" ]` で足りる。

### 見送った表示: tab-bar usage

Codex の rate-limit(`/status` 相当)・Copilot の premium request quota も
同じ調査で洗ったが、いずれも安定した取得手段が無い(Codex はセッションの
rollout JSONL や experimental な app-server API、Copilot は 2026-06 の
billing 移行で legacy 化予定の REST エンドポイントのみ)。tab-bar 拡張は
実装せず、調査メモを添えて別 Issue(#117)に切り出した。

## アクティブ行の視認性(`[theme.custom]`)

catppuccin テーマ既定の `active_row_bg` は base(`#1E1E2E`)とほぼ同系の暗色で、
~17 workspace を並走させるとサイドバーのどの行がフォーカス中かが判別しづらい。

### 最初の 2 回の対策と、それぞれの見落とし

1. **surface1 案**(`active_row_bg = "#45475A"` への引き上げのみ)— WCAG 相対
   輝度で矩形 vs 地が **1.9:1**(非テキスト UI 基準 SC 1.4.11 の 3:1 未達)に
   とどまった(WCAG 2.1: https://www.w3.org/TR/WCAG21/)。
2. **地の暗色化案**(`sidebar_bg = "#11111B"` Mocha crust への明示塗り)—
   矩形 vs 地は 2.6:1 まで改善したが、herdr サイドバーだけが周囲の Alacritty
   ペインと違う不透明な黒い板になった。原因は Alacritty の `window.opacity`
   の仕様: **既定背景色のセルにしか適用されない**(man alacritty.toml(5)、
   0.15.1 で確認、2026-09-21 取得; alacritty/alacritty PR #847
   "Fix solid background color opacity",
   https://github.com/alacritty/alacritty/pull/847、2026-09-21 取得)。herdr
   0.8.2 の catppuccin テーマは `sidebar_bg` の既定値が `Color::Reset`(端末
   既定背景 = 透過対象)なので(v0.8.2 `src/app/state.rs`、2026-09-21 取得
   https://github.com/herdrdev/herdr/blob/v0.8.2/src/app/state.rs)、素の
   テーマのままサイドバーは既に端末の透過(当時 opacity 0.75)を継承していた
   — 1. の計測値は純 hex 前提で、実際の半透明表示を反映していなかった。
   `sidebar_bg` を明示 RGB で上書きした時点でそのセルは不透明になり、
   透過している周囲のペインとの断絶が生まれた。

### 現在の対策: 主戦場を端末側に移す

透過が高すぎて地(既定背景セル)が周囲の壁紙・背後ウィンドウと過剰に混ざり
matrix 全体が washed out していたことが、「ハイライトが薄い」と感じる主因
だった。TUI 側の塗り足しでなく、**`config/alacritty/alacritty.toml` の
`window.opacity` を 0.75 → 0.95 に引き上げる**ことで地をほぼ純色に戻す。
`[theme.custom]` 側は次の 2 点だけを担当する:

1. **`sidebar_bg = "reset"` を明示する** — テーマ既定と同値(暗黙の
   `Color::Reset` を上書きしない)だが、herdr 0.9.x で導入された透過継承の
   regression(既定省略時の透過が壊れる、herdrdev/herdr#3773、
   https://github.com/herdrdev/herdr/issues/3773、2026-09-21 取得)の
   workaround が「`sidebar_bg`/`panel_bg` に `"reset"` を明示すること」なので、
   将来の herdr バンプへの前方互換保険として書いておく。
2. **矩形(`active_row_bg`)をグレー階調でなく accent 系タイントにする** —
   `active_row_bg = "#52567A"` は lavender `#B4BEFE` を base に ~35% ブレンド
   した非純正 hex(Catppuccin Mocha の役割トークンには存在しない値)。同輝度
   でもグレーとの色相差で知覚的に見つけやすくなる。この発想は VS Code 標準
   dark テーマの `list.activeSelectionBackground`(`#04395E`、青系タイント)に
   倣った(https://github.com/microsoft/vscode/blob/main/src/vs/platform/theme/common/colors/listColors.ts、
   2026-09-21 取得)。矩形自体は明示 bg セルなので opacity 0.95 の影響を受けず、
   常に不透明に近い状態で描かれる。

計測値(純 hex 前提、opacity 1.0 相当): 矩形 vs 地 2.6:1(+ 色相差)、矩形上の
text `#CDD6F4` は 4.9:1。3:1 には届いていないが、この上ではテキストの可読性を
壊す明度が必要になるトレードオフを確認済みで、色相差での補完を優先した。
opacity 0.95 の下では地がほぼ純色 base に近づくため、この計測値がおおむね
実表示に一致する(0.75 時代は地が壁紙と 25% 合成されており、この前提が
崩れていた)。

navigate モードのカーソル行(`selection_bg`)もテーマ既定の surface0 だと
薄いため、旧 `active_row_bg` だった `#45475A`(surface1)に引き上げている。

行内の控えめ情報(`$branch`/`$ctx`/`$cost`/`$effort`/`terminal_title_stripped`)
も overlay1(矩形上 2.5:1)から subtext0 `#A6ADC8`(矩形上 3.2:1)に引き上げて
いる。herdr の theme.custom 語彙(`herdr --default-config` 0.8.2 で確認、一次
情報は https://raw.githubusercontent.com/herdrdev/herdr/v0.8.2/docs/next/website/src/data/config-reference.json )
に `active_row_fg` は存在せず、行の文字色はトークンの静的 `fg` 指定のみ —
「アクティブ時だけ文字を明るくする」動的表現はできないため、常時 subtext0 に
底上げする形で代替した。同じ理由で `dim = true` 修飾も使っていない: dim の
実効輝度は端末レンダラ依存で不定なため、暗さは fg の明示色だけで表現する。

`herdr --default-config`(0.8.2)で確認した通り `[theme.custom]` はテーマ本体を
書き換えずに個別トークンだけ差し替えられるので、テーマ更新に追従したまま
維持できる。

### 学び: TUI の色計測は端末の透過設定込みで行う

WCAG 相対輝度は「実際に画面に出る RGB 値」を前提にした指標であり、config に
書いた純 hex 値ではない。半透明端末(`window.opacity < 1.0`)上では、TUI 側の
セルが明示 bg を持つか既定背景のままかで実効色が変わる(Alacritty は前者を
不透明、後者を透過合成で描く)。今回のように「TUI 側の色を変えたのに実機の
見え方が計算と合わない」ときは、まず端末の透過設定とその適用範囲仕様を疑う
— TUI のテーマ機構だけを見ていては原因に辿り着けない。

## 既知の制約・運用ノート

- **statusline の巻き戻り**: `~/.claude/settings.json` の `statusLine` は activation
  (`registerClaudeStatusLine` → `syncStatusLine`)が宣言値に合わせるので、
  `/statusline` で手動変更しても次の `home-manager switch` で戻る。変更はこの
  リポジトリの `config/claude/statusline/claude-statusline.sh` を編集すること。
- **hook / statusLine の撤回は forward switch でのみ効く**: `registerHooks` /
  `syncStatusLine` は `retiredHookEntries` / `retiredStatusLineCommands`
  (`home/modules/claude.nix`)に載っている command を完全一致で settings.json から
  削除する。これが効くのは新しい generation への **forward** switch だけで、
  home-manager generation の `--rollback` では効かない — rollback 先の世代の
  activation は当時のコードをそのまま実行するため、撤回機構自体がまだ無い世代に
  戻れば孤児が再発し得る(`retiredPermissionRules` も同じ限界を持つ、
  home-manager の generation モデル一般の制約)。緊急 rollback 後は
  `docs/operations.md` の孤児チェックを走らせること。
- **herdr 統合 hook との共存**: herdr は自分の `herdr-agent-state.sh`(編集禁止、
  integration 更新で上書き)を settings.json に登録する。registerHooks は command
  文字列が異なるエントリに触れず、herdr も自ファイル以外に触れないので衝突しない
  (herdr の書き込みは `.hooks.SessionStart` の自ファイルと `~/.claude/hooks/`,
  `~/.codex/`, `~/.copilot/hooks/`, `~/.config/devin/` への統合ファイル配備だけで、
  `statusLine` には一切触れない — 実行バイナリの静的解析で確認済み)。この
  integration hook は herdr のネイティブ agent セッション復元
  (`[session] resume_agents_on_restore`、既定 on)が動く前提条件でもある
  — 無いと `herdr server` 再起動後の復元は layout だけになり、claude ペインは
  素のシェルとして戻る。onboarding フローでしか入らず、本リポジトリは
  `config/herdr/config.toml` で `onboarding = false` を配備しているため、
  `home/modules/herdr.nix` の `home.activation.installHerdrClaudeIntegration`
  が `herdr integration install claude` を代わりに(未導入時のみ)実行する。
  同じ理由・同じ仕組みで codex/copilot にも `installHerdrCodexIntegration` /
  `installHerdrCopilotIntegration` を用意している(#168)。
- **Herdr 外では無害**: どちらのスクリプトも `HERDR_ENV=1` と socket/pane 環境変数を
  ガードにしており、素のターミナルでは statusline の表示だけが動く(ADR-0005 の
  binary-existence gating に倣い、欠如時は黙って no-op)。
- **herdr バイナリは nix 管理**(`home/modules/herdr.nix`)。nixos-26.05 に herdr が
  無いため flake の `nixpkgs-unstable` input から overlay で取っている — 安定
  チャネルに入ったら input ごと畳むこと(Issue #42)。旧 self-installed
  `~/.local/bin/herdr` は `home.activation.quarantineSelfInstalledHerdr` が
  `.pre-nix` へ自動的に退避する(手動削除の手順は置かない — ADR-0001 のゼロ手
  作業の原則)。
- **config.toml は store symlink**。herdr の実行時書き込み(in-TUI の theme /
  sound / toast / status indicators / agent border labels トグル、onboarding、
  channel set)はすべて失敗する。CLI 経路(`herdr channel set`)は
  `Permission denied` を返して停止することを実機で確認済み。in-TUI 経路は
  `logging::config_write_failed` に記録されて飲み込まれ、UI のトグルが黙って
  元に戻るだけ(見える失敗ではない)。設定変更はこのリポジトリの
  `config/herdr/config.toml` を編集して `home-manager switch` + 反映は
  `herdr server reload-config`。
